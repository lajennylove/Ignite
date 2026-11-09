//! The `ignite` command line: `ignite user create --email <email> [--name <name>]`.
//!
//! The command checks `DATABASE_URL`, connects, runs the panel's migrations and
//! checks for an existing user before it reads the password. Interactively the
//! password is read from the terminal without echoing, and asked for twice. When
//! standard input is not a terminal, or with `--password-stdin`, the password is
//! the first line of standard input, so a script can pipe it in. There is no
//! environment variable for the password. Apps call [`main`] from a small binary
//! of their own, after loading `.env` the way the workspace's `ignite` binary does.

use std::process::exit;

use diesel::mysql::MysqlConnection;
use diesel::prelude::*;

use crate::{run_migrations, users};

const USAGE: &str = "usage: ignite user create --email <email> [--name <name>] [--password-stdin]

Reads DATABASE_URL. Asks for the password without echoing it, or reads it from
standard input when standard input is not a terminal or with --password-stdin.";

/// The shortest password the CLI accepts.
const MIN_PASSWORD_CHARS: usize = 8;

/// Runs the command in the process arguments and exits.
pub fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let exit_code = match args.as_slice() {
        [user, create, rest @ ..] if user == "user" && create == "create" => create_user(rest),
        _ => {
            eprintln!("{USAGE}");
            2
        }
    };
    exit(exit_code);
}

/// Parses `--email` and `--name`, then creates the user. Returns the exit code.
fn create_user(args: &[String]) -> i32 {
    let mut email = None;
    let mut name = None;
    let mut password_stdin = false;
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        match flag.as_str() {
            "--email" => match rest.next() {
                Some(v) => email = Some(v.clone()),
                None => {
                    eprintln!("{USAGE}");
                    return 2;
                }
            },
            "--name" => match rest.next() {
                Some(v) => name = Some(v.clone()),
                None => {
                    eprintln!("{USAGE}");
                    return 2;
                }
            },
            "--password-stdin" => password_stdin = true,
            _ => {
                eprintln!("{USAGE}");
                return 2;
            }
        }
    }
    let Some(email) = email
        .map(|e| e.trim().to_string())
        .filter(|e| !e.is_empty())
    else {
        eprintln!("--email is required.\n\n{USAGE}");
        return 2;
    };
    let name = name.unwrap_or_default();

    let url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("DATABASE_URL must be set.");
            return 1;
        }
    };
    let mut conn = match MysqlConnection::establish(&url) {
        Ok(conn) => conn,
        Err(error) => {
            eprintln!("Could not connect to DATABASE_URL: {error}");
            return 1;
        }
    };
    if let Err(error) = run_migrations(&mut conn) {
        eprintln!("Could not run the panel's migrations: {error}");
        return 1;
    }
    // Everything that can fail without a password runs before the prompt.
    match users::find_by_email(&mut conn, &email) {
        Ok(Some(_)) => {
            eprintln!("A user with the email {email} already exists.");
            return 1;
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!("Could not check for an existing user: {error}");
            return 1;
        }
    }

    let password = match read_password(password_stdin) {
        Ok(password) => password,
        Err(message) => {
            eprintln!("{message}");
            return 1;
        }
    };

    match users::create_user(&mut conn, &name, &email, &password) {
        Ok(user) => {
            println!("Created user #{} for {}.", user.id, user.email);
            0
        }
        Err(diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::UniqueViolation,
            _,
        )) => {
            eprintln!("A user with the email {email} already exists.");
            1
        }
        Err(error) => {
            eprintln!("Could not create the user: {error}");
            1
        }
    }
}

/// Reads the password: from standard input when `from_stdin` is set or when standard
/// input is not a terminal, and otherwise from the terminal, asked for twice.
fn read_password(from_stdin: bool) -> Result<String, String> {
    use std::io::{BufRead, IsTerminal};

    let stdin = std::io::stdin();
    if from_stdin && stdin.is_terminal() {
        return Err("--password-stdin reads the password from a pipe, not a terminal.".to_string());
    }
    if from_stdin || !stdin.is_terminal() {
        let mut line = String::new();
        stdin
            .lock()
            .read_line(&mut line)
            .map_err(|e| format!("Could not read the password from standard input: {e}"))?;
        let password = line.trim_end_matches(['\n', '\r']).to_string();
        if password.is_empty() {
            return Err("No password on standard input.".to_string());
        }
        return check_length(password);
    }

    let password = rpassword::prompt_password("Password: ")
        .map_err(|e| format!("Could not read the password: {e}"))?;
    let confirm = rpassword::prompt_password("Confirm password: ")
        .map_err(|e| format!("Could not read the password: {e}"))?;
    if password != confirm {
        return Err("The passwords do not match.".to_string());
    }
    check_length(password)
}

fn check_length(password: String) -> Result<String, String> {
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(format!(
            "The password must be at least {MIN_PASSWORD_CHARS} characters."
        ));
    }
    Ok(password)
}

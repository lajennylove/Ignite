//! The `ignite` command line: `ignite user create --email <email> [--name <name>]`.
//!
//! The command checks `DATABASE_URL`, connects, runs the panel's migrations and
//! checks for an existing user before it asks for a password. The password is
//! read from the terminal without echoing, and asked for twice. Apps call
//! [`main`] from a small binary of their own, after loading `.env` the way the
//! workspace's `ignite` binary does.

use std::process::exit;

use diesel::mysql::MysqlConnection;
use diesel::prelude::*;

use crate::{run_migrations, users};

const USAGE: &str = "usage: ignite user create --email <email> [--name <name>]

Reads DATABASE_URL. Asks for the password without echoing it.";

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
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let value = rest.next();
        match (flag.as_str(), value) {
            ("--email", Some(v)) => email = Some(v.clone()),
            ("--name", Some(v)) => name = Some(v.clone()),
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

    let password = match read_password() {
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

/// Asks for the password twice, without echoing it.
fn read_password() -> Result<String, String> {
    let password = rpassword::prompt_password("Password: ")
        .map_err(|e| format!("Could not read the password: {e}"))?;
    let confirm = rpassword::prompt_password("Confirm password: ")
        .map_err(|e| format!("Could not read the password: {e}"))?;
    if password != confirm {
        return Err("The passwords do not match.".to_string());
    }
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(format!(
            "The password must be at least {MIN_PASSWORD_CHARS} characters."
        ));
    }
    Ok(password)
}

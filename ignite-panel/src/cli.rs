//! The `ignite` command line:
//!
//! - `ignite migrate` runs the panel's pending migrations, and prints what it applied.
//!   Run it as a deploy step, before the first start. The app never migrates on boot.
//! - `ignite migrate:status` lists each migration as Applied or Pending.
//! - `ignite migrate:rollback` reverts the most recently applied migration with its
//!   `down.sql`. One step per run.
//! - `ignite user create --email <email> [--name <name>]` creates a user. It does not
//!   migrate: when migrations are pending it stops and says to run `ignite migrate`.
//! - `ignite role create <name>` creates a role.
//! - `ignite role grant <role> <permission>` gives a role a permission, such as `users.delete`.
//! - `ignite user role add <email> <role>` and `ignite user role remove <email> <role>` give
//!   a user a role or take one away. The last super-admin cannot lose the role.
//! - `ignite user roles <email>` lists the roles of a user.
//! - `ignite user disable <email>` and `ignite user enable <email>` stop or restore a user's
//!   sign-in. Disabling ends the user's sessions, and the last enabled super-admin cannot be
//!   disabled.
//! - `ignite user password <email>` sets a new password for an existing user, and ends
//!   their sessions, as a password reset does.
//!
//! The user command checks `DATABASE_URL`, connects and checks for an existing user
//! before it reads the password. Interactively the password is read from the terminal
//! without echoing, and asked for twice. When standard input is not a terminal, or with
//! `--password-stdin`, the password is the first line of standard input, so a script can
//! pipe it in. There is no environment variable for the password. Apps call [`main`]
//! from a small binary of their own, after loading `.env` the way the workspace's
//! `ignite` binary does.

use std::process::exit;

use diesel::mysql::MysqlConnection;
use diesel::prelude::*;

use crate::authz::{self, AuthzError};
use crate::{migrate, migration_status as status, pending_migrations, rollback, users};

const USAGE: &str = "usage:
  ignite migrate
  ignite migrate:status
  ignite migrate:rollback
  ignite user create --email <email> [--name <name>] [--password-stdin]
  ignite role create <name>
  ignite role grant <role> <permission>
  ignite user role add <email> <role>
  ignite user role remove <email> <role>
  ignite user roles <email>
  ignite user disable <email>
  ignite user enable <email>
  ignite user password <email> [--password-stdin]

`migrate` runs the panel's pending migrations. Run it before the first start.
`migrate:status` lists each migration as Applied or Pending. `migrate:rollback`
reverts the most recently applied migration, one step per run.
`user create` reads DATABASE_URL and asks for the password without echoing it, or
reads it from standard input when standard input is not a terminal or with
--password-stdin. It does not migrate; run `ignite migrate` first.
`role create` makes a role. `role grant` gives it a permission, such as `users.delete`;
the app must have started once so the permission exists. `user role add` and
`user role remove` change a user's roles, and the last super-admin cannot lose the role.
`user roles` lists a user's roles. `user disable` stops a user from signing in and ends
their sessions; `user enable` lets them sign in again. `user password` reads a new password
the same way `user create` does, and ends the user's sessions.";

/// The shortest password the CLI accepts.
const MIN_PASSWORD_CHARS: usize = 8;

/// Runs the command in the process arguments and exits.
pub fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let exit_code = match args.as_slice() {
        [migrate] if migrate == "migrate" => migrate_database(),
        [status] if status == "migrate:status" => migration_status(),
        [rollback] if rollback == "migrate:rollback" => rollback_migration(),
        [user, create, rest @ ..] if user == "user" && create == "create" => create_user(rest),
        [role, create, name] if role == "role" && create == "create" => create_role(name),
        [role, grant, role_name, permission] if role == "role" && grant == "grant" => {
            grant_permission(role_name, permission)
        }
        [user, role, add, email, role_name] if user == "user" && role == "role" && add == "add" => {
            assign_role(email, role_name)
        }
        [user, role, remove, email, role_name]
            if user == "user" && role == "role" && remove == "remove" =>
        {
            remove_role(email, role_name)
        }
        [user, roles, email] if user == "user" && roles == "roles" => list_user_roles(email),
        [user, disable, email] if user == "user" && disable == "disable" => disable_user(email),
        [user, enable, email] if user == "user" && enable == "enable" => enable_user(email),
        [user, password, rest @ ..] if user == "user" && password == "password" => {
            reset_password(rest)
        }
        _ => {
            eprintln!("{USAGE}");
            2
        }
    };
    exit(exit_code);
}

/// Connects with `DATABASE_URL` and runs the pending migrations. Returns the exit code.
fn migrate_database() -> i32 {
    let mut conn = match connect() {
        Ok(conn) => conn,
        Err(code) => return code,
    };
    match migrate(&mut conn) {
        Ok(applied) if applied.is_empty() => {
            println!("Nothing to migrate.");
            0
        }
        Ok(applied) => {
            for name in applied {
                println!("Applied {name}");
            }
            0
        }
        Err(error) => {
            eprintln!("Migration failed: {error}");
            1
        }
    }
}

/// Lists each migration as Applied or Pending. Returns the exit code.
fn migration_status() -> i32 {
    let mut conn = match connect() {
        Ok(conn) => conn,
        Err(code) => return code,
    };
    match status(&mut conn) {
        Ok(rows) => {
            for (name, applied) in rows {
                let state = if applied { "Applied" } else { "Pending" };
                println!("{state:<8} {name}");
            }
            0
        }
        Err(error) => {
            eprintln!("Could not read the migrations: {error}");
            1
        }
    }
}

/// Reverts the most recently applied migration, one step. Returns the exit code.
fn rollback_migration() -> i32 {
    let mut conn = match connect() {
        Ok(conn) => conn,
        Err(code) => return code,
    };
    match rollback(&mut conn) {
        Ok(Some(name)) => {
            println!("Rolled back one migration: reverted {name}.");
            println!("Run `ignite migrate:rollback` again to revert the next one.");
            0
        }
        Ok(None) => {
            println!("Nothing to roll back.");
            0
        }
        Err(error) => {
            eprintln!("Rollback failed: {error}");
            1
        }
    }
}

/// Connects to the database named by `DATABASE_URL`. Prints the problem and returns the
/// exit code when it cannot.
fn connect() -> Result<MysqlConnection, i32> {
    let url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("DATABASE_URL must be set.");
            return Err(1);
        }
    };
    MysqlConnection::establish(&url).map_err(|error| {
        eprintln!("Could not connect to DATABASE_URL: {error}");
        1
    })
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

    let mut conn = match connect() {
        Ok(conn) => conn,
        Err(code) => return code,
    };
    // The command never migrates. A schema that is behind is refused, with the fix.
    match pending_migrations(&mut conn) {
        Ok(pending) if pending.is_empty() => {}
        Ok(pending) => {
            eprintln!(
                "The panel's tables are not set up: these migrations have not run: {}. \
                 Run `ignite migrate` first.",
                pending.join(", ")
            );
            return 1;
        }
        Err(error) => {
            eprintln!("Could not read the migration table: {error}");
            return 1;
        }
    }
    // Everything that can fail without a password runs before the prompt.
    match users::find_by_email(&mut conn, &email) {
        Ok(Some(_)) => {
            eprintln!("A user with the email {email} already exists.");
            return 1;
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!("Could not check for an existing user: {error}. Run `ignite migrate` if the users table is missing.");
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

/// Connects and checks that every migration has run, which needs `ignite migrate` first when
/// one is pending. On failure it prints the problem and returns the exit code.
fn connect_with_roles() -> Result<MysqlConnection, i32> {
    let mut conn = connect()?;
    match pending_migrations(&mut conn) {
        Ok(pending) if pending.is_empty() => Ok(conn),
        Ok(pending) => {
            eprintln!(
                "The panel's tables are not set up: these migrations have not run: {}. \
                 Run `ignite migrate` first.",
                pending.join(", ")
            );
            Err(1)
        }
        Err(error) => {
            eprintln!("Could not read the migration table: {error}");
            Err(1)
        }
    }
}

/// Prints the outcome of a role command: the message on success, or the problem and exit 1.
fn report(result: Result<String, AuthzError>) -> i32 {
    match result {
        Ok(message) => {
            println!("{message}");
            0
        }
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

/// `ignite role create <name>`. Returns the exit code.
fn create_role(name: &str) -> i32 {
    let name = name.trim();
    if name.is_empty() {
        eprintln!("The role name must not be empty.");
        return 2;
    }
    let mut conn = match connect_with_roles() {
        Ok(conn) => conn,
        Err(code) => return code,
    };
    report(authz::create_role(&mut conn, name).map(|()| format!("Created role {name}.")))
}

/// `ignite role grant <role> <permission>`. Returns the exit code.
fn grant_permission(role: &str, permission: &str) -> i32 {
    let mut conn = match connect_with_roles() {
        Ok(conn) => conn,
        Err(code) => return code,
    };
    report(
        authz::grant_permission(&mut conn, role, permission)
            .map(|()| format!("Granted {permission} to {role}.")),
    )
}

/// `ignite user password <email> [--password-stdin]`. Returns the exit code. The user must
/// exist, and the password is read the way `user create` reads it.
fn reset_password(args: &[String]) -> i32 {
    let mut email = None;
    let mut password_stdin = false;
    for arg in args {
        match arg.as_str() {
            "--password-stdin" => password_stdin = true,
            value if email.is_none() && !value.starts_with("--") => {
                email = Some(value.trim().to_string());
            }
            _ => {
                eprintln!("{USAGE}");
                return 2;
            }
        }
    }
    let Some(email) = email.filter(|e| !e.is_empty()) else {
        eprintln!("{USAGE}");
        return 2;
    };

    let mut conn = match connect_with_roles() {
        Ok(conn) => conn,
        Err(code) => return code,
    };
    let user = match users::find_by_email(&mut conn, &email) {
        Ok(Some(user)) => user,
        Ok(None) => {
            eprintln!("The user {email} does not exist.");
            return 1;
        }
        Err(error) => {
            eprintln!("Could not read the users table: {error}");
            return 1;
        }
    };

    let password = match read_password(password_stdin) {
        Ok(password) => password,
        Err(message) => {
            eprintln!("{message}");
            return 1;
        }
    };

    match users::reset_password(&mut conn, user.id, &password) {
        Ok(()) => {
            println!("Changed the password of {email}. Their sessions have ended.");
            0
        }
        Err(error) => {
            eprintln!("Could not change the password: {error}");
            1
        }
    }
}

/// `ignite user disable <email>`. Returns the exit code. A user who is already disabled is
/// reported, and is not an error.
fn disable_user(email: &str) -> i32 {
    let mut conn = match connect_with_roles() {
        Ok(conn) => conn,
        Err(code) => return code,
    };
    report(authz::disable_user(&mut conn, email).map(|changed| {
        if changed {
            format!("Disabled {email}. Their sessions have ended.")
        } else {
            format!("{email} is already disabled.")
        }
    }))
}

/// `ignite user enable <email>`. Returns the exit code. A user who is already enabled is
/// reported, and is not an error.
fn enable_user(email: &str) -> i32 {
    let mut conn = match connect_with_roles() {
        Ok(conn) => conn,
        Err(code) => return code,
    };
    report(authz::enable_user(&mut conn, email).map(|changed| {
        if changed {
            format!("Enabled {email}.")
        } else {
            format!("{email} is already enabled.")
        }
    }))
}

/// `ignite user role add <email> <role>`. Returns the exit code.
fn assign_role(email: &str, role: &str) -> i32 {
    let mut conn = match connect_with_roles() {
        Ok(conn) => conn,
        Err(code) => return code,
    };
    report(
        authz::assign_role(&mut conn, email, role)
            .map(|()| format!("Added role {role} to {email}.")),
    )
}

/// `ignite user role remove <email> <role>`. Returns the exit code. A user who did not have
/// the role is reported, and is not an error.
fn remove_role(email: &str, role: &str) -> i32 {
    let mut conn = match connect_with_roles() {
        Ok(conn) => conn,
        Err(code) => return code,
    };
    report(authz::remove_role(&mut conn, email, role).map(|removed| {
        if removed {
            format!("Removed role {role} from {email}.")
        } else {
            format!("{email} did not have the role {role}.")
        }
    }))
}

/// `ignite user roles <email>`. Returns the exit code.
fn list_user_roles(email: &str) -> i32 {
    let mut conn = match connect_with_roles() {
        Ok(conn) => conn,
        Err(code) => return code,
    };
    match authz::user_roles(&mut conn, email) {
        Ok(roles) if roles.is_empty() => {
            println!("{email} has no roles.");
            0
        }
        Ok(roles) => {
            for role in roles {
                println!("{role}");
            }
            0
        }
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

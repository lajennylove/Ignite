//! The `ignite` command line:
//!
//! - `ignite migrate` runs the panel's pending migrations, and prints what it applied.
//!   Run it as a deploy step, before the first start. The app never migrates on boot.
//! - `ignite migrate:status` lists each migration as Applied or Pending.
//! - `ignite migrate:rollback` reverts the most recently applied batch, the migrations that
//!   one `ignite migrate` applied, each with its `down.sql`, last first.
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

use orbit_orm::Database;
use rocket::tokio::runtime::Runtime;

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
reverts the last batch: every migration that the last `migrate` applied, last first.
It is not one step: after a `migrate` that applied three migrations, it reverts all three.
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
    let (runtime, db) = match connect_orbit() {
        Ok(connection) => connection,
        Err(code) => return code,
    };
    match runtime.block_on(migrate(&db)) {
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
    let (runtime, db) = match connect_orbit() {
        Ok(connection) => connection,
        Err(code) => return code,
    };
    match runtime.block_on(status(&db)) {
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

/// Reverts the most recently applied batch of migrations. Returns the exit code.
fn rollback_migration() -> i32 {
    let (runtime, db) = match connect_orbit() {
        Ok(connection) => connection,
        Err(code) => return code,
    };
    match runtime.block_on(rollback(&db)) {
        Ok(names) if names.is_empty() => {
            println!("Nothing to roll back.");
            0
        }
        Ok(names) => {
            println!("Rolled back the last batch: {} migrations.", names.len());
            for name in &names {
                println!("reverted {name}.");
            }
            println!("Run `ignite migrate:rollback` again to revert the batch before it.");
            0
        }
        Err(error) => {
            eprintln!("Rollback failed: {error}");
            1
        }
    }
}

/// Reads `DATABASE_URL`. Prints the problem and returns the exit code when it is not set.
fn database_url() -> Result<String, i32> {
    std::env::var("DATABASE_URL").map_err(|_| {
        eprintln!("DATABASE_URL must be set.");
        1
    })
}

/// Connects Orbit to `DATABASE_URL`. The commands drive it with `block_on` on the runtime that
/// is returned, so the CLI stays synchronous. Prints the problem and returns the exit code when
/// it cannot connect.
fn connect_orbit() -> Result<(Runtime, Database), i32> {
    let url = database_url()?;
    let runtime = Runtime::new().map_err(|error| {
        eprintln!("Could not start the runtime: {error}");
        1
    })?;
    let db = runtime.block_on(Database::connect(&url)).map_err(|error| {
        eprintln!("Could not connect to DATABASE_URL: {error}");
        1
    })?;
    Ok((runtime, db))
}

/// Connects Orbit and refuses to continue when migrations are pending, which needs
/// `ignite migrate` first. Returns the runtime that drives the database. On failure it prints
/// the problem and returns the exit code.
fn check_migrated() -> Result<(Runtime, Database), i32> {
    let (runtime, db) = connect_orbit()?;
    match runtime.block_on(pending_migrations(&db)) {
        Ok(pending) if pending.is_empty() => Ok((runtime, db)),
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

/// Connects and checks that every migration has run. See [`check_migrated`].
fn connect_with_roles() -> Result<(Runtime, Database), i32> {
    check_migrated()
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

    // The command never migrates. A schema that is behind is refused, with the fix.
    let (runtime, db) = match check_migrated() {
        Ok(connection) => connection,
        Err(code) => return code,
    };
    // Everything that can fail without a password runs before the prompt.
    match runtime.block_on(users::find_by_email(&db, &email)) {
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

    match runtime.block_on(users::create_user(&db, &name, &email, &password)) {
        Ok(user) => {
            println!("Created user #{} for {}.", user.id, user.email);
            0
        }
        Err(orbit_orm::Error::UniqueViolation { .. }) => {
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
    let (runtime, db) = match connect_with_roles() {
        Ok(connection) => connection,
        Err(code) => return code,
    };
    report(
        runtime
            .block_on(authz::create_role(&db, name))
            .map(|()| format!("Created role {name}.")),
    )
}

/// `ignite role grant <role> <permission>`. Returns the exit code.
fn grant_permission(role: &str, permission: &str) -> i32 {
    let (runtime, db) = match connect_with_roles() {
        Ok(connection) => connection,
        Err(code) => return code,
    };
    report(
        runtime
            .block_on(authz::grant_permission(&db, role, permission))
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

    let (runtime, db) = match connect_with_roles() {
        Ok(connection) => connection,
        Err(code) => return code,
    };
    let user = match runtime.block_on(users::find_by_email(&db, &email)) {
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

    match runtime.block_on(users::reset_password(&db, user.id, &password)) {
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
    let (runtime, db) = match connect_with_roles() {
        Ok(connection) => connection,
        Err(code) => return code,
    };
    report(
        runtime
            .block_on(authz::disable_user(&db, email))
            .map(|changed| {
                if changed {
                    format!("Disabled {email}. Their sessions have ended.")
                } else {
                    format!("{email} is already disabled.")
                }
            }),
    )
}

/// `ignite user enable <email>`. Returns the exit code. A user who is already enabled is
/// reported, and is not an error.
fn enable_user(email: &str) -> i32 {
    let (runtime, db) = match connect_with_roles() {
        Ok(connection) => connection,
        Err(code) => return code,
    };
    report(
        runtime
            .block_on(authz::enable_user(&db, email))
            .map(|changed| {
                if changed {
                    format!("Enabled {email}.")
                } else {
                    format!("{email} is already enabled.")
                }
            }),
    )
}

/// `ignite user role add <email> <role>`. Returns the exit code.
fn assign_role(email: &str, role: &str) -> i32 {
    let (runtime, db) = match connect_with_roles() {
        Ok(connection) => connection,
        Err(code) => return code,
    };
    report(
        runtime
            .block_on(authz::assign_role(&db, email, role))
            .map(|()| format!("Added role {role} to {email}.")),
    )
}

/// `ignite user role remove <email> <role>`. Returns the exit code. A user who did not have
/// the role is reported, and is not an error.
fn remove_role(email: &str, role: &str) -> i32 {
    let (runtime, db) = match connect_with_roles() {
        Ok(connection) => connection,
        Err(code) => return code,
    };
    report(
        runtime
            .block_on(authz::remove_role(&db, email, role))
            .map(|removed| {
                if removed {
                    format!("Removed role {role} from {email}.")
                } else {
                    format!("{email} did not have the role {role}.")
                }
            }),
    )
}

/// `ignite user roles <email>`. Returns the exit code.
fn list_user_roles(email: &str) -> i32 {
    let (runtime, db) = match connect_with_roles() {
        Ok(connection) => connection,
        Err(code) => return code,
    };
    match runtime.block_on(authz::user_roles(&db, email)) {
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

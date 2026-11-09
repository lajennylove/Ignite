//! The `ignite` command for this workspace. Apps have their own copy, which
//! loads `.env` the same way.

fn main() {
    // The app reads DATABASE_URL from .env, so the CLI does too.
    let _ = dotenvy::dotenv();
    ignite_panel::cli::main();
}

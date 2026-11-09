# Local patch to Diesel 2.3.14

Only `src/mysql/connection/bind.rs` differs from the crates.io release. In
`OutputBinds::populate_dynamic_buffers`, the lengths of the bound columns are
saved before the result is rebound, and restored afterwards.

Why: after a truncated fetch, Diesel rebinds the result with
`mysql_stmt_bind_result`. libmariadb 3.4.x sets the length of each bound column
to zero on that call, so the first row of a text query came back as "". Older
Diesel 2.x releases have the same code path, so this is not a 2.3.14 regression.

Remove this directory and the `[patch.crates-io]` entry in the workspace
`Cargo.toml` once a Diesel release contains the fix.

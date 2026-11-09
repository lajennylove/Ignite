//! The MySQL/MariaDB connection pool that the panel and resources share.

use diesel::mysql::MysqlConnection;
use diesel::r2d2::{ConnectionManager, Pool as R2d2Pool, PooledConnection};

/// The shared pool. Build it with [`build_pool`].
pub type Pool = R2d2Pool<ConnectionManager<MysqlConnection>>;

/// A connection borrowed from the [`Pool`]. Returned to the pool when dropped.
pub type Conn = PooledConnection<ConnectionManager<MysqlConnection>>;

/// Builds a pool for a `mysql://user:pass@host:port/db` URL.
///
/// The pool does not open connections until the first borrow fails or
/// succeeds, so an unreachable database surfaces on the first request, or
/// here if `r2d2` can check the connection at build time.
pub fn build_pool(url: &str) -> Result<Pool, diesel::r2d2::PoolError> {
    let manager = ConnectionManager::<MysqlConnection>::new(url);
    R2d2Pool::builder().build(manager)
}

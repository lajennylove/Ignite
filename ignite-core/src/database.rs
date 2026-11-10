//! The MySQL/MariaDB connection pool that the panel and resources share.

use diesel::mysql::MysqlConnection;
use diesel::r2d2::{ConnectionManager, Pool as R2d2Pool, PooledConnection};

/// The shared pool. Build it with [`build_pool`].
pub type Pool = R2d2Pool<ConnectionManager<MysqlConnection>>;

/// A connection borrowed from the [`Pool`]. Returned to the pool when dropped.
pub type Conn = PooledConnection<ConnectionManager<MysqlConnection>>;

/// Builds a pool for a `mysql://user:pass@host:port/db` URL.
///
/// The builder uses r2d2's defaults: up to 10 connections, all opened before
/// `build` returns, because the minimum idle count defaults to the maximum. So
/// an unreachable database makes this return an error, after r2d2's 30-second
/// connection timeout, rather than surfacing on the first request.
pub fn build_pool(url: &str) -> Result<Pool, diesel::r2d2::PoolError> {
    let manager = ConnectionManager::<MysqlConnection>::new(url);
    R2d2Pool::builder().build(manager)
}

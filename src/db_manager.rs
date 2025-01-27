#[cfg(feature = "db-mongo")]
mod mongo_db_manager;
#[cfg(feature = "db-redis")]
mod redis_db_manager;
#[cfg(feature = "db-sled")]
mod sled_db_manager;
#[cfg(not(any(feature = "db-mongo", feature = "db-redis", feature = "db-sled")))]
mod no_db_manager;
mod traits;
mod utils;

#[cfg(feature = "db-mongo")]
pub use mongo_db_manager::MongoDbManager;
#[cfg(feature = "db-redis")]
pub use redis_db_manager::RedisDbManager;
#[cfg(feature = "db-sled")]
pub use sled_db_manager::SledDbManager;

#[cfg(not(any(feature = "db-mongo", feature = "db-redis", feature = "db-sled")))]
pub use no_db_manager::NoDbManager;
pub use traits::DbManager;
pub mod errors;
pub mod model;
pub mod observe;
pub mod prelude;

pub mod spi;

#[cfg(feature = "mock")]
pub mod mock;

pub mod surreal;

pub use errors::StorageError;
pub use model::*;

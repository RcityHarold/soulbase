#![cfg(feature = "surreal")]

mod mapper;
pub mod schema;
mod store;

pub use schema::migrations;
pub use store::{
    apply_migrations, SurrealDeadStore, SurrealIdempoStore, SurrealOutboxStore, SurrealSagaStore,
    SurrealTxStore,
};

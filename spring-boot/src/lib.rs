//! [spring-boot](https://spring-rs.github.io/docs/plugins/plugin-by-self/)

/// App Builder
pub mod app;
/// App Config
pub mod config;
pub mod error;
pub mod log;
pub mod plugin;
pub use async_trait::async_trait;
pub use tracing;
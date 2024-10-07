//! [spring-sqlx](https://spring-rs.github.io/docs/plugins/spring-sqlx/)
#![doc(html_favicon_url = "https://spring-rs.github.io/favicon.ico")]
#![doc(html_logo_url = "https://spring-rs.github.io/logo.svg")]

pub mod config;
pub extern crate sqlx;
use anyhow::Context;
use config::SqlxConfig;
use spring::app::AppBuilder;
use spring::config::ConfigRegistry;
use spring::error::Result;
use spring::plugin::Plugin;
use spring::{async_trait, App};
use sqlx::any::AnyPoolOptions;
use std::sync::Arc;
use std::time::Duration;

pub type ConnectPool = sqlx::AnyPool;

pub struct SqlxPlugin;

#[async_trait]
impl Plugin for SqlxPlugin {
    async fn build(&self, app: &mut AppBuilder) {
        sqlx::any::install_default_drivers();
        let config = app
            .get_config::<SqlxConfig>()
            .expect("sqlx plugin config load failed");

        let connect_pool = Self::connect(&config)
            .await
            .expect("sqlx plugin load failed");

        tracing::info!("sqlx connection success");

        app.add_component(connect_pool)
            .add_shutdown_hook(|app: Arc<App>| Box::new(Self::close_db_connection(app)));
    }
}

impl SqlxPlugin {
    pub async fn connect(config: &config::SqlxConfig) -> Result<ConnectPool> {
        let mut opt = AnyPoolOptions::new();
        opt = opt
            .max_connections(config.max_connections)
            .min_connections(config.min_connections);

        if let Some(acquire_timeout) = config.acquire_timeout {
            opt = opt.acquire_timeout(Duration::from_millis(acquire_timeout));
        }
        if let Some(idle_timeout) = config.idle_timeout {
            opt = opt.idle_timeout(Duration::from_millis(idle_timeout));
        }
        if let Some(connect_timeout) = config.connect_timeout {
            opt = opt.max_lifetime(Duration::from_millis(connect_timeout));
        }

        Ok(opt
            .connect(&config.uri)
            .await
            .with_context(|| format!("sqlx connection failed: {}", config.uri))?)
    }

    async fn close_db_connection(app: Arc<App>) -> Result<()> {
        app.get_component::<ConnectPool>()
            .expect("sqlx connect pool not exists")
            .close()
            .await;
        Ok(())
    }
}

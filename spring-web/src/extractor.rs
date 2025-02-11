pub use axum::extract::*;

use crate::error::{Result, WebError};
use crate::AppState;
use anyhow::Context;
use axum::http::request::Parts;
use spring::config::{ConfigRegistry, Configurable};
use spring::plugin::ComponentRegistry;
use std::ops::{Deref, DerefMut};
use std::result::Result as StdResult;

/// Extending the functionality of RequestParts
pub trait RequestPartsExt {
    /// get AppState
    fn get_app_state(&self) -> &AppState;

    /// get Component
    fn get_component<T: Clone + Send + Sync + 'static>(&self) -> Result<T>;

    /// get Config
    fn get_config<T: serde::de::DeserializeOwned + Configurable>(&self) -> Result<T>;
}

impl RequestPartsExt for Parts {
    fn get_app_state(&self) -> &AppState {
        self.extensions
            .get::<AppState>()
            .expect("extract app state from extension failed")
    }

    fn get_component<T: Clone + Send + Sync + 'static>(&self) -> Result<T> {
        Ok(self
            .get_app_state()
            .app
            .try_get_component()
            .context("get_component failed")?)
    }

    fn get_config<T: serde::de::DeserializeOwned + Configurable>(&self) -> Result<T> {
        self.get_app_state()
            .app
            .get_config::<T>()
            .map_err(|e| WebError::ConfigDeserializeErr(std::any::type_name::<T>(), e))
    }
}

/// Extract the components registered by the plugin from AppState
pub struct Component<T: Clone>(pub T);

impl<T, S> FromRequestParts<S> for Component<T>
where
    T: Clone + Send + Sync + 'static,
    S: Sync
{
    type Rejection = WebError;

    async fn from_request_parts(parts: &mut Parts, _s: &S) -> StdResult<Self, Self::Rejection> {
        parts.get_component::<T>().map(|c| Component(c))
    }
}

impl<T: Clone> Deref for Component<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: Clone> DerefMut for Component<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub struct Config<T>(pub T)
where
    T: serde::de::DeserializeOwned + Configurable;

impl<T, S> FromRequestParts<S> for Config<T>
where
    T: serde::de::DeserializeOwned + Configurable,
    S: Sync
{
    type Rejection = WebError;

    async fn from_request_parts(parts: &mut Parts, _s: &S) -> StdResult<Self, Self::Rejection> {
        parts.get_config().map(|c| Config(c))
    }
}

impl<T> Deref for Config<T>
where
    T: serde::de::DeserializeOwned + Configurable,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for Config<T>
where
    T: serde::de::DeserializeOwned + Configurable,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

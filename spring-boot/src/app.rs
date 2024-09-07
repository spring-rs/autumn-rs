use crate::config::Configurable;
use crate::log::LogPlugin;
use crate::{config, plugin::Plugin};
use crate::{
    config::env,
    error::Result,
    plugin::{component::ComponentRef, PluginRef},
};
use anyhow::Context;
use dashmap::DashMap;
use std::any::Any;
use std::collections::HashMap;
use std::{
    any,
    collections::HashSet,
    future::Future,
    path::{Path, PathBuf},
    sync::Arc,
};
use toml::Table;

pub type Registry<T> = DashMap<String, T>;
pub type ReadonlyRegistry<T> = HashMap<String, T>;
pub type Scheduler = dyn FnOnce(Arc<App>) -> Box<dyn Future<Output = Result<String>> + Send>;

pub struct App {
    /// Component
    components: ReadonlyRegistry<ComponentRef>,
}

pub struct AppBuilder {
    pub(crate) tracing_registry: tracing_subscriber::Registry,
    /// Plugin
    pub(crate) plugin_registry: Registry<PluginRef>,
    /// Component
    components: Registry<ComponentRef>,
    /// Path of config file
    pub(crate) config_path: PathBuf,
    /// Configuration read from `config_path`
    config: Table,
    /// task
    schedulers: Vec<Box<Scheduler>>,
}

impl App {
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> AppBuilder {
        AppBuilder::default()
    }

    /// Get the component of the specified type
    pub fn get_component<T>(&self) -> Option<Arc<T>>
    where
        T: Any + Send + Sync,
    {
        let component_name = std::any::type_name::<T>();
        let component_ref = self.components.get(component_name)?;
        component_ref.clone().downcast::<T>()
    }

    pub fn get_components(&self) -> Vec<String> {
        self.components.iter().map(|(key, _)| key.clone()).collect()
    }
}

unsafe impl Send for AppBuilder {}
unsafe impl Sync for AppBuilder {}

impl AppBuilder {
    /// add plugin
    pub fn add_plugin<T: Plugin>(&mut self, plugin: T) -> &mut Self {
        log::debug!("added plugin: {}", plugin.name());
        let plugin_name = plugin.name().to_string();
        if self.plugin_registry.contains_key(plugin.name()) {
            panic!("Error adding plugin {plugin_name}: plugin was already added in application")
        }
        self.plugin_registry
            .insert(plugin_name, PluginRef::new(plugin));
        self
    }

    /// Returns `true` if the [`Plugin`] has already been added.
    pub fn is_plugin_added<T: Plugin>(&self) -> bool {
        self.plugin_registry.contains_key(any::type_name::<T>())
    }

    /// The path of the configuration file, default is `./config/app.toml`.
    /// The application automatically reads the environment configuration file
    /// in the same directory according to the `spring_ENV` environment variable,
    /// such as `./config/app-dev.toml`.
    /// The environment configuration file has a higher priority and will
    /// overwrite the configuration items of the main configuration file.
    ///
    /// For specific supported environments, see the [Env](./config/env) enum.
    pub fn config_file(&mut self, config_path: &str) -> &mut Self {
        self.config_path = Path::new(config_path).to_path_buf();
        self
    }

    /// Get the configuration items of the plugin according to the plugin's `config_prefix`
    pub fn get_config<T>(&self, plugin: &impl Configurable) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let prefix = plugin.config_prefix();
        let table = match self.config.get(prefix) {
            Some(toml::Value::Table(table)) => table.to_owned(),
            _ => Table::new(),
        };
        Ok(T::deserialize(table.to_owned()).with_context(|| {
            format!(
                "Failed to deserialize the configuration of prefix \"{}\"",
                prefix
            )
        })?)
    }

    /// Add component to the registry
    pub fn add_component<T>(&mut self, component: T) -> &mut Self
    where
        T: Clone + any::Any + Send + Sync,
    {
        let component_name = std::any::type_name::<T>();
        log::debug!("added component: {}", component_name);
        if self.components.contains_key(component_name) {
            panic!("Error adding component {component_name}: component was already added in application")
        }
        let component_name = component_name.to_string();
        self.components
            .insert(component_name, ComponentRef::new(component));
        self
    }

    /// Get the component of the specified type
    pub fn get_component<T>(&self) -> Option<Arc<T>>
    where
        T: Any + Send + Sync,
    {
        let component_name = std::any::type_name::<T>();
        let pair = self.components.get(component_name)?;
        let component_ref = pair.value().clone();
        component_ref.downcast::<T>()
    }

    /// Add a scheduled task
    pub fn add_scheduler<T>(&mut self, scheduler: T) -> &mut Self
    where
        T: FnOnce(Arc<App>) -> Box<dyn Future<Output = Result<String>> + Send> + 'static,
    {
        self.schedulers.push(Box::new(scheduler));
        self
    }

    /// Running
    pub async fn run(&mut self) {
        match self.inner_run().await {
            Err(e) => {
                log::error!("{:?}", e);
            }
            Ok(_app) => {}
        }
    }

    async fn inner_run(&mut self) -> Result<Arc<App>> {
        // 1. read env variable
        let env = env::init()?;

        // 2. load yaml config
        self.config = self.load_config(env)?;

        // 3. build plugin
        self.build_plugins().await;

        // 4. schedule
        self.schedule().await
    }

    fn load_config(&mut self, env: env::Env) -> Result<Table> {
        config::load_config(&self.config_path, env)
    }

    async fn build_plugins(&mut self) {
        LogPlugin.build(self);

        let registry = std::mem::take(&mut self.plugin_registry);
        let mut to_register = registry
            .iter()
            .map(|e| e.value().to_owned())
            .collect::<Vec<_>>();
        let mut registered: HashSet<String> = HashSet::new();

        while !to_register.is_empty() {
            let mut progress = false;
            let mut next_round = vec![];

            for plugin in to_register {
                let deps = plugin.dependencies();
                if deps.iter().all(|dep| registered.contains(*dep)) {
                    plugin.build(self).await;
                    registered.insert(plugin.name().to_string());
                    log::info!("{} plugin registered", plugin.name());
                    progress = true;
                } else {
                    next_round.push(plugin);
                }
            }

            if !progress {
                panic!("Cyclic dependency detected or missing dependencies for some plugins");
            }

            to_register = next_round;
        }
        self.plugin_registry = registry;
    }

    async fn schedule(&mut self) -> Result<Arc<App>> {
        let app = self.build_app();
        while let Some(task) = self.schedulers.pop() {
            let poll_future = task(app.clone());
            let poll_future = Box::into_pin(poll_future);
            match tokio::spawn(poll_future).await? {
                Err(e) => log::error!("{}", e),
                Ok(msg) => log::info!("scheduled result: {}", msg),
            }
        }
        Ok(app)
    }

    fn build_app(&mut self) -> Arc<App> {
        let dash_map = std::mem::take(&mut self.components);
        let mut hash_map = HashMap::with_capacity(dash_map.len());
        for entry in dash_map.iter() {
            hash_map.insert(entry.key().clone(), entry.value().clone());
        }
        Arc::new(App {
            components: hash_map,
        })
    }
}

impl Default for AppBuilder {
    fn default() -> Self {
        Self {
            tracing_registry: tracing_subscriber::registry(),
            plugin_registry: Default::default(),
            config_path: Path::new("./config/app.toml").to_path_buf(),
            config: Default::default(),
            components: Default::default(),
            schedulers: Default::default(),
        }
    }
}

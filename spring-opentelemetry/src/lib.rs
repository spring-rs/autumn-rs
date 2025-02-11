//! [![spring-rs](https://img.shields.io/github/stars/spring-rs/spring-rs)](https://spring-rs.github.io/docs/plugins/spring-opentelemetry)
#![doc(html_favicon_url = "https://spring-rs.github.io/favicon.ico")]
#![doc(html_logo_url = "https://spring-rs.github.io/logo.svg")]

pub mod middlewares;

#[rustfmt::skip]
pub use opentelemetry_otlp::{
    OTEL_EXPORTER_OTLP_COMPRESSION,
    OTEL_EXPORTER_OTLP_ENDPOINT,
    OTEL_EXPORTER_OTLP_HEADERS,
    OTEL_EXPORTER_OTLP_TIMEOUT,
    // logs
    OTEL_EXPORTER_OTLP_LOGS_COMPRESSION,
    OTEL_EXPORTER_OTLP_LOGS_ENDPOINT,
    OTEL_EXPORTER_OTLP_LOGS_HEADERS,
    OTEL_EXPORTER_OTLP_LOGS_TIMEOUT,
    // metrics
    OTEL_EXPORTER_OTLP_METRICS_COMPRESSION,
    OTEL_EXPORTER_OTLP_METRICS_ENDPOINT,
    OTEL_EXPORTER_OTLP_METRICS_HEADERS,
    OTEL_EXPORTER_OTLP_METRICS_TIMEOUT,
    // trace
    OTEL_EXPORTER_OTLP_TRACES_COMPRESSION,
    OTEL_EXPORTER_OTLP_TRACES_ENDPOINT,
    OTEL_EXPORTER_OTLP_TRACES_HEADERS,
    OTEL_EXPORTER_OTLP_TRACES_TIMEOUT,
};
pub use opentelemetry::{global, KeyValue};
pub use opentelemetry_sdk::Resource;
pub use opentelemetry_semantic_conventions::resource::*;

use anyhow::Context;
use opentelemetry::trace::TracerProvider;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_sdk::logs::LoggerProvider;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::{self as sdktrace, BatchConfig};
use opentelemetry_sdk::{resource, runtime};
use opentelemetry_semantic_conventions::attribute;
use spring::config::env::Env;
use spring::plugin::component::ComponentRef;
use spring::plugin::{ComponentRegistry, MutableComponentRegistry};
use spring::{app::AppBuilder, error::Result, plugin::Plugin};
use std::time::Duration;
use tracing_opentelemetry::{MetricsLayer, OpenTelemetryLayer};

pub struct OpenTelemetryPlugin;

impl Plugin for OpenTelemetryPlugin {
    fn immediately_build(&self, app: &mut AppBuilder) {
        let resource = Self::get_resource_attr(app.get_env());
        let resource = match app.get_component_ref::<Resource>() {
            Some(r) => resource.merge(r),
            None => resource,
        };
        let log_provider = Self::init_logs(resource.clone());
        let meter_provider = Self::init_metrics(resource.clone());
        let tracer = Self::init_tracer(resource);

        let log_layer = OpenTelemetryTracingBridge::new(&log_provider);
        let metric_layer = MetricsLayer::new(meter_provider.clone());
        let trace_layer = OpenTelemetryLayer::new(tracer);

        app.add_layer(trace_layer)
            .add_layer(log_layer)
            .add_layer(metric_layer)
            .add_shutdown_hook(move |_| Box::new(Self::shutdown(meter_provider, log_provider)));
    }

    fn immediately(&self) -> bool {
        true
    }
}

impl OpenTelemetryPlugin {
    fn init_logs(resource: Resource) -> LoggerProvider {
        opentelemetry_otlp::new_pipeline()
            .logging()
            .with_exporter(opentelemetry_otlp::new_exporter().tonic())
            .with_resource(resource)
            .install_batch(runtime::Tokio)
            .expect("build LogProvider failed")
    }

    fn init_metrics(resource: Resource) -> SdkMeterProvider {
        let provider = opentelemetry_otlp::new_pipeline()
            .metrics(runtime::Tokio)
            .with_exporter(opentelemetry_otlp::new_exporter().tonic())
            .with_resource(resource)
            .build()
            .expect("build MeterProvider failed");

        global::set_meter_provider(provider.clone());
        tracing::debug!("metrics provider installed");

        provider
    }

    fn init_tracer(resource: Resource) -> sdktrace::Tracer {
        global::set_text_map_propagator(TraceContextPropagator::new());
        #[cfg(feature = "jaeger")]
        global::set_text_map_propagator(opentelemetry_jaeger_propagator::Propagator::new());
        #[cfg(feature = "zipkin")]
        global::set_text_map_propagator(opentelemetry_zipkin::Propagator::new());

        let provider = opentelemetry_otlp::new_pipeline()
            .tracing()
            .with_exporter(opentelemetry_otlp::new_exporter().tonic())
            .with_trace_config(sdktrace::Config::default().with_resource(resource))
            .with_batch_config(BatchConfig::default())
            .install_batch(runtime::Tokio)
            .expect("build TraceProvider failed");

        let tracer = provider.tracer(env!("CARGO_PKG_NAME"));
        global::set_tracer_provider(provider);
        tracing::debug!("tracer provider installed");

        tracer
    }

    fn get_resource_attr(env: Env) -> Resource {
        Self::infra_resource().merge(&Self::app_resource(env))
    }

    fn app_resource(env: Env) -> Resource {
        Resource::from_schema_url(
            [KeyValue::new(
                attribute::DEPLOYMENT_ENVIRONMENT_NAME,
                format!("{:?}", env),
            )],
            opentelemetry_semantic_conventions::SCHEMA_URL,
        )
    }

    fn infra_resource() -> Resource {
        Resource::from_detectors(
            Duration::from_secs(0),
            vec![
                #[cfg(feature = "more-resource")]
                Box::new(opentelemetry_resource_detectors::HostResourceDetector::default()),
                #[cfg(feature = "more-resource")]
                Box::new(opentelemetry_resource_detectors::OsResourceDetector),
                #[cfg(feature = "more-resource")]
                Box::new(opentelemetry_resource_detectors::ProcessResourceDetector),
                Box::new(resource::SdkProvidedResourceDetector),
                Box::new(resource::TelemetryResourceDetector),
                Box::new(resource::EnvResourceDetector::new()),
            ],
        )
    }

    async fn shutdown(
        meter_provider: SdkMeterProvider,
        log_provider: LoggerProvider,
    ) -> Result<String> {
        global::shutdown_tracer_provider();
        meter_provider
            .shutdown()
            .context("shutdown meter provider failed")?;
        log_provider
            .shutdown()
            .context("shutdown log provider failed")?;
        Ok("OpenTelemetry shutdown successful".into())
    }
}

pub trait ResourceConfigurator {
    fn opentelemetry_attrs<KV>(&mut self, kvs: KV) -> &mut Self
    where
        KV: IntoIterator<Item = KeyValue>,
    {
        self.merge_resource(Resource::from_schema_url(
            kvs,
            opentelemetry_semantic_conventions::SCHEMA_URL,
        ))
    }

    fn merge_resource(&mut self, resource: Resource) -> &mut Self;
}

impl ResourceConfigurator for AppBuilder {
    fn merge_resource(&mut self, resource: Resource) -> &mut Self {
        if let Some(old_resource) = self.get_component_ref::<Resource>() {
            unsafe {
                let raw_ptr = ComponentRef::into_raw(old_resource) as *mut Resource;
                let old_resource = &mut *(raw_ptr);
                std::ptr::write(raw_ptr, old_resource.merge(&resource));
            }
            self
        } else {
            self.add_component(resource)
        }
    }
}

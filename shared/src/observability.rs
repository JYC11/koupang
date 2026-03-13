use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Initialize tracing with console output.
///
/// When the `telemetry` feature is enabled and `OTLP_ENDPOINT` is set,
/// an OTLP exporter layer is added alongside the console layer,
/// sending traces to the configured collector (e.g. Jaeger at `localhost:4317`).
pub fn init_tracing(service_name: &str) {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| format!("{service_name}=debug,tower_http=debug").into());
    let fmt_layer = tracing_subscriber::fmt::layer();

    #[cfg(feature = "telemetry")]
    {
        let otel_layer = init_otel_layer(service_name);
        let otel_enabled = otel_layer.is_some();
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .with(otel_layer)
            .init();
        if otel_enabled {
            tracing::info!("OTLP tracing enabled");
        }
    }

    #[cfg(not(feature = "telemetry"))]
    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .init();
}

#[cfg(feature = "telemetry")]
fn init_otel_layer<S>(
    service_name: &str,
) -> Option<tracing_opentelemetry::OpenTelemetryLayer<S, opentelemetry_sdk::trace::Tracer>>
where
    S: tracing::Subscriber + for<'span> tracing_subscriber::registry::LookupSpan<'span>,
{
    use opentelemetry::global;
    use opentelemetry::trace::TracerProvider;
    use opentelemetry_otlp::{SpanExporter, WithExportConfig};
    use opentelemetry_sdk::{Resource, trace::SdkTracerProvider};

    let endpoint = std::env::var("OTLP_ENDPOINT").ok()?;

    let exporter = SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&endpoint)
        .build()
        .map_err(|e| eprintln!("Failed to create OTLP span exporter: {e}"))
        .ok()?;

    let resource = Resource::builder()
        .with_service_name(service_name.to_owned())
        .build();

    let provider = SdkTracerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(exporter)
        .build();

    let tracer = provider.tracer(service_name.to_owned());
    global::set_tracer_provider(provider);

    Some(tracing_opentelemetry::layer().with_tracer(tracer))
}

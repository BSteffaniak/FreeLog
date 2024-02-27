#![cfg_attr(feature = "fail-on-warnings", deny(warnings))]

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::SystemTime,
};

use once_cell::sync::Lazy;
use reqwest::StatusCode;
use serde::{Serialize, Serializer};
use serde_json::Value;
use strum_macros::{AsRefStr, EnumString};
use thiserror::Error;
use tracing_log::{log_tracer, LogTracer};
use tracing_subscriber::{layer::SubscriberExt as _, Layer};

static CLIENT: Lazy<reqwest::Client> = Lazy::new(reqwest::Client::new);

static RT: Lazy<tokio::runtime::Runtime> = Lazy::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap()
});

fn extract_event_data(event: &tracing::Event) -> (Option<String>, FieldVisitor) {
    // Find message of the event, if any
    let mut visitor = FieldVisitor::default();
    event.record(&mut visitor);
    let message = visitor
        .json_values
        .remove("message")
        // When #[instrument(err)] is used the event does not have a message attached to it.
        // the error message is attached to the field "error".
        .or_else(|| visitor.json_values.remove("error"))
        .and_then(|v| match v {
            Value::String(s) => Some(s),
            _ => None,
        });

    (message, visitor)
}

#[derive(Default)]
pub(crate) struct FieldVisitor {
    pub json_values: BTreeMap<String, Value>,
}

impl FieldVisitor {
    fn record<T: Into<Value>>(&mut self, field: &tracing::field::Field, value: T) {
        self.json_values
            .insert(field.name().to_owned(), value.into());
    }
}

impl tracing::field::Visit for FieldVisitor {
    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.record(field, value);
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.record(field, value);
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.record(field, value);
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.record(field, value);
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.record(field, format!("{value:?}"));
    }
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    level: String,
    message: String,
    ts: usize,
}

impl ::serde::Serialize for LogEntry {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct ApiLogEntry<'a> {
            level: &'a str,
            values: &'a Vec<&'a str>,
            ts: usize,
        }

        let api = ApiLogEntry {
            level: &self.level.to_string().to_uppercase(),
            values: &vec![&self.message],
            ts: self.ts,
        };

        api.serialize(serializer)
    }
}

#[derive(Debug, Error)]
pub enum FlushError {
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
    #[error("Unsuccessful: {0}")]
    Unsuccessful(String),
}

#[derive(Clone)]
pub struct FreeLogLayer {
    buffer: Arc<Mutex<Vec<LogEntry>>>,
    config: LogsConfig,
}

impl FreeLogLayer {
    pub fn new(config: LogsConfig) -> Self {
        Self {
            buffer: Arc::new(Mutex::new(vec![])),
            config,
        }
    }

    pub async fn flush(&self) -> Result<(), FlushError> {
        let buffer: Vec<LogEntry> = self.buffer.lock().as_mut().unwrap().drain(..).collect();

        if buffer.is_empty() {
            return Ok(());
        }

        let body = serde_json::to_string(&buffer)?;

        println!("Sending request with {body}");

        let response = CLIENT
            .post(format!("{}/logs", self.config.log_writer_api_url))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(reqwest::header::USER_AGENT, &self.config.user_agent)
            .body(body)
            .send()
            .await?;

        if response.status() != StatusCode::OK {
            return Err(FlushError::Unsuccessful(response.text().await?));
        }

        let value: Value = response.json().await?;

        if !value
            .get("success")
            .and_then(|x| x.as_bool())
            .ok_or(FlushError::Unsuccessful(format!(
                "Received unsuccessful response: {value:?}"
            )))?
        {
            return Err(FlushError::Unsuccessful(format!(
                "Received unsuccessful response: {value:?}"
            )));
        }

        Ok(())
    }
}

fn level_int(level: &str) -> u8 {
    match level {
        "TRACE" => 0,
        "DEBUG" => 1,
        "INFO" => 2,
        "WARN" => 3,
        "ERROR" => 4,
        _ => 0,
    }
}

impl<S> Layer<S> for FreeLogLayer
where
    S: tracing::Subscriber + for<'span> tracing_subscriber::registry::LookupSpan<'span>,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let level = event.metadata().level();

        if level_int(level.as_str()) < level_int(self.config.log_level.as_ref()) {
            return;
        }

        let (message, _) = extract_event_data(event);

        if let Some(message) = message {
            self.buffer.lock().unwrap().push(LogEntry {
                level: level.to_string().to_uppercase(),
                message,
                ts: SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as usize,
            });
        }
    }
}

#[derive(Debug, Error)]
pub enum LogsInitError {
    #[error(transparent)]
    SetLogger(#[from] log_tracer::SetLoggerError),
    #[error(transparent)]
    SetGlobalDefault(#[from] tracing::subscriber::SetGlobalDefaultError),
}

#[derive(Debug, Default, Clone, Copy, EnumString, AsRefStr)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum Level {
    Trace,
    Debug,
    #[default]
    Info,
    Warn,
    Error,
}

#[derive(Clone, Default)]
pub struct LogsConfig {
    pub user_agent: String,
    pub log_writer_api_url: String,
    pub log_level: Level,
    pub auto_flush: bool,
    pub auto_flush_on_close: bool,
}

impl LogsConfig {
    pub fn builder() -> LogsConfigBuilder {
        LogsConfigBuilder::default()
    }
}

#[derive(Debug, Error)]
pub enum BuildLogsConfigError {
    #[error("Missing required property: {0}")]
    MissingRequiredProperty(String),
}

#[derive(Clone, Default)]
pub struct LogsConfigBuilder {
    user_agent: Option<String>,
    log_writer_api_url: Option<String>,
    log_level: Option<Level>,
    auto_flush: Option<bool>,
    auto_flush_on_close: Option<bool>,
}

impl LogsConfigBuilder {
    pub fn user_agent(mut self, value: impl Into<String>) -> LogsConfigBuilder {
        self.user_agent = Some(value.into());
        self
    }

    pub fn log_writer_api_url(mut self, value: impl Into<String>) -> LogsConfigBuilder {
        self.log_writer_api_url = Some(value.into());
        self
    }

    pub fn log_level(mut self, value: impl Into<Level>) -> LogsConfigBuilder {
        self.log_level = Some(value.into());
        self
    }

    pub fn auto_flush(mut self, value: impl Into<bool>) -> LogsConfigBuilder {
        self.auto_flush = Some(value.into());
        self
    }

    pub fn auto_flush_on_close(mut self, value: impl Into<bool>) -> LogsConfigBuilder {
        self.auto_flush_on_close = Some(value.into());
        self
    }

    pub fn build(self) -> Result<LogsConfig, BuildLogsConfigError> {
        Ok(LogsConfig {
            user_agent: self.user_agent.unwrap_or("free_log_rust_client".into()),
            log_writer_api_url: self.log_writer_api_url.ok_or(
                BuildLogsConfigError::MissingRequiredProperty("log_writer_api_url".into()),
            )?,
            log_level: self.log_level.unwrap_or(Level::default()),
            auto_flush: self.auto_flush.unwrap_or(true),
            auto_flush_on_close: self.auto_flush_on_close.unwrap_or(true),
        })
    }
}

pub fn init(config: impl Into<LogsConfig>) -> Result<FreeLogLayer, LogsInitError> {
    LogTracer::init()?;

    let config = config.into();
    let auto_flush = config.auto_flush;

    let free_log_layer = FreeLogLayer::new(config);
    let layer_send = free_log_layer.clone();
    let layer_return = free_log_layer.clone();

    let subscriber = tracing_subscriber::registry()
        .with(free_log_layer)
        .with(tracing_subscriber::fmt::Layer::default().with_writer(std::io::stdout))
        .with(tracing_subscriber::EnvFilter::from_default_env());

    tracing::subscriber::set_global_default(subscriber)?;

    if auto_flush {
        RT.spawn(async move {
            log_monitor(&layer_send).await;
        });
    }

    Ok(layer_return)
}

async fn log_monitor(layer: &FreeLogLayer) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(1000));

    loop {
        if let Err(err) = layer.flush().await {
            eprintln!("Failed to flush: {err:?}");
        }
        interval.tick().await;
    }
}
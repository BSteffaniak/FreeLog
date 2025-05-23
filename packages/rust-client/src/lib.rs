#![cfg_attr(feature = "fail-on-warnings", deny(warnings))]

use std::{
    collections::{BTreeMap, HashMap},
    convert::Infallible,
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex},
    time::SystemTime,
};

use free_log_models::{LogComponent, LogEntryRequest, LogLevel};
use serde_json::Value;
use strum_macros::{AsRefStr, EnumString};
use thiserror::Error;
use tracing_log::{log_tracer, LogTracer};
use tracing_subscriber::{layer::SubscriberExt, Layer, Registry};

#[cfg(feature = "api")]
pub mod api;

struct EventData {
    message: Option<String>,
    error: Option<String>,
    file: Option<String>,
    line: Option<u64>,
    module_path: Option<String>,
    target: Option<String>,
}

fn extract_event_data(event: &tracing::Event) -> (EventData, FieldVisitor) {
    // Find message of the event, if any
    let mut visitor = FieldVisitor::default();
    event.record(&mut visitor);

    let message = visitor.json_values.remove("message").and_then(|v| match v {
        Value::String(s) => Some(s),
        _ => None,
    });
    // When #[instrument(err)] is used the event does not have a message attached to it.
    // the error message is attached to the field "error".
    let error = visitor.json_values.remove("error").and_then(|v| match v {
        Value::String(s) => Some(s),
        _ => None,
    });

    let file = visitor
        .json_values
        .remove("log.file")
        .and_then(|v| match v {
            Value::String(s) => Some(s),
            _ => None,
        });

    let line = visitor
        .json_values
        .remove("log.line")
        .and_then(|v| match v {
            Value::Number(s) => s.as_u64(),
            _ => None,
        });

    let module_path = visitor
        .json_values
        .remove("log.module_path")
        .and_then(|v| match v {
            Value::String(s) => Some(s),
            _ => None,
        });

    let target = visitor
        .json_values
        .remove("log.target")
        .and_then(|v| match v {
            Value::String(s) => Some(s),
            _ => None,
        });

    (
        EventData {
            message,
            error,
            file,
            line,
            module_path,
            target,
        },
        visitor,
    )
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

#[derive(Debug, Error)]
pub enum FlushError {
    #[error(transparent)]
    IO(#[from] std::io::Error),
    #[cfg(feature = "api")]
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
    #[error("Unsuccessful: {0}")]
    Unsuccessful(String),
    #[error("Multiple errors: {0:?}")]
    Multi(Vec<FlushError>),
}

#[derive(Debug, Clone)]
pub struct FreeLogLayer {
    buffer: Arc<Mutex<Vec<LogEntryRequest>>>,
    config: Arc<LogsConfig>,
    #[cfg(feature = "api")]
    file_writers: api::FileWriters,
    properties: Arc<Mutex<Option<HashMap<String, LogComponent>>>>,
}

impl FreeLogLayer {
    pub fn new(config: LogsConfig) -> Self {
        Self {
            buffer: Arc::new(Mutex::new(vec![])),
            config: Arc::new(config),
            #[cfg(feature = "api")]
            file_writers: Arc::new(tokio::sync::Mutex::new(None)),
            properties: Arc::new(Mutex::new(None)),
        }
    }

    pub fn with_properties(&self, properties: HashMap<String, LogComponent>) -> &Self {
        self.properties.lock().as_mut().unwrap().replace(properties);
        self
    }

    pub fn set_property(&self, name: &str, value: LogComponent) -> &Self {
        self.properties
            .lock()
            .as_mut()
            .unwrap()
            .get_or_insert(HashMap::new())
            .insert(name.to_string(), value);
        self
    }

    pub fn remove_property(&self, name: &str) -> &Self {
        self.properties
            .lock()
            .as_mut()
            .unwrap()
            .get_or_insert(HashMap::new())
            .remove(name);
        self
    }

    #[cfg(feature = "api")]
    pub async fn flush(&self) -> Result<(), FlushError> {
        let mut errs = vec![];

        if !self.config.file_writers.is_empty() {
            let mut writers = self.file_writers.lock().await;

            if writers.is_none() {
                let mut new_writers = vec![];

                #[cfg(feature = "api")]
                for file_config in self.config.file_writers.iter() {
                    match tokio::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .write(true)
                        .open(&file_config.path)
                        .await
                    {
                        Ok(file) => {
                            new_writers
                                .push((file_config.log_level, tokio::io::BufWriter::new(file)));
                        }
                        Err(err) => {
                            errs.push(err.into());
                        }
                    };
                }

                writers.replace(new_writers);
            }
        }

        let buffer: Vec<LogEntryRequest> = self.buffer.lock().as_mut().unwrap().drain(..).collect();

        if buffer.is_empty() {
            return Ok(());
        }

        for api_config in self.config.api_writers.iter() {
            let entries = buffer
                .iter()
                .filter(|r| level_int(r.level.into()) >= level_int(api_config.log_level))
                .collect::<Vec<_>>();

            if entries.is_empty() {
                continue;
            }

            let body = serde_json::to_string(&entries)?;

            let response = match api::CLIENT
                .post(format!("{}/logs", api_config.api_url))
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .header(reqwest::header::USER_AGENT, &self.config.user_agent)
                .body(body)
                .send()
                .await
            {
                Ok(response) => response,
                Err(err) => {
                    errs.push(err.into());
                    continue;
                }
            };

            if response.status() != reqwest::StatusCode::OK {
                errs.push(FlushError::Unsuccessful(
                    response
                        .text()
                        .await
                        .unwrap_or("(failed to get response text)".to_string()),
                ));
                continue;
            }

            let value: Value = match response.json().await {
                Ok(response) => response,
                Err(err) => {
                    errs.push(err.into());
                    continue;
                }
            };

            if !value
                .get("success")
                .and_then(|x| x.as_bool())
                .ok_or(FlushError::Unsuccessful(format!(
                    "Received unsuccessful response: {value:?}"
                )))?
            {
                errs.push(FlushError::Unsuccessful(format!(
                    "Received unsuccessful response: {value:?}"
                )));
                continue;
            }
        }

        use tokio::io::AsyncWriteExt as _;
        if let Some(writers) = self.file_writers.lock().await.as_mut() {
            for (level, writer) in writers.iter_mut() {
                for entry in buffer
                    .iter()
                    .filter(|r| level_int(r.level.into()) >= level_int(*level))
                {
                    let mut body = serde_json::to_string(entry)?;
                    body.push('\n');

                    if let Err(err) = writer.write_all(body.as_bytes()).await {
                        errs.push(err.into());
                        continue;
                    }
                }

                if let Err(err) = writer.flush().await {
                    errs.push(err.into());
                    continue;
                }
            }
        }

        match errs.len() {
            0 => Ok(()),
            1 => Err(errs.into_iter().next().unwrap()),
            _ => Err(FlushError::Multi(errs)),
        }
    }
}

fn level_int(level: Level) -> u8 {
    match level {
        Level::Trace => 0,
        Level::Debug => 1,
        Level::Info => 2,
        Level::Warn => 3,
        Level::Error => 4,
    }
}

impl From<tracing::Level> for Level {
    fn from(value: tracing::Level) -> Self {
        (&value).into()
    }
}

impl From<&tracing::Level> for Level {
    fn from(value: &tracing::Level) -> Self {
        match *value {
            tracing::Level::TRACE => Level::Trace,
            tracing::Level::DEBUG => Level::Debug,
            tracing::Level::INFO => Level::Info,
            tracing::Level::WARN => Level::Warn,
            tracing::Level::ERROR => Level::Error,
        }
    }
}

impl From<LogLevel> for Level {
    fn from(value: LogLevel) -> Self {
        (&value).into()
    }
}

impl From<&LogLevel> for Level {
    fn from(value: &LogLevel) -> Self {
        match *value {
            LogLevel::Trace => Level::Trace,
            LogLevel::Debug => Level::Debug,
            LogLevel::Info => Level::Info,
            LogLevel::Warn => Level::Warn,
            LogLevel::Error => Level::Error,
        }
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

        if level_int(level.into()) < level_int(self.config.log_level) {
            return;
        }

        let (event_data, _) = extract_event_data(event);

        let location = if let (Some(file), Some(line)) = (&event_data.file, event_data.line) {
            Some(format!("{file}:{line}"))
        } else {
            event_data.file
        };

        self.buffer.lock().unwrap().push(LogEntryRequest {
            level: LogLevel::from_str(level.as_str()).unwrap(),
            ts: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis() as usize,
            values: vec![LogComponent::String(
                event_data.message.or(event_data.error).unwrap_or_default(),
            )],
            target: event_data.target,
            module_path: event_data.module_path,
            location,
            properties: self.properties.lock().as_ref().unwrap().as_ref().cloned(),
        });
    }
}

#[derive(Debug, Error)]
pub enum LogsInitError {
    #[error(transparent)]
    BuildLogsConfig(#[from] BuildLogsConfigError),
    #[error(transparent)]
    EnvFilter(#[from] EnvFilterError),
    #[error(transparent)]
    SetLogger(#[from] log_tracer::SetLoggerError),
    #[error(transparent)]
    SetGlobalDefault(#[from] tracing::subscriber::SetGlobalDefaultError),
}

#[derive(Debug, Default, Clone, Copy, EnumString, AsRefStr)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum Level {
    #[default]
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

pub type DynLayer = Box<dyn Layer<Registry> + Send + Sync>;

#[derive(Default)]
pub struct LogsConfig {
    pub user_agent: String,
    #[cfg(feature = "api")]
    pub api_writers: Vec<ApiWriterConfig>,
    #[cfg(feature = "api")]
    pub file_writers: Vec<FileWriterConfig>,
    pub log_level: Level,
    #[cfg(feature = "api")]
    pub auto_flush: bool,
    pub auto_flush_on_close: bool,
    env_filter: Option<EnvFilter>,
    layers: Vec<DynLayer>,
}

impl std::fmt::Debug for LogsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut binding = f.debug_struct("LogsConfig");

        let dbg = binding
            .field("user_agent", &self.user_agent)
            .field("log_level", &self.log_level)
            .field("auto_flush_on_close", &self.auto_flush_on_close)
            .field("env_filter", &self.env_filter);

        #[cfg(feature = "api")]
        let dbg = dbg
            .field("api_writers", &self.api_writers)
            .field("file_writers", &self.file_writers)
            .field("auto_flush", &self.auto_flush);

        dbg.finish_non_exhaustive()
    }
}

impl LogsConfig {
    pub fn builder() -> LogsConfigBuilder {
        LogsConfigBuilder::default()
    }

    pub fn take_layers(mut self) -> (Self, Vec<DynLayer>) {
        let layers = self.layers;
        self.layers = vec![];
        (self, layers)
    }
}

#[derive(Debug, Error)]
pub enum BuildLogsConfigError {
    #[error("Missing required property: {0}")]
    MissingRequiredProperty(String),
}

#[derive(Debug, Clone)]
pub struct EnvFilter {
    directives: Option<String>,
    from_env: Option<String>,
    from_default_env: bool,
}

impl EnvFilter {
    pub fn new<S: AsRef<str>>(directives: S) -> Self {
        Self {
            directives: Some(directives.as_ref().to_string()),
            from_env: None,
            from_default_env: false,
        }
    }

    pub fn from_env<S: AsRef<str>>(env: S) -> Self {
        Self {
            directives: None,
            from_env: Some(env.as_ref().to_string()),
            from_default_env: false,
        }
    }

    pub fn from_default_env() -> Self {
        Self {
            directives: None,
            from_env: None,
            from_default_env: true,
        }
    }
}

#[derive(Debug, Error)]
pub enum EnvFilterError {
    #[error("Invalid configuration")]
    InvalidConfiguration,
    #[error(transparent)]
    Parse(#[from] tracing_subscriber::filter::ParseError),
}

impl<T> From<T> for EnvFilter
where
    T: AsRef<str>,
{
    fn from(value: T) -> Self {
        EnvFilter::new(value)
    }
}

impl TryInto<tracing_subscriber::EnvFilter> for EnvFilter {
    type Error = EnvFilterError;

    fn try_into(self) -> Result<tracing_subscriber::EnvFilter, Self::Error> {
        (&self).try_into()
    }
}

impl TryInto<tracing_subscriber::EnvFilter> for &EnvFilter {
    type Error = EnvFilterError;

    fn try_into(self) -> Result<tracing_subscriber::EnvFilter, Self::Error> {
        if let Some(env) = &self.from_env {
            let filter = tracing_subscriber::EnvFilter::from_env(env);

            Ok(if let Some(directives) = &self.directives {
                filter.add_directive(directives.parse()?)
            } else {
                filter
            })
        } else if self.from_default_env {
            let filter = tracing_subscriber::EnvFilter::from_default_env();

            Ok(if let Some(directives) = &self.directives {
                filter.add_directive(directives.parse()?)
            } else {
                filter
            })
        } else if let Some(directives) = &self.directives {
            Ok(tracing_subscriber::EnvFilter::new(directives))
        } else {
            Err(EnvFilterError::InvalidConfiguration)
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ApiWriterConfig {
    pub user_agent: String,
    pub api_url: String,
    pub log_level: Level,
}

impl ApiWriterConfig {
    pub fn builder() -> ApiWriterConfigBuilder {
        ApiWriterConfigBuilder::default()
    }
}

#[derive(Clone, Default)]
pub struct ApiWriterConfigBuilder {
    user_agent: Option<String>,
    api_url: Option<String>,
    log_level: Option<Level>,
}

impl ApiWriterConfigBuilder {
    pub fn user_agent(mut self, value: impl Into<String>) -> ApiWriterConfigBuilder {
        self.user_agent = Some(value.into());
        self
    }

    pub fn api_url(mut self, value: impl Into<String>) -> ApiWriterConfigBuilder {
        self.api_url.replace(value.into());
        self
    }

    pub fn log_level(mut self, value: impl Into<Level>) -> ApiWriterConfigBuilder {
        self.log_level = Some(value.into());
        self
    }

    pub fn build(self) -> Result<ApiWriterConfig, BuildApiWriterConfigError> {
        Ok(ApiWriterConfig {
            user_agent: self.user_agent.unwrap_or("free_log_rust_client".into()),
            api_url: self.api_url.ok_or_else(|| {
                BuildApiWriterConfigError::MissingRequiredProperty("api_url".to_string())
            })?,
            log_level: self.log_level.unwrap_or_default(),
        })
    }
}

#[derive(Debug, Error)]
pub enum BuildApiWriterConfigError {
    #[error("Missing required property: {0}")]
    MissingRequiredProperty(String),
}

impl TryFrom<ApiWriterConfigBuilder> for ApiWriterConfig {
    type Error = BuildApiWriterConfigError;

    fn try_from(value: ApiWriterConfigBuilder) -> Result<Self, Self::Error> {
        value.build()
    }
}

#[derive(Debug, Default, Clone)]
pub struct FileWriterConfig {
    pub path: PathBuf,
    pub log_level: Level,
}

impl FileWriterConfig {
    pub fn builder() -> FileWriterConfigBuilder {
        FileWriterConfigBuilder::default()
    }
}

#[derive(Clone, Default)]
pub struct FileWriterConfigBuilder {
    path: Option<PathBuf>,
    log_level: Option<Level>,
}

impl FileWriterConfigBuilder {
    pub fn file_path(mut self, value: impl Into<PathBuf>) -> FileWriterConfigBuilder {
        self.path.replace(value.into());
        self
    }

    pub fn log_level(mut self, value: impl Into<Level>) -> FileWriterConfigBuilder {
        self.log_level = Some(value.into());
        self
    }

    pub fn build(self) -> Result<FileWriterConfig, BuildFileWriterConfigError> {
        Ok(FileWriterConfig {
            path: self.path.ok_or_else(|| {
                BuildFileWriterConfigError::MissingRequiredProperty("path".to_string())
            })?,
            log_level: self.log_level.unwrap_or_default(),
        })
    }
}

#[derive(Debug, Error)]
pub enum BuildFileWriterConfigError {
    #[error("Missing required property: {0}")]
    MissingRequiredProperty(String),
}

impl TryFrom<FileWriterConfigBuilder> for FileWriterConfig {
    type Error = BuildFileWriterConfigError;

    fn try_from(value: FileWriterConfigBuilder) -> Result<Self, Self::Error> {
        value.build()
    }
}

#[derive(Default)]
pub struct LogsConfigBuilder {
    user_agent: Option<String>,
    api_writers: Vec<ApiWriterConfig>,
    file_writers: Vec<FileWriterConfig>,
    log_level: Option<Level>,
    auto_flush: Option<bool>,
    auto_flush_on_close: Option<bool>,
    env_filter: Option<EnvFilter>,
    layers: Vec<DynLayer>,
}

impl LogsConfigBuilder {
    pub fn user_agent(mut self, value: impl Into<String>) -> LogsConfigBuilder {
        self.user_agent = Some(value.into());
        self
    }

    pub fn with_api_writer<T: TryInto<ApiWriterConfig>>(
        mut self,
        value: T,
    ) -> Result<LogsConfigBuilder, T::Error> {
        self.api_writers.push(value.try_into()?);
        Ok(self)
    }

    pub fn with_file_writer<T: TryInto<FileWriterConfig>>(
        mut self,
        value: T,
    ) -> Result<LogsConfigBuilder, T::Error> {
        self.file_writers.push(value.try_into()?);
        Ok(self)
    }

    pub fn with_layer<T: Layer<Registry> + Send + Sync>(mut self, value: T) -> LogsConfigBuilder {
        self.layers.push(Box::new(value));
        self
    }

    pub fn with_layer_dyn(mut self, value: DynLayer) -> LogsConfigBuilder {
        self.layers.push(value);
        self
    }

    pub fn with_layers(mut self, values: Vec<DynLayer>) -> LogsConfigBuilder {
        self.layers.extend(values);
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

    pub fn env_filter(mut self, value: impl Into<EnvFilter>) -> LogsConfigBuilder {
        self.env_filter = Some(value.into());
        self
    }

    pub fn build(self) -> Result<LogsConfig, BuildLogsConfigError> {
        Ok(LogsConfig {
            user_agent: self.user_agent.unwrap_or("free_log_rust_client".into()),
            #[cfg(feature = "api")]
            api_writers: self.api_writers,
            #[cfg(feature = "api")]
            file_writers: self.file_writers,
            log_level: self.log_level.unwrap_or_default(),
            #[cfg(feature = "api")]
            auto_flush: self.auto_flush.unwrap_or(true),
            auto_flush_on_close: self.auto_flush_on_close.unwrap_or(true),
            env_filter: self.env_filter,
            layers: self.layers,
        })
    }
}

impl TryFrom<LogsConfigBuilder> for LogsConfig {
    type Error = BuildLogsConfigError;

    fn try_from(value: LogsConfigBuilder) -> Result<Self, Self::Error> {
        value.build()
    }
}

impl From<Infallible> for BuildLogsConfigError {
    fn from(_value: Infallible) -> Self {
        unreachable!()
    }
}

pub fn init<T, X>(config: T) -> Result<FreeLogLayer, LogsInitError>
where
    T: TryInto<LogsConfig, Error = X>,
    X: Into<LogsInitError>,
{
    LogTracer::init()?;

    let config: LogsConfig = config.try_into().map_err(|x| x.into())?;
    #[cfg(feature = "api")]
    let auto_flush = config.auto_flush;
    let env_filter = config.env_filter.clone();

    let (config, mut layers) = config.take_layers();

    let free_log_layer = FreeLogLayer::new(config);
    layers.push(free_log_layer.clone().boxed());
    layers.push(
        tracing_subscriber::fmt::Layer::default()
            .with_writer(std::io::stdout)
            .boxed(),
    );
    if let Some(env_filter) = env_filter {
        let env_filter: tracing_subscriber::EnvFilter = env_filter.try_into()?;
        layers.push(env_filter.boxed());
    } else {
        layers.push(tracing_subscriber::EnvFilter::from_default_env().boxed());
    }

    let registry = tracing_subscriber::registry();

    let subscriber: DynLayer = layers
        .into_iter()
        .reduce(|acc, layer| acc.and_then(layer).boxed())
        .expect("No layers to build a subscriber to");

    tracing::subscriber::set_global_default(registry.with(subscriber))?;

    #[cfg(feature = "api")]
    {
        let layer_send = free_log_layer.clone();

        if auto_flush {
            api::RT.spawn(async move {
                log_monitor(&layer_send).await?;
                Ok::<_, MonitorError>(())
            });
        }
    }

    Ok(free_log_layer)
}

#[derive(Debug, Error)]
pub enum MonitorError {
    #[error(transparent)]
    IO(#[from] std::io::Error),
}

#[cfg(feature = "api")]
async fn log_monitor(layer: &FreeLogLayer) -> Result<(), MonitorError> {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(1000));

    loop {
        if let Err(err) = layer.flush().await {
            eprintln!("Failed to flush: {err:?}");
        }
        interval.tick().await;
    }
}

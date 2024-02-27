#![cfg_attr(feature = "fail-on-warnings", deny(warnings))]

use std::fmt::Display;

use actix_web::error::{ErrorBadRequest, ErrorInternalServerError};
use aws_sdk_cloudwatchlogs::{
    operation::{put_log_events::PutLogEventsError, RequestId},
    types::InputLogEvent,
};
use serde::Deserialize;
use serde_json::Value;
use strum_macros::{AsRefStr, EnumString};
use thiserror::Error;

pub mod api;

#[derive(Debug, Deserialize, EnumString, AsRefStr)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

pub enum LogComponent {
    Integer(isize),
    UInteger(usize),
    Real(f64),
    String(String),
    Boolean(bool),
    Undefined,
    Null,
}

impl Display for LogComponent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogComponent::Integer(value) => f.write_fmt(format_args!("{value}")),
            LogComponent::UInteger(value) => f.write_fmt(format_args!("{value}")),
            LogComponent::Real(value) => f.write_fmt(format_args!("{value}")),
            LogComponent::String(value) => f.write_fmt(format_args!("{value}")),
            LogComponent::Boolean(value) => f.write_fmt(format_args!("{value}")),
            LogComponent::Undefined => f.write_str("undefined"),
            LogComponent::Null => f.write_str("null"),
        }
    }
}

impl std::fmt::Debug for LogComponent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(self, f)
    }
}

impl From<LogComponent> for String {
    fn from(value: LogComponent) -> Self {
        value.to_string()
    }
}

impl<'de> Deserialize<'de> for LogComponent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value: Value = Deserialize::deserialize(deserializer)?;

        if value.is_u64() {
            Ok(LogComponent::UInteger(value.as_u64().unwrap() as usize))
        } else if value.is_i64() {
            Ok(LogComponent::Integer(value.as_i64().unwrap() as isize))
        } else if value.is_f64() {
            Ok(LogComponent::Real(value.as_f64().unwrap()))
        } else if value.is_string() {
            Ok(LogComponent::String(value.as_str().unwrap().to_string()))
        } else if value.is_boolean() {
            Ok(LogComponent::Boolean(value.as_bool().unwrap()))
        } else if value.is_null() {
            Ok(LogComponent::Null)
        } else {
            Ok(LogComponent::Undefined)
        }
    }
}

#[derive(Deserialize)]
pub struct LogEntryFromRequest {
    level: LogLevel,
    values: Vec<LogComponent>,
    ts: usize,
}

pub struct LogEntry<'a> {
    level: LogLevel,
    values: Vec<LogComponent>,
    ts: usize,
    ip: &'a str,
    user_agent: &'a str,
}

#[derive(Debug, Error)]
pub enum CreateLogsError {
    #[error("Invalid payload")]
    InvalidPayload,
    #[error("MissingLogGroupConfiguration: {r#type:?}")]
    MissingLogGroupConfiguration { r#type: String },
    #[error("Failed to put logs")]
    PutLogs(
        #[from]
        aws_smithy_runtime_api::client::result::SdkError<
            PutLogEventsError,
            aws_smithy_runtime_api::client::orchestrator::HttpResponse,
        >,
    ),
}

impl From<CreateLogsError> for actix_web::Error {
    fn from(value: CreateLogsError) -> Self {
        match value {
            CreateLogsError::InvalidPayload => ErrorBadRequest("Invalid payload"),
            CreateLogsError::MissingLogGroupConfiguration { .. } => {
                ErrorInternalServerError(value.to_string())
            }
            CreateLogsError::PutLogs(e) => {
                log::error!("Error: {e:?}");
                ErrorInternalServerError(e)
            }
        }
    }
}

pub async fn create_logs<'a>(
    payload: Value,
    ip: &'a str,
    user_agent: &'a str,
) -> Result<(), CreateLogsError> {
    let entries: Vec<LogEntryFromRequest> = serde_json::from_value(payload).map_err(|e| {
        log::error!("Invalid payload: {e:?}");
        CreateLogsError::InvalidPayload
    })?;

    let entries = entries
        .into_iter()
        .map(|x| LogEntry {
            level: x.level,
            values: x.values,
            ts: x.ts,
            ip,
            user_agent,
        })
        .collect::<Vec<_>>();

    create_log_entries(entries).await
}

pub async fn create_log_entries(entries: Vec<LogEntry<'_>>) -> Result<(), CreateLogsError> {
    let log_group_name = std::env::var("LOG_GROUP_NAME").map_err(|_| {
        CreateLogsError::MissingLogGroupConfiguration {
            r#type: "LOG_GROUP_NAME".into(),
        }
    })?;
    let log_stream_name = std::env::var("LOG_STREAM_NAME").map_err(|_| {
        CreateLogsError::MissingLogGroupConfiguration {
            r#type: "LOG_STREAM_NAME".into(),
        }
    })?;

    let config = aws_config::load_from_env().await;
    let client = aws_sdk_cloudwatchlogs::Client::new(&config);

    let events = entries
        .iter()
        .map(|x| {
            InputLogEvent::builder()
                .timestamp(x.ts as i64)
                .message(format!(
                    "{}:\n\n\t\
                     {:?}\n\n\t\
                     ip={}\n\n\t\
                     user_agent={}",
                    x.level.as_ref(),
                    x.values,
                    x.ip,
                    x.user_agent,
                ))
                .build()
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| {
            log::error!("Error: {e:?}");
            CreateLogsError::InvalidPayload
        })?;

    log::debug!("Writing events ({}): {events:?}", events.len());

    let output = client
        .put_log_events()
        .log_group_name(log_group_name)
        .log_stream_name(log_stream_name)
        .set_log_events(Some(events))
        .send()
        .await?;

    log::debug!("Successful request {:?}", output.request_id());

    Ok(())
}

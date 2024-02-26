#![cfg_attr(feature = "fail-on-warnings", deny(warnings))]

use std::fmt::Display;

use actix_web::error::ErrorBadRequest;
use aws_sdk_cloudwatchlogs::{operation::RequestId, types::InputLogEvent};
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
pub struct LogEntry {
    level: LogLevel,
    values: Vec<LogComponent>,
    ts: usize,
}

#[derive(Debug, Error)]
pub enum CreateLogsError {
    #[error("Invalid payload")]
    InvalidPayload,
}

impl From<CreateLogsError> for actix_web::Error {
    fn from(value: CreateLogsError) -> Self {
        match value {
            CreateLogsError::InvalidPayload => ErrorBadRequest("Invalid payload"),
        }
    }
}

pub async fn create_logs(payload: Value) -> Result<(), CreateLogsError> {
    let entries: Vec<LogEntry> =
        serde_json::from_value(payload).map_err(|_e| CreateLogsError::InvalidPayload)?;

    let log_group_name =
        std::env::var("LOG_GROUP_NAME").map_err(|_| CreateLogsError::InvalidPayload)?;
    let log_stream_name =
        std::env::var("LOG_STREAM_NAME").map_err(|_| CreateLogsError::InvalidPayload)?;

    let config = aws_config::load_from_env().await;
    let client = aws_sdk_cloudwatchlogs::Client::new(&config);

    let events = entries
        .iter()
        .map(|x| {
            InputLogEvent::builder()
                .timestamp(x.ts as i64)
                .message(format!("{}: {:?}", x.level.as_ref(), x.values))
                .build()
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_e| CreateLogsError::InvalidPayload)?;

    let output = client
        .put_log_events()
        .log_group_name(log_group_name)
        .log_stream_name(log_stream_name)
        .set_log_events(Some(events))
        .send()
        .await
        .map_err(|e| {
            log::error!("Error: {e:?}");

            CreateLogsError::InvalidPayload
        })?;

    log::debug!("Successful request {:?}", output.request_id());

    Ok(())
}

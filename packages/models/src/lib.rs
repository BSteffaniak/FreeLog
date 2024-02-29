#![cfg_attr(feature = "fail-on-warnings", deny(warnings))]

use std::{collections::HashMap, fmt::Display};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use strum_macros::{AsRefStr, EnumString};

#[derive(Debug, Serialize, Deserialize, EnumString, AsRefStr)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Clone)]
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

impl From<isize> for LogComponent {
    fn from(value: isize) -> Self {
        LogComponent::Integer(value)
    }
}

impl From<usize> for LogComponent {
    fn from(value: usize) -> Self {
        LogComponent::UInteger(value)
    }
}

impl From<f64> for LogComponent {
    fn from(value: f64) -> Self {
        LogComponent::Real(value)
    }
}

impl From<bool> for LogComponent {
    fn from(value: bool) -> Self {
        LogComponent::Boolean(value)
    }
}

impl From<&str> for LogComponent {
    fn from(value: &str) -> Self {
        LogComponent::String(value.to_string())
    }
}

impl From<String> for LogComponent {
    fn from(value: String) -> Self {
        LogComponent::String(value)
    }
}

impl From<LogComponent> for String {
    fn from(value: LogComponent) -> Self {
        value.to_string()
    }
}

impl Serialize for LogComponent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            LogComponent::Integer(value) => serializer.serialize_i64(*value as i64),
            LogComponent::UInteger(value) => serializer.serialize_u64(*value as u64),
            LogComponent::Real(value) => serializer.serialize_f64(*value),
            LogComponent::String(value) => serializer.serialize_str(value),
            LogComponent::Boolean(value) => serializer.serialize_bool(*value),
            LogComponent::Undefined => serializer.serialize_none(),
            LogComponent::Null => serializer.serialize_none(),
        }
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

pub struct LogEntry<'a> {
    pub level: LogLevel,
    pub values: Vec<LogComponent>,
    pub ts: usize,
    pub ip: &'a str,
    pub user_agent: &'a str,
    pub properties: Option<HashMap<String, LogComponent>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntryRequest {
    pub level: LogLevel,
    pub values: Vec<LogComponent>,
    pub ts: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, LogComponent>>,
}

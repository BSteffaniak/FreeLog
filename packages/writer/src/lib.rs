#![cfg_attr(feature = "fail-on-warnings", deny(warnings))]

use actix_web::error::{ErrorBadRequest, ErrorInternalServerError};
use aws_sdk_cloudwatchlogs::{
    operation::{put_log_events::PutLogEventsError, RequestId},
    types::InputLogEvent,
};
use free_log_models::{LogEntry, LogEntryRequest};
use serde_json::Value;
use thiserror::Error;

pub mod api;

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
    let entries: Vec<LogEntryRequest> = serde_json::from_value(payload).map_err(|e| {
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
            properties: x.properties,
        })
        .collect::<Vec<_>>();

    create_log_entries(entries).await
}

pub async fn create_log_entries(entries: Vec<LogEntry<'_>>) -> Result<(), CreateLogsError> {
    let log_group_name = std::env::var("LogGroupName").map_err(|_| {
        CreateLogsError::MissingLogGroupConfiguration {
            r#type: "LogGroupName".into(),
        }
    })?;
    let log_stream_name = std::env::var("LogStreamName").map_err(|_| {
        CreateLogsError::MissingLogGroupConfiguration {
            r#type: "LogStreamName".into(),
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
                     user_agent={}\n\n\t\
                     properties={:?}",
                    x.level.as_ref(),
                    x.values,
                    x.ip,
                    x.user_agent,
                    x.properties,
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

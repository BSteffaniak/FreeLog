use actix_web::{
    web::{self, Json},
    HttpRequest, Result,
};
use lambda_web::actix_web::{self, get, post};
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LogsError {
    #[error(transparent)]
    BadRequest(#[from] actix_web::Error),
    #[error("Internal server error: {error:?}")]
    InternalServerError { error: String },
    #[error("Not Found Error: {error:?}")]
    NotFound { error: String },
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GetLogsQuery {}

#[get("/logs")]
pub async fn get_logs_endpoint(_query: web::Query<GetLogsQuery>) -> Result<Json<Value>> {
    Ok(Json(serde_json::json!({"success": true})))
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CreateLogsQuery {}

#[post("/logs")]
pub async fn create_logs_endpoint(
    _query: web::Query<CreateLogsQuery>,
    req: HttpRequest,
    payload: Json<Value>,
) -> Result<Json<Value>> {
    let ip = req
        .peer_addr()
        .map(|x| x.to_string())
        .unwrap_or("unknown".to_string());

    let user_agent = req
        .headers()
        .get(actix_web::http::header::USER_AGENT)
        .and_then(|x| x.to_str().ok().map(|x| x.to_string()))
        .unwrap_or("none".to_string());

    crate::create_logs(payload.clone(), &ip, &user_agent).await?;

    Ok(Json(serde_json::json!({"success": true})))
}

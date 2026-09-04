use axum::{
    Json,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use serde_json::{Value, json};
use utoipa::ToSchema;

use crate::shared::error::ApplicationError;

#[derive(Debug, Serialize, ToSchema)]
pub struct ApiResponse<T>
where
    T: Serialize,
{
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<ApiErrorBody>,
    pub meta: Option<Value>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ApiErrorBody {
    pub message: String,
}

pub fn success<T>(data: T) -> Json<ApiResponse<T>>
where
    T: Serialize,
{
    Json(ApiResponse {
        success: true,
        data: Some(data),
        error: None,
        meta: None,
    })
}

pub fn success_with_meta<T>(data: T, meta: Value) -> Json<ApiResponse<T>>
where
    T: Serialize,
{
    Json(ApiResponse {
        success: true,
        data: Some(data),
        error: None,
        meta: Some(meta),
    })
}

impl IntoResponse for ApplicationError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let message = sanitize_public_error(&self);
        let body = Json(ApiResponse::<Value> {
            success: false,
            data: None,
            error: Some(ApiErrorBody {
                message,
            }),
            meta: Some(json!({ "status": status.as_u16() })),
        });

        (status, body).into_response()
    }
}

fn sanitize_public_error(err: &ApplicationError) -> String {
    match err {
        ApplicationError::Database(_) | ApplicationError::Internal(_) => {
            "internal server error".to_owned()
        }
        ApplicationError::ExternalService(msg) => format!("external service error: {}", sanitize_message(msg)),
        ApplicationError::BadRequest(msg) => format!("bad request: {}", sanitize_message(msg)),
        ApplicationError::Conflict(msg) => format!("conflict: {}", sanitize_message(msg)),
        ApplicationError::NotFound(msg) => format!("not found: {}", sanitize_message(msg)),
        ApplicationError::ServiceUnavailable(msg) => format!("service unavailable: {}", sanitize_message(msg)),
        ApplicationError::Unauthorized => "unauthorized".to_owned(),
        ApplicationError::Forbidden => "forbidden".to_owned(),
    }
}

fn sanitize_message(input: &str) -> String {
    let mut out = input.to_owned();
    if let Some(start) = out.find("://") {
        if let Some(at_pos) = out[start + 3..].find('@') {
            let creds = &out[start + 3..start + 3 + at_pos];
            if let Some(colon) = creds.find(':') {
                let user = &creds[..colon];
                let prefix = &out[..start + 3];
                let suffix = &out[start + 3 + at_pos..];
                out = format!("{prefix}{user}:***{suffix}");
            }
        }
    }
    out
}

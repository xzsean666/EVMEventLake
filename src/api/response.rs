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
        let body = Json(ApiResponse::<Value> {
            success: false,
            data: None,
            error: Some(ApiErrorBody {
                message: self.public_message(),
            }),
            meta: Some(json!({ "status": status.as_u16() })),
        });

        (status, body).into_response()
    }
}

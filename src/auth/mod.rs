use axum::{
    Json, Router,
    extract::{FromRequestParts, State},
    http::{HeaderMap, request::Parts},
    routing::post,
};
use chrono::{Duration, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use utoipa::ToSchema;

use crate::{
    api::response::{self, ApiResponse},
    app::application_state::ApplicationState,
    shared::error::ApplicationError,
};

pub fn routes() -> Router<ApplicationState> {
    Router::new().route("/api/auth/api-keys", post(create_api_key))
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Admin,
    ReadOnly,
}

#[derive(Debug, Clone)]
pub struct AuthenticatedPrincipal {
    pub subject: String,
    pub role: Role,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateApiKeyRequest {
    pub name: String,
    pub role: Role,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CreateApiKeyResponse {
    pub id: uuid::Uuid,
    pub name: String,
    pub role: Role,
    pub api_key: String,
}

#[derive(Debug, Deserialize)]
struct JwtClaims {
    sub: String,
    role: String,
    exp: i64,
}

#[derive(Debug, FromRow)]
struct ApiKeyRecord {
    id: uuid::Uuid,
    role: String,
}

impl Role {
    pub fn from_database(value: &str) -> Result<Self, ApplicationError> {
        match value {
            "admin" | "Admin" => Ok(Self::Admin),
            "read_only" | "readonly" | "ReadOnly" => Ok(Self::ReadOnly),
            other => Err(ApplicationError::Internal(format!(
                "unknown role stored in database: {other}"
            ))),
        }
    }

    pub fn can_write(&self) -> bool {
        matches!(self, Self::Admin)
    }

    fn as_database_value(&self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::ReadOnly => "read_only",
        }
    }
}

impl AuthenticatedPrincipal {
    pub fn require_admin(&self) -> Result<(), ApplicationError> {
        if self.role.can_write() {
            Ok(())
        } else {
            Err(ApplicationError::Forbidden)
        }
    }
}

impl FromRequestParts<ApplicationState> for AuthenticatedPrincipal {
    type Rejection = ApplicationError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &ApplicationState,
    ) -> Result<Self, Self::Rejection> {
        if !state.configuration.auth.require_authentication {
            return Ok(Self {
                subject: "local-development".to_owned(),
                role: Role::Admin,
            });
        }

        if let Some(api_key) = read_api_key(&parts.headers) {
            return authenticate_api_key(state, api_key).await;
        }

        if let Some(token) = read_bearer_token(&parts.headers) {
            return authenticate_jwt(state, token);
        }

        Err(ApplicationError::Unauthorized)
    }
}

async fn authenticate_api_key(
    state: &ApplicationState,
    api_key: &str,
) -> Result<AuthenticatedPrincipal, ApplicationError> {
    let key_hash = hash_api_key(api_key);
    let record = sqlx::query_as::<_, ApiKeyRecord>(
        r#"
        UPDATE eventlake_api_keys
        SET last_used_at = now()
        WHERE key_hash = $1 AND revoked = false
        RETURNING id, role
        "#,
    )
    .bind(key_hash)
    .fetch_optional(&state.pool)
    .await?;

    let record = record.ok_or(ApplicationError::Unauthorized)?;

    Ok(AuthenticatedPrincipal {
        subject: record.id.to_string(),
        role: Role::from_database(&record.role)?,
    })
}

fn authenticate_jwt(
    state: &ApplicationState,
    token: &str,
) -> Result<AuthenticatedPrincipal, ApplicationError> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.leeway = 30;

    let token_data = decode::<JwtClaims>(
        token,
        &DecodingKey::from_secret(state.configuration.auth.jwt_secret.as_bytes()),
        &validation,
    )
    .map_err(|_| ApplicationError::Unauthorized)?;

    if token_data.claims.exp < Utc::now().timestamp() - Duration::seconds(30).num_seconds() {
        return Err(ApplicationError::Unauthorized);
    }

    Ok(AuthenticatedPrincipal {
        subject: token_data.claims.sub,
        role: Role::from_database(&token_data.claims.role)?,
    })
}

fn read_api_key(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
}

fn read_bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    value.strip_prefix("Bearer ")
}

pub fn hash_api_key(api_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(api_key.as_bytes());
    format!("{:x}", hasher.finalize())
}

async fn create_api_key(
    principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Json(request): Json<CreateApiKeyRequest>,
) -> Result<Json<ApiResponse<CreateApiKeyResponse>>, ApplicationError> {
    principal.require_admin()?;

    let api_key = format!("evl_{}", uuid::Uuid::new_v4().simple());
    let key_hash = hash_api_key(&api_key);
    let id = uuid::Uuid::new_v4();

    sqlx::query(
        r#"
        INSERT INTO eventlake_api_keys (id, name, key_hash, role)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(id)
    .bind(&request.name)
    .bind(&key_hash)
    .bind(request.role.as_database_value())
    .execute(&state.pool)
    .await?;

    Ok(response::success(CreateApiKeyResponse {
        id,
        name: request.name,
        role: request.role,
        api_key,
    }))
}

pub async fn require_admin(
    State(_state): State<ApplicationState>,
    principal: AuthenticatedPrincipal,
) -> Result<AuthenticatedPrincipal, ApplicationError> {
    principal.require_admin()?;
    Ok(principal)
}

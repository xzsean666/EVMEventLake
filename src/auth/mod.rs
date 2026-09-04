use axum::{
    Json, Router,
    extract::{FromRequestParts, State},
    http::{HeaderMap, request::Parts},
    routing::post,
};
use chrono::{DateTime, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use utoipa::{OpenApi, ToSchema};

use crate::{
    api::response::{self, ApiResponse},
    app::application_state::ApplicationState,
    shared::error::ApplicationError,
};

pub fn routes() -> Router<ApplicationState> {
    Router::new()
        .route(
            "/api/auth/api-keys",
            post(create_api_key).get(list_api_keys),
        )
        .route("/api/auth/api-keys/{id}/revoke", post(revoke_api_key))
}

#[derive(OpenApi)]
#[openapi(
    paths(create_api_key, list_api_keys, revoke_api_key),
    components(schemas(Role, CreateApiKeyRequest, CreateApiKeyResponse, ApiKeySummary))
)]
struct AuthApiDocumentation;

pub fn openapi() -> utoipa::openapi::OpenApi {
    AuthApiDocumentation::openapi()
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
    // Required so tokens without an expiry fail to deserialize; the actual expiry check
    // is performed by `Validation`.
    #[allow(dead_code)]
    exp: i64,
}

#[derive(Debug, FromRow)]
struct ApiKeyRecord {
    id: uuid::Uuid,
    role: String,
}

#[derive(Debug, Serialize, FromRow, ToSchema)]
pub struct ApiKeySummary {
    pub id: uuid::Uuid,
    pub name: String,
    pub role: String,
    pub revoked: bool,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
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
        SELECT id, role
        FROM eventlake_api_keys
        WHERE key_hash = $1 AND revoked = false
        "#,
    )
    .bind(key_hash)
    .fetch_optional(&state.pool)
    .await?;

    let record = record.ok_or(ApplicationError::Unauthorized)?;

    // Debounce last_used_at updates so high-throughput requests using the same key
    // don't serialize on a single PostgreSQL row lock.
    let should_update = {
        let now = std::time::Instant::now();
        let cache = state
            .api_key_last_used
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match cache.get(&record.id) {
            Some(last_update) => {
                now.duration_since(*last_update) >= std::time::Duration::from_secs(60)
            }
            None => true,
        }
    };

    if should_update {
        let mut cache = state
            .api_key_last_used
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cache.insert(record.id, std::time::Instant::now());
        let pool = state.pool.clone();
        let key_id = record.id;
        tokio::spawn(async move {
            let _ = sqlx::query("UPDATE eventlake_api_keys SET last_used_at = now() WHERE id = $1")
                .bind(key_id)
                .execute(&pool)
                .await;
        });
    }

    Ok(AuthenticatedPrincipal {
        subject: record.id.to_string(),
        role: Role::from_database(&record.role)?,
    })
}

fn authenticate_jwt(
    state: &ApplicationState,
    token: &str,
) -> Result<AuthenticatedPrincipal, ApplicationError> {
    // `Validation` already enforces `exp` (with the configured leeway), so no manual
    // expiry check is needed here.
    let mut validation = Validation::new(Algorithm::HS256);
    validation.leeway = 30;

    let token_data = decode::<JwtClaims>(
        token,
        &DecodingKey::from_secret(state.configuration.auth.jwt_secret.as_bytes()),
        &validation,
    )
    .map_err(|_| ApplicationError::Unauthorized)?;

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

#[utoipa::path(
    post,
    path = "/api/auth/api-keys",
    tag = "auth",
    request_body = CreateApiKeyRequest,
    responses(
        (status = 200, description = "API key created", body = ApiResponse<CreateApiKeyResponse>),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden")
    )
)]
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

#[utoipa::path(
    get,
    path = "/api/auth/api-keys",
    tag = "auth",
    responses(
        (status = 200, description = "API keys", body = ApiResponse<Vec<ApiKeySummary>>),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden")
    )
)]
async fn list_api_keys(
    principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
) -> Result<Json<ApiResponse<Vec<ApiKeySummary>>>, ApplicationError> {
    principal.require_admin()?;

    let records = sqlx::query_as::<_, ApiKeySummary>(
        r#"
        SELECT id, name, role, revoked, created_at, last_used_at
        FROM eventlake_api_keys
        ORDER BY created_at DESC
        "#,
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(response::success(records))
}

#[utoipa::path(
    post,
    path = "/api/auth/api-keys/{id}/revoke",
    tag = "auth",
    params(("id" = uuid::Uuid, Path, description = "API key id")),
    responses(
        (status = 200, description = "API key revoked", body = ApiResponse<ApiKeySummary>),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "API key not found")
    )
)]
async fn revoke_api_key(
    principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    axum::extract::Path(id): axum::extract::Path<uuid::Uuid>,
) -> Result<Json<ApiResponse<ApiKeySummary>>, ApplicationError> {
    principal.require_admin()?;

    let record = sqlx::query_as::<_, ApiKeySummary>(
        r#"
        UPDATE eventlake_api_keys
        SET revoked = true
        WHERE id = $1
        RETURNING id, name, role, revoked, created_at, last_used_at
        "#,
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApplicationError::NotFound(format!("api key {id}")))?;

    Ok(response::success(record))
}

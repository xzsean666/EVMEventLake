use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct PageRequest {
    pub page: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PageMeta {
    pub page: i64,
    pub limit: i64,
}

impl PageRequest {
    pub fn normalized(&self) -> PageMeta {
        let page = self.page.unwrap_or(1).max(1);
        let limit = self.limit.unwrap_or(50).clamp(1, 500);
        PageMeta { page, limit }
    }

    pub fn offset(&self) -> i64 {
        let normalized = self.normalized();
        (normalized.page - 1) * normalized.limit
    }
}

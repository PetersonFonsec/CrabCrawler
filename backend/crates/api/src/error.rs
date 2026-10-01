use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

pub enum ApiError {
    NotFound,
    Internal,
}

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        tracing::error!(error = %err, "erro de banco");
        Self::Internal
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::NotFound => (StatusCode::NOT_FOUND, "não encontrado".to_string()),
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "erro interno".to_string(),
            ),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

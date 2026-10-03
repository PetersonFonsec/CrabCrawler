use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug)]
pub enum ApiError {
    NotFound,
    BadRequest(String),
    /// Dados recusados pela validação, campo a campo.
    Invalid {
        errors: serde_json::Value,
        warnings: serde_json::Value,
    },
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
        if let Self::Invalid { errors, warnings } = self {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({ "error": "dados inválidos", "errors": errors, "warnings": warnings })),
            )
                .into_response();
        }
        let (status, message) = match self {
            Self::NotFound => (StatusCode::NOT_FOUND, "não encontrado".to_string()),
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            Self::Invalid { .. } | Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "erro interno".to_string(),
            ),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

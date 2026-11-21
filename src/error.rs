use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// Erros da API. Mensagens de `Internal` nunca chegam ao cliente.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),
    #[error("credenciais inválidas")]
    InvalidCredentials,
    #[error("sessão ausente ou expirada")]
    Unauthorized,
    #[error("apenas o responsável pela família pode fazer isso")]
    Forbidden,
    #[error("recurso não encontrado")]
    NotFound,
    #[error("{0}")]
    Conflict(String),
    #[error("muitas tentativas, aguarde alguns minutos")]
    TooManyRequests,
    #[error("erro interno")]
    Internal(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl AppError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }

    fn parts(&self) -> (StatusCode, &'static str) {
        match self {
            Self::BadRequest(_) => (StatusCode::UNPROCESSABLE_ENTITY, "VALIDATION_ERROR"),
            Self::InvalidCredentials => (StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS"),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "UNAUTHORIZED"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "FORBIDDEN"),
            Self::NotFound => (StatusCode::NOT_FOUND, "NOT_FOUND"),
            Self::Conflict(_) => (StatusCode::CONFLICT, "CONFLICT"),
            Self::TooManyRequests => (StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED"),
            Self::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = self.parts();
        if let Self::Internal(err) = &self {
            tracing::error!(error = %err, "erro interno");
        }
        (
            status,
            Json(json!({ "error": code, "message": self.to_string() })),
        )
            .into_response()
    }
}

/// Traduz violações de constraint do Postgres em erros de cliente:
/// o banco é a última linha de defesa da integridade.
impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        if let sqlx::Error::RowNotFound = err {
            return Self::NotFound;
        }
        if let Some(db) = err.as_database_error() {
            match db.code().as_deref() {
                Some("23505") => return Self::Conflict("registro já existe".into()),
                Some("23503") => {
                    return Self::bad_request("referência inválida (conta, categoria ou membro)")
                }
                Some("23514") => return Self::bad_request("valor fora das regras permitidas"),
                Some("22P02") => return Self::bad_request("formato inválido"),
                _ => {}
            }
        }
        Self::Internal(Box::new(err))
    }
}

pub type AppResult<T> = Result<T, AppError>;

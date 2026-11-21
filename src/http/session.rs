use std::net::SocketAddr;

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;
use axum::http::HeaderMap;
use uuid::Uuid;

use super::AppState;
use crate::auth::token_hash;
use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Owner,
    Member,
}

/// Usuário autenticado e a família dele. Toda consulta de dados usa
/// `family_id` daqui, nunca um id vindo do cliente.
#[derive(Debug, Clone)]
pub struct Session {
    pub user_id: Uuid,
    pub family_id: Uuid,
    pub role: Role,
    pub token_hash: Vec<u8>,
}

impl Session {
    pub fn require_owner(&self) -> Result<(), AppError> {
        match self.role {
            Role::Owner => Ok(()),
            Role::Member => Err(AppError::Forbidden),
        }
    }
}

pub fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|t| !t.is_empty() && t.len() <= 100)
}

impl FromRequestParts<AppState> for Session {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = bearer_token(&parts.headers).ok_or(AppError::Unauthorized)?;
        let hash = token_hash(token);
        let row: Option<(Uuid, Uuid, String)> = sqlx::query_as(
            "SELECT s.user_id, m.family_id, m.role
               FROM sessions s
               JOIN memberships m ON m.user_id = s.user_id
              WHERE s.token_hash = $1 AND s.expires_at > now()",
        )
        .bind(&hash)
        .fetch_optional(&state.pool)
        .await?;

        let (user_id, family_id, role) = row.ok_or(AppError::Unauthorized)?;
        Ok(Session {
            user_id,
            family_id,
            role: if role == "OWNER" {
                Role::Owner
            } else {
                Role::Member
            },
            token_hash: hash,
        })
    }
}

/// IP do cliente como extractor (não falha se não houver ConnectInfo, ex.: testes).
pub struct ClientIp(pub String);

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(ClientIp(client_ip(
            &parts.headers,
            &parts.extensions,
            state.trust_proxy,
        )))
    }
}

/// IP do cliente para o rate limit. Só confia em X-Forwarded-For atrás de proxy configurado.
pub fn client_ip(
    parts_headers: &HeaderMap,
    extensions: &axum::http::Extensions,
    trust_proxy: bool,
) -> String {
    if trust_proxy {
        if let Some(ip) = parts_headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            return ip.to_string();
        }
    }
    extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip().to_string())
        .unwrap_or_else(|| "unknown".into())
}

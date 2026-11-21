use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::session::{ClientIp, Session};
use super::{require_text, AppState};
use crate::auth::{
    dummy_hash, hash_password, new_session_token, normalize_email, token_hash, validate_password,
    verify_password,
};
use crate::error::{AppError, AppResult};

const DEFAULT_EXPENSES: &[&str] = &[
    "Mercado",
    "Moradia",
    "Contas da casa",
    "Transporte",
    "Saúde",
    "Educação",
    "Restaurantes",
    "Lazer",
    "Assinaturas",
];
const DEFAULT_INCOME: &[&str] = &["Salário", "Outras entradas"];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegisterBody {
    family_name: String,
    name: String,
    email: String,
    password: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoginBody {
    email: String,
    password: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResponse {
    token: String,
    expires_at: DateTime<Utc>,
    user_id: Uuid,
    family_id: Uuid,
}

async fn hash_blocking(password: String) -> AppResult<String> {
    // Argon2 é CPU-bound de propósito: roda fora do executor async.
    tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(|e| AppError::Internal(Box::new(e)))?
        .map_err(|e| AppError::Internal(e.to_string().into()))
}

async fn create_session(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    state: &AppState,
    user_id: Uuid,
) -> AppResult<(String, DateTime<Utc>)> {
    let token = new_session_token();
    let expires_at = Utc::now() + state.session_ttl;
    sqlx::query("INSERT INTO sessions (token_hash, user_id, expires_at) VALUES ($1, $2, $3)")
        .bind(token_hash(&token))
        .bind(user_id)
        .bind(expires_at)
        .execute(&mut **tx)
        .await?;
    Ok((token, expires_at))
}

pub async fn register(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Json(body): Json<RegisterBody>,
) -> AppResult<(StatusCode, Json<SessionResponse>)> {
    if !state.auth_limiter.check(&ip) {
        return Err(AppError::TooManyRequests);
    }
    let family_name = require_text(&body.family_name, "familyName", 80)?;
    let name = require_text(&body.name, "name", 80)?;
    let email =
        normalize_email(&body.email).ok_or_else(|| AppError::bad_request("e-mail inválido"))?;
    validate_password(&body.password).map_err(AppError::bad_request)?;
    let password_hash = hash_blocking(body.password).await?;

    let mut tx = state.pool.begin().await?;
    let family_id: Uuid =
        sqlx::query_scalar("INSERT INTO families (name) VALUES ($1) RETURNING id")
            .bind(&family_name)
            .fetch_one(&mut *tx)
            .await?;
    let user_id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (email, name, password_hash) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(&email)
    .bind(&name)
    .bind(&password_hash)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match AppError::from(e) {
        AppError::Conflict(_) => AppError::Conflict("e-mail já cadastrado".into()),
        other => other,
    })?;
    sqlx::query("INSERT INTO memberships (family_id, user_id, role) VALUES ($1, $2, 'OWNER')")
        .bind(family_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

    // Categorias iniciais em um único INSERT com UNNEST.
    let names: Vec<&str> = DEFAULT_EXPENSES
        .iter()
        .chain(DEFAULT_INCOME)
        .copied()
        .collect();
    let kinds: Vec<&str> = DEFAULT_EXPENSES
        .iter()
        .map(|_| "EXPENSE")
        .chain(DEFAULT_INCOME.iter().map(|_| "INCOME"))
        .collect();
    sqlx::query("INSERT INTO categories (family_id, name, kind) SELECT $1, * FROM UNNEST($2::text[], $3::text[])")
        .bind(family_id)
        .bind(&names)
        .bind(&kinds)
        .execute(&mut *tx)
        .await?;

    let (token, expires_at) = create_session(&mut tx, &state, user_id).await?;
    tx.commit().await?;

    Ok((
        StatusCode::CREATED,
        Json(SessionResponse {
            token,
            expires_at,
            user_id,
            family_id,
        }),
    ))
}

pub async fn login(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Json(body): Json<LoginBody>,
) -> AppResult<Json<SessionResponse>> {
    if !state.auth_limiter.check(&ip) {
        return Err(AppError::TooManyRequests);
    }
    let email = normalize_email(&body.email).ok_or(AppError::InvalidCredentials)?;
    if body.password.len() > crate::auth::MAX_PASSWORD_LEN * 4 {
        return Err(AppError::InvalidCredentials);
    }

    let row: Option<(Uuid, String, Uuid)> = sqlx::query_as(
        "SELECT u.id, u.password_hash, m.family_id FROM users u JOIN memberships m ON m.user_id = u.id WHERE u.email = $1",
    )
    .bind(&email)
    .fetch_optional(&state.pool)
    .await?;

    // Verifica contra um hash fictício quando o e-mail não existe: mesmo custo de tempo.
    let stored = row
        .as_ref()
        .map(|r| r.1.clone())
        .unwrap_or_else(|| dummy_hash().to_string());
    let password = body.password;
    let ok = tokio::task::spawn_blocking(move || verify_password(&password, &stored))
        .await
        .map_err(|e| AppError::Internal(Box::new(e)))?;
    let (user_id, _, family_id) = match (row, ok) {
        (Some(r), true) => r,
        _ => return Err(AppError::InvalidCredentials),
    };

    let mut tx = state.pool.begin().await?;
    // Aproveita o login para limpar sessões vencidas do usuário.
    sqlx::query("DELETE FROM sessions WHERE user_id = $1 AND expires_at <= now()")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    let (token, expires_at) = create_session(&mut tx, &state, user_id).await?;
    tx.commit().await?;

    Ok(Json(SessionResponse {
        token,
        expires_at,
        user_id,
        family_id,
    }))
}

pub async fn logout(State(state): State<AppState>, session: Session) -> AppResult<StatusCode> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
        .bind(&session.token_hash)
        .execute(&state.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct MemberOut {
    id: Uuid,
    name: String,
    email: String,
    role: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeResponse {
    user_id: Uuid,
    family_id: Uuid,
    family_name: String,
    members: Vec<MemberOut>,
}

pub async fn me(State(state): State<AppState>, session: Session) -> AppResult<Json<MeResponse>> {
    let family_name: String = sqlx::query_scalar("SELECT name FROM families WHERE id = $1")
        .bind(session.family_id)
        .fetch_one(&state.pool)
        .await?;
    let members: Vec<MemberOut> = sqlx::query_as(
        "SELECT u.id, u.name, u.email, m.role FROM memberships m JOIN users u ON u.id = m.user_id
          WHERE m.family_id = $1 ORDER BY m.role DESC, u.name",
    )
    .bind(session.family_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(MeResponse {
        user_id: session.user_id,
        family_id: session.family_id,
        family_name,
        members,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddMemberBody {
    name: String,
    email: String,
    password: String,
}

/// O responsável cadastra um membro com senha inicial (ex.: cônjuge, filho).
pub async fn add_member(
    State(state): State<AppState>,
    session: Session,
    Json(body): Json<AddMemberBody>,
) -> AppResult<(StatusCode, Json<MemberOut>)> {
    session.require_owner()?;
    let name = require_text(&body.name, "name", 80)?;
    let email =
        normalize_email(&body.email).ok_or_else(|| AppError::bad_request("e-mail inválido"))?;
    validate_password(&body.password).map_err(AppError::bad_request)?;
    let password_hash = hash_blocking(body.password).await?;

    let mut tx = state.pool.begin().await?;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (email, name, password_hash) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(&email)
    .bind(&name)
    .bind(&password_hash)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match AppError::from(e) {
        AppError::Conflict(_) => AppError::Conflict("e-mail já cadastrado".into()),
        other => other,
    })?;
    sqlx::query("INSERT INTO memberships (family_id, user_id, role) VALUES ($1, $2, 'MEMBER')")
        .bind(session.family_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    Ok((
        StatusCode::CREATED,
        Json(MemberOut {
            id,
            name,
            email,
            role: "MEMBER".into(),
        }),
    ))
}

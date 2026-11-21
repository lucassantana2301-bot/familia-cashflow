mod finance;
mod identity;
mod imports;
pub mod session;

use std::sync::Arc;
use std::time::Duration;

use axum::extract::DefaultBodyLimit;
use axum::http::{header, HeaderValue, Method};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{Datelike, NaiveDate};
use serde_json::json;
use sqlx::PgPool;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

use crate::error::AppError;
use crate::rate_limit::RateLimiter;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub auth_limiter: Arc<RateLimiter>,
    pub session_ttl: Duration,
    pub trust_proxy: bool,
}

impl AppState {
    pub fn new(pool: PgPool, session_ttl: Duration, trust_proxy: bool) -> Self {
        Self {
            pool,
            // 10 tentativas de login/cadastro a cada 5 minutos por IP.
            auth_limiter: Arc::new(RateLimiter::new(10, Duration::from_secs(300))),
            session_ttl,
            trust_proxy,
        }
    }
}

const JSON_LIMIT: usize = 64 * 1024;
pub const IMPORT_LIMIT: usize = 5 * 1024 * 1024;

pub fn router(state: AppState, cors_origins: &[String]) -> Router {
    let origins: Vec<HeaderValue> = cors_origins.iter().filter_map(|o| o.parse().ok()).collect();
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);

    let api = Router::new()
        .route("/auth/register", post(identity::register))
        .route("/auth/login", post(identity::login))
        .route("/auth/logout", post(identity::logout))
        .route("/me", get(identity::me))
        .route("/members", post(identity::add_member))
        .route(
            "/accounts",
            get(finance::list_accounts).post(finance::create_account),
        )
        .route(
            "/categories",
            get(finance::list_categories).post(finance::create_category),
        )
        .route(
            "/transactions",
            get(finance::list_transactions).post(finance::create_transaction),
        )
        .route("/transactions/{id}", delete(finance::delete_transaction))
        .route(
            "/rules",
            get(finance::list_rules).post(finance::create_rule),
        )
        .route("/rules/{id}", delete(finance::delete_rule))
        .route(
            "/budgets",
            get(finance::list_budgets).put(finance::upsert_budget),
        )
        .route(
            "/goals",
            get(finance::list_goals).post(finance::create_goal),
        )
        .route("/goals/{id}/contributions", post(finance::contribute_goal))
        .route("/reports/monthly", get(finance::monthly_report))
        .route(
            "/imports",
            post(imports::import_statement).layer(DefaultBodyLimit::max(IMPORT_LIMIT)),
        )
        .layer(DefaultBodyLimit::max(JSON_LIMIT));

    Router::new()
        .route("/health", get(|| async { Json(json!({ "status": "ok" })) }))
        .nest("/api", api)
        .with_state(state)
        .layer(cors)
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ))
        .layer(TraceLayer::new_for_http())
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
}

/// "2026-11" -> (2026-11-01, 2026-12-01): intervalo semiaberto do mês.
pub fn month_range(month: &str) -> Result<(NaiveDate, NaiveDate), AppError> {
    let first = NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d")
        .map_err(|_| AppError::bad_request("mês deve estar no formato YYYY-MM"))?;
    let next = if first.month() == 12 {
        NaiveDate::from_ymd_opt(first.year() + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(first.year(), first.month() + 1, 1)
    }
    .ok_or_else(|| AppError::bad_request("mês fora do intervalo"))?;
    Ok((first, next))
}

pub fn require_text(value: &str, field: &str, max: usize) -> Result<String, AppError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > max {
        return Err(AppError::bad_request(format!(
            "{field} deve ter entre 1 e {max} caracteres"
        )));
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_range_handles_december() {
        let (a, b) = month_range("2026-12").unwrap();
        assert_eq!(a.to_string(), "2026-12-01");
        assert_eq!(b.to_string(), "2027-01-01");
        assert!(month_range("2026-13").is_err());
        assert!(month_range("nov").is_err());
    }
}

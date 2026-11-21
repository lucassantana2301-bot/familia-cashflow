use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::session::Session;
use super::{month_range, require_text, AppState};
use crate::error::{AppError, AppResult};
use crate::report::{build_monthly_report, CategoryRow, MonthlyReport, TxRow};

const MAX_AMOUNT_CENTS: i64 = 100_000_000_000; // R$ 1 bilhão: teto de sanidade

// ------------------------------------------------------------------ contas

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AccountOut {
    id: Uuid,
    name: String,
    kind: String,
    balance_cents: i64,
}

pub async fn list_accounts(
    State(state): State<AppState>,
    session: Session,
) -> AppResult<Json<Vec<AccountOut>>> {
    let rows = sqlx::query_as(
        "SELECT a.id, a.name, a.kind, COALESCE(SUM(t.amount_cents), 0)::BIGINT AS balance_cents
           FROM accounts a LEFT JOIN transactions t ON t.account_id = a.id
          WHERE a.family_id = $1
          GROUP BY a.id ORDER BY a.name",
    )
    .bind(session.family_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewAccount {
    name: String,
    kind: String,
}

pub async fn create_account(
    State(state): State<AppState>,
    session: Session,
    Json(body): Json<NewAccount>,
) -> AppResult<(StatusCode, Json<AccountOut>)> {
    let name = require_text(&body.name, "name", 60)?;
    if !["CHECKING", "SAVINGS", "CREDIT_CARD", "CASH"].contains(&body.kind.as_str()) {
        return Err(AppError::bad_request(
            "kind deve ser CHECKING, SAVINGS, CREDIT_CARD ou CASH",
        ));
    }
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO accounts (family_id, name, kind) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(session.family_id)
    .bind(&name)
    .bind(&body.kind)
    .fetch_one(&state.pool)
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(AccountOut {
            id,
            name,
            kind: body.kind,
            balance_cents: 0,
        }),
    ))
}

// ------------------------------------------------------------------ categorias

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct CategoryOut {
    id: Uuid,
    name: String,
    kind: String,
}

pub async fn list_categories(
    State(state): State<AppState>,
    session: Session,
) -> AppResult<Json<Vec<CategoryOut>>> {
    let rows = sqlx::query_as(
        "SELECT id, name, kind FROM categories WHERE family_id = $1 ORDER BY kind DESC, name",
    )
    .bind(session.family_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewCategory {
    name: String,
    kind: String,
}

pub async fn create_category(
    State(state): State<AppState>,
    session: Session,
    Json(body): Json<NewCategory>,
) -> AppResult<(StatusCode, Json<CategoryOut>)> {
    let name = require_text(&body.name, "name", 40)?;
    if body.kind != "EXPENSE" && body.kind != "INCOME" {
        return Err(AppError::bad_request("kind deve ser EXPENSE ou INCOME"));
    }
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO categories (family_id, name, kind) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(session.family_id)
    .bind(&name)
    .bind(&body.kind)
    .fetch_one(&state.pool)
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(CategoryOut {
            id,
            name,
            kind: body.kind,
        }),
    ))
}

// ------------------------------------------------------------------ lançamentos

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TransactionOut {
    id: Uuid,
    account_id: Uuid,
    category_id: Option<Uuid>,
    member_id: Option<Uuid>,
    amount_cents: i64,
    description: String,
    occurred_on: NaiveDate,
}

#[derive(Deserialize)]
pub struct MonthQuery {
    month: String,
}

pub async fn list_transactions(
    State(state): State<AppState>,
    session: Session,
    Query(q): Query<MonthQuery>,
) -> AppResult<Json<Vec<TransactionOut>>> {
    let (from, to) = month_range(&q.month)?;
    let rows = sqlx::query_as(
        "SELECT id, account_id, category_id, member_id, amount_cents, description, occurred_on
           FROM transactions
          WHERE family_id = $1 AND occurred_on >= $2 AND occurred_on < $3
          ORDER BY occurred_on DESC, created_at DESC
          LIMIT 2000",
    )
    .bind(session.family_id)
    .bind(from)
    .bind(to)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewTransaction {
    account_id: Uuid,
    category_id: Option<Uuid>,
    /// Centavos com sinal: negativo é gasto, positivo é entrada.
    amount_cents: i64,
    description: String,
    occurred_on: NaiveDate,
    /// Por padrão, quem lançou é quem gastou.
    member_id: Option<Uuid>,
}

pub async fn create_transaction(
    State(state): State<AppState>,
    session: Session,
    Json(body): Json<NewTransaction>,
) -> AppResult<(StatusCode, Json<TransactionOut>)> {
    if body.amount_cents == 0 || body.amount_cents.abs() > MAX_AMOUNT_CENTS {
        return Err(AppError::bad_request(
            "amountCents deve ser diferente de zero e menor que R$ 1 bilhão",
        ));
    }
    let description = require_text(&body.description, "description", 200)?;
    let member_id = body.member_id.unwrap_or(session.user_id);

    // O membro precisa ser da mesma família (FK composta cobre conta e categoria).
    let member_ok: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM memberships WHERE family_id = $1 AND user_id = $2)",
    )
    .bind(session.family_id)
    .bind(member_id)
    .fetch_one(&state.pool)
    .await?;
    if !member_ok {
        return Err(AppError::bad_request("membro não pertence à família"));
    }

    let row = sqlx::query_as(
        "INSERT INTO transactions (family_id, account_id, category_id, member_id, amount_cents, description, occurred_on)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id, account_id, category_id, member_id, amount_cents, description, occurred_on",
    )
    .bind(session.family_id)
    .bind(body.account_id)
    .bind(body.category_id)
    .bind(member_id)
    .bind(body.amount_cents)
    .bind(&description)
    .bind(body.occurred_on)
    .fetch_one(&state.pool)
    .await?;
    Ok((StatusCode::CREATED, Json(row)))
}

pub async fn delete_transaction(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    let result = sqlx::query("DELETE FROM transactions WHERE id = $1 AND family_id = $2")
        .bind(id)
        .bind(session.family_id)
        .execute(&state.pool)
        .await?;
    // Id de outra família responde 404, sem revelar que existe.
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

// ------------------------------------------------------------------ regras

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RuleOut {
    id: Uuid,
    pattern: String,
    category_id: Uuid,
    priority: i32,
}

pub async fn list_rules(
    State(state): State<AppState>,
    session: Session,
) -> AppResult<Json<Vec<RuleOut>>> {
    let rows = sqlx::query_as(
        "SELECT id, pattern, category_id, priority FROM rules WHERE family_id = $1 ORDER BY priority DESC, pattern",
    )
    .bind(session.family_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewRule {
    pattern: String,
    category_id: Uuid,
    #[serde(default)]
    priority: i32,
}

pub async fn create_rule(
    State(state): State<AppState>,
    session: Session,
    Json(body): Json<NewRule>,
) -> AppResult<(StatusCode, Json<RuleOut>)> {
    let pattern = require_text(&body.pattern, "pattern", 60)?;
    if pattern.chars().count() < 2 || !(-100..=100).contains(&body.priority) {
        return Err(AppError::bad_request(
            "pattern precisa de 2+ caracteres e priority entre -100 e 100",
        ));
    }
    let row = sqlx::query_as(
        "INSERT INTO rules (family_id, pattern, category_id, priority) VALUES ($1, $2, $3, $4)
         RETURNING id, pattern, category_id, priority",
    )
    .bind(session.family_id)
    .bind(&pattern)
    .bind(body.category_id)
    .bind(body.priority)
    .fetch_one(&state.pool)
    .await?;
    Ok((StatusCode::CREATED, Json(row)))
}

pub async fn delete_rule(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    let result = sqlx::query("DELETE FROM rules WHERE id = $1 AND family_id = $2")
        .bind(id)
        .bind(session.family_id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

// ------------------------------------------------------------------ orçamentos

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct BudgetOut {
    category_id: Uuid,
    limit_cents: i64,
}

pub async fn list_budgets(
    State(state): State<AppState>,
    session: Session,
    Query(q): Query<MonthQuery>,
) -> AppResult<Json<Vec<BudgetOut>>> {
    let (month, _) = month_range(&q.month)?;
    let rows = sqlx::query_as(
        "SELECT category_id, limit_cents FROM budgets WHERE family_id = $1 AND month = $2",
    )
    .bind(session.family_id)
    .bind(month)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BudgetBody {
    category_id: Uuid,
    month: String,
    limit_cents: i64,
}

/// Define o envelope da categoria no mês (cria ou substitui).
pub async fn upsert_budget(
    State(state): State<AppState>,
    session: Session,
    Json(body): Json<BudgetBody>,
) -> AppResult<Json<BudgetOut>> {
    let (month, _) = month_range(&body.month)?;
    if body.limit_cents <= 0 || body.limit_cents > MAX_AMOUNT_CENTS {
        return Err(AppError::bad_request("limitCents deve ser positivo"));
    }
    let row = sqlx::query_as(
        "INSERT INTO budgets (family_id, category_id, month, limit_cents) VALUES ($1, $2, $3, $4)
         ON CONFLICT (family_id, category_id, month) DO UPDATE SET limit_cents = EXCLUDED.limit_cents
         RETURNING category_id, limit_cents",
    )
    .bind(session.family_id)
    .bind(body.category_id)
    .bind(month)
    .bind(body.limit_cents)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row))
}

// ------------------------------------------------------------------ metas

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct GoalOut {
    id: Uuid,
    name: String,
    target_cents: i64,
    saved_cents: i64,
    target_date: Option<NaiveDate>,
}

pub async fn list_goals(
    State(state): State<AppState>,
    session: Session,
) -> AppResult<Json<Vec<GoalOut>>> {
    let rows = sqlx::query_as(
        "SELECT id, name, target_cents, saved_cents, target_date FROM goals WHERE family_id = $1
          ORDER BY target_date NULLS LAST, name",
    )
    .bind(session.family_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewGoal {
    name: String,
    target_cents: i64,
    target_date: Option<NaiveDate>,
}

pub async fn create_goal(
    State(state): State<AppState>,
    session: Session,
    Json(body): Json<NewGoal>,
) -> AppResult<(StatusCode, Json<GoalOut>)> {
    let name = require_text(&body.name, "name", 60)?;
    if body.target_cents <= 0 || body.target_cents > MAX_AMOUNT_CENTS {
        return Err(AppError::bad_request("targetCents deve ser positivo"));
    }
    let row = sqlx::query_as(
        "INSERT INTO goals (family_id, name, target_cents, target_date) VALUES ($1, $2, $3, $4)
         RETURNING id, name, target_cents, saved_cents, target_date",
    )
    .bind(session.family_id)
    .bind(&name)
    .bind(body.target_cents)
    .bind(body.target_date)
    .fetch_one(&state.pool)
    .await?;
    Ok((StatusCode::CREATED, Json(row)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Contribution {
    /// Positivo guarda; negativo resgata (nunca abaixo de zero).
    amount_cents: i64,
}

pub async fn contribute_goal(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<Uuid>,
    Json(body): Json<Contribution>,
) -> AppResult<Json<GoalOut>> {
    if body.amount_cents == 0 || body.amount_cents.abs() > MAX_AMOUNT_CENTS {
        return Err(AppError::bad_request("amountCents inválido"));
    }
    // Atualização atômica no banco: sem leitura-e-escrita que perderia aportes simultâneos.
    let row = sqlx::query_as(
        "UPDATE goals SET saved_cents = saved_cents + $3
          WHERE id = $1 AND family_id = $2
          RETURNING id, name, target_cents, saved_cents, target_date",
    )
    .bind(id)
    .bind(session.family_id)
    .bind(body.amount_cents)
    .fetch_optional(&state.pool)
    .await?;
    row.map(Json).ok_or(AppError::NotFound)
}

// ------------------------------------------------------------------ relatório

pub async fn monthly_report(
    State(state): State<AppState>,
    session: Session,
    Query(q): Query<MonthQuery>,
) -> AppResult<Json<MonthlyReport>> {
    let (from, to) = month_range(&q.month)?;
    let family = session.family_id;

    let txs: Vec<(i64, Option<Uuid>, Option<Uuid>, String)> = sqlx::query_as(
        "SELECT amount_cents, category_id, member_id, description FROM transactions
          WHERE family_id = $1 AND occurred_on >= $2 AND occurred_on < $3",
    )
    .bind(family)
    .bind(from)
    .bind(to)
    .fetch_all(&state.pool)
    .await?;
    let categories: Vec<(Uuid, String, String)> =
        sqlx::query_as("SELECT id, name, kind FROM categories WHERE family_id = $1")
            .bind(family)
            .fetch_all(&state.pool)
            .await?;
    let budgets: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT category_id, limit_cents FROM budgets WHERE family_id = $1 AND month = $2",
    )
    .bind(family)
    .bind(from)
    .fetch_all(&state.pool)
    .await?;
    let members: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT u.id, u.name FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.family_id = $1")
            .bind(family)
            .fetch_all(&state.pool)
            .await?;

    let txs: Vec<TxRow> = txs
        .into_iter()
        .map(
            |(amount_cents, category_id, member_id, description)| TxRow {
                amount_cents,
                category_id,
                member_id,
                description,
            },
        )
        .collect();
    let categories: Vec<CategoryRow> = categories
        .into_iter()
        .map(|(id, name, kind)| CategoryRow {
            id,
            name,
            is_expense: kind == "EXPENSE",
        })
        .collect();
    let budgets: HashMap<Uuid, i64> = budgets.into_iter().collect();
    let members: HashMap<Uuid, String> = members.into_iter().collect();

    Ok(Json(build_monthly_report(
        &q.month,
        &txs,
        &categories,
        &budgets,
        &members,
    )))
}

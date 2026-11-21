use axum::extract::{Multipart, State};
use axum::Json;
use serde::Serialize;
use uuid::Uuid;

use super::session::Session;
use super::{AppState, IMPORT_LIMIT};
use crate::categorize::{Categorizer, Rule};
use crate::error::{AppError, AppResult};
use crate::importer::parse_statement;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    read: usize,
    imported: u64,
    duplicates: u64,
    categorized: usize,
}

/// Recebe `multipart/form-data` com `accountId` e `file` (.ofx ou .csv).
pub async fn import_statement(
    State(state): State<AppState>,
    session: Session,
    mut multipart: Multipart,
) -> AppResult<Json<ImportResult>> {
    let mut account_id: Option<Uuid> = None;
    let mut file: Option<(String, Vec<u8>)> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::bad_request(format!("multipart inválido: {e}")))?
    {
        match field.name() {
            Some("accountId") => {
                let text = field
                    .text()
                    .await
                    .map_err(|_| AppError::bad_request("accountId inválido"))?;
                account_id = Some(
                    text.trim()
                        .parse()
                        .map_err(|_| AppError::bad_request("accountId inválido"))?,
                );
            }
            Some("file") => {
                let name = field.file_name().unwrap_or("extrato").to_string();
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|_| AppError::bad_request("arquivo acima de 5 MB"))?;
                if bytes.len() > IMPORT_LIMIT {
                    return Err(AppError::bad_request("arquivo acima de 5 MB"));
                }
                file = Some((name, bytes.to_vec()));
            }
            _ => {}
        }
    }
    let account_id = account_id.ok_or_else(|| AppError::bad_request("accountId é obrigatório"))?;
    let (file_name, bytes) = file.ok_or_else(|| AppError::bad_request("file é obrigatório"))?;

    let account_ok: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM accounts WHERE id = $1 AND family_id = $2)",
    )
    .bind(account_id)
    .bind(session.family_id)
    .fetch_one(&state.pool)
    .await?;
    if !account_ok {
        return Err(AppError::NotFound);
    }

    // Parsing é CPU-bound: fora do executor async.
    let parsed = tokio::task::spawn_blocking(move || parse_statement(&file_name, &bytes))
        .await
        .map_err(|e| AppError::Internal(Box::new(e)))?
        .map_err(|e| AppError::bad_request(e.to_string()))?;

    let rules: Vec<(String, Uuid, i32)> =
        sqlx::query_as("SELECT pattern, category_id, priority FROM rules WHERE family_id = $1")
            .bind(session.family_id)
            .fetch_all(&state.pool)
            .await?;
    let categorizer = Categorizer::new(
        rules
            .into_iter()
            .map(|(pattern, category_id, priority)| Rule {
                pattern,
                category_id,
                priority,
            })
            .collect(),
    );

    let mut dates = Vec::with_capacity(parsed.len());
    let mut amounts = Vec::with_capacity(parsed.len());
    let mut descriptions = Vec::with_capacity(parsed.len());
    let mut external_ids = Vec::with_capacity(parsed.len());
    let mut categories: Vec<Option<Uuid>> = Vec::with_capacity(parsed.len());
    for t in &parsed {
        dates.push(t.occurred_on);
        amounts.push(t.amount_cents);
        descriptions.push(t.description.clone());
        external_ids.push(t.external_id.clone());
        categories.push(categorizer.categorize(&t.description));
    }
    let categorized = categories.iter().filter(|c| c.is_some()).count();

    // Inserção em lote com UNNEST; ON CONFLICT ignora o que já foi importado antes.
    let result = sqlx::query(
        "INSERT INTO transactions (family_id, account_id, member_id, occurred_on, amount_cents, description, external_id, category_id)
         SELECT $1, $2, $3, * FROM UNNEST($4::date[], $5::bigint[], $6::text[], $7::text[], $8::uuid[])
         ON CONFLICT (account_id, external_id) DO NOTHING",
    )
    .bind(session.family_id)
    .bind(account_id)
    .bind(session.user_id)
    .bind(&dates)
    .bind(&amounts)
    .bind(&descriptions)
    .bind(&external_ids)
    .bind(&categories)
    .execute(&state.pool)
    .await?;

    let imported = result.rows_affected();
    Ok(Json(ImportResult {
        read: parsed.len(),
        imported,
        duplicates: parsed.len() as u64 - imported,
        categorized,
    }))
}

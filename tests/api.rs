//! Testes de integração da API contra um Postgres real.
//! Rodam só com `TEST_DATABASE_URL` definido (no CI, um container de serviço).

use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use familia_cashflow::http::{router, AppState};
use familia_cashflow::MIGRATOR;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use tokio::sync::OnceCell;
use tower::ServiceExt;
use uuid::Uuid;

static MIGRATED: OnceCell<()> = OnceCell::const_new();

async fn app() -> Option<Router> {
    let url = std::env::var("TEST_DATABASE_URL").ok()?;
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .expect("conectar no Postgres de teste");
    MIGRATED
        .get_or_init(|| async { MIGRATOR.run(&pool).await.expect("migrations") })
        .await;
    let state = AppState::new(pool, Duration::from_secs(3600), false);
    Some(router(state, &["http://localhost:5173".to_string()]))
}

macro_rules! require_db {
    () => {
        match app().await {
            Some(app) => app,
            None => {
                eprintln!("TEST_DATABASE_URL não definido: teste ignorado");
                return;
            }
        }
    };
}

async fn call(
    app: &Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let req = match body {
        Some(b) => req
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(b.to_string())),
        None => req.body(Body::empty()),
    }
    .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

async fn register(app: &Router, family: &str) -> (String, String) {
    let email = format!("{}@teste.dev", Uuid::new_v4());
    let (status, body) = call(
        app,
        Method::POST,
        "/api/auth/register",
        None,
        Some(json!({ "familyName": family, "name": "Lucas", "email": email, "password": "senha-forte-123" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    (body["token"].as_str().unwrap().to_string(), email)
}

async fn category_id(app: &Router, token: &str, name: &str) -> String {
    let (_, cats) = call(app, Method::GET, "/api/categories", Some(token), None).await;
    cats.as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn new_account(app: &Router, token: &str) -> String {
    let (status, acc) = call(
        app,
        Method::POST,
        "/api/accounts",
        Some(token),
        Some(json!({ "name": "Nubank", "kind": "CHECKING" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    acc["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn register_login_logout_flow() {
    let app = require_db!();
    let (token, email) = register(&app, "Família Santana").await;

    let (status, me) = call(&app, Method::GET, "/api/me", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["familyName"], "Família Santana");
    assert_eq!(me["members"][0]["role"], "OWNER");

    let (_, cats) = call(&app, Method::GET, "/api/categories", Some(&token), None).await;
    assert!(
        cats.as_array().unwrap().len() >= 10,
        "categorias padrão criadas"
    );

    let (status, _) = call(
        &app,
        Method::POST,
        "/api/auth/login",
        None,
        Some(json!({ "email": email, "password": "errada-errada" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = call(
        &app,
        Method::POST,
        "/api/auth/login",
        None,
        Some(json!({ "email": "ninguem@teste.dev", "password": "qualquer-coisa" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, login) = call(
        &app,
        Method::POST,
        "/api/auth/login",
        None,
        Some(json!({ "email": email.to_uppercase(), "password": "senha-forte-123" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let second = login["token"].as_str().unwrap();

    let (status, _) = call(&app, Method::POST, "/api/auth/logout", Some(second), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(&app, Method::GET, "/api/me", Some(second), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "token revogado no logout");
    let (status, _) = call(&app, Method::GET, "/api/me", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "outra sessão continua válida");
}

#[tokio::test]
async fn validates_registration_input() {
    let app = require_db!();
    let (status, body) = call(
        &app,
        Method::POST,
        "/api/auth/register",
        None,
        Some(json!({ "familyName": "X", "name": "Y", "email": "sem-arroba", "password": "senha-forte-123" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (status, _) = call(
        &app,
        Method::POST,
        "/api/auth/register",
        None,
        Some(json!({ "familyName": "X", "name": "Y", "email": "a@b.dev", "password": "curta" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = call(&app, Method::GET, "/api/accounts", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn families_are_isolated() {
    let app = require_db!();
    let (alice, _) = register(&app, "Família A").await;
    let (bob, _) = register(&app, "Família B").await;
    let alice_account = new_account(&app, &alice).await;
    let bob_category = category_id(&app, &bob, "Mercado").await;

    let (status, tx) = call(
        &app,
        Method::POST,
        "/api/transactions",
        Some(&alice),
        Some(json!({ "accountId": alice_account, "amountCents": -5000, "description": "Padaria", "occurredOn": "2026-11-03" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{tx}");
    let tx_id = tx["id"].as_str().unwrap();

    // Bob não vê nem apaga o lançamento da Alice.
    let (_, bob_list) = call(
        &app,
        Method::GET,
        "/api/transactions?month=2026-11",
        Some(&bob),
        None,
    )
    .await;
    assert!(bob_list.as_array().unwrap().is_empty());
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/api/transactions/{tx_id}"),
        Some(&bob),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Bob não consegue lançar na conta da Alice: a FK composta barra no banco.
    let (status, _) = call(
        &app,
        Method::POST,
        "/api/transactions",
        Some(&bob),
        Some(json!({ "accountId": alice_account, "amountCents": -1, "description": "x", "occurredOn": "2026-11-03" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Alice não consegue usar categoria da família do Bob.
    let (status, _) = call(
        &app,
        Method::POST,
        "/api/transactions",
        Some(&alice),
        Some(json!({ "accountId": alice_account, "categoryId": bob_category, "amountCents": -1, "description": "x", "occurredOn": "2026-11-03" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

fn multipart(account_id: &str, file_name: &str, content: &str) -> (String, Vec<u8>) {
    let boundary = "----familiacashflowtest";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"accountId\"\r\n\r\n{account_id}\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\nContent-Type: text/csv\r\n\r\n{content}\r\n\
         --{boundary}--\r\n"
    );
    (
        format!("multipart/form-data; boundary={boundary}"),
        body.into_bytes(),
    )
}

async fn import(app: &Router, token: &str, account: &str, csv: &str) -> (StatusCode, Value) {
    let (content_type, body) = multipart(account, "extrato.csv", csv);
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/imports")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(body))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn imports_statement_with_rules_and_dedup_then_reports() {
    let app = require_db!();
    let (token, _) = register(&app, "Família Import").await;
    let account = new_account(&app, &token).await;
    let mercado = category_id(&app, &token, "Mercado").await;
    let lazer = category_id(&app, &token, "Lazer").await;
    let salario = category_id(&app, &token, "Salário").await;

    for (pattern, cat) in [
        ("extra", &mercado),
        ("cinema", &lazer),
        ("salario", &salario),
    ] {
        let (status, _) = call(
            &app,
            Method::POST,
            "/api/rules",
            Some(&token),
            Some(json!({ "pattern": pattern, "categoryId": cat })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let csv = "Data;Descrição;Valor\n02/11/2026;Mercado Extra;-850,00\n10/11/2026;Cinema Kinoplex;-250,00\n05/11/2026;SALÁRIO ACME;10.000,00\n12/11/2026;Tarifa bancária;-30,00\n";
    let (status, first) = import(&app, &token, &account, csv).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(
        first,
        json!({ "read": 4, "imported": 4, "duplicates": 0, "categorized": 3 })
    );

    let (_, again) = import(&app, &token, &account, csv).await;
    assert_eq!(again["imported"], 0);
    assert_eq!(again["duplicates"], 4);

    let (status, err) = import(&app, &token, &account, "02/11/2026;X;abc\n").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(err["message"].as_str().unwrap().contains("linha 1"));

    for (cat, limit) in [(&mercado, 100_000), (&lazer, 20_000)] {
        let (status, _) = call(
            &app,
            Method::PUT,
            "/api/budgets",
            Some(&token),
            Some(json!({ "categoryId": cat, "month": "2026-11", "limitCents": limit })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    let (status, report) = call(
        &app,
        Method::GET,
        "/api/reports/monthly?month=2026-11",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(report["incomeCents"], 1_000_000);
    assert_eq!(report["expenseCents"], 113_000);
    assert_eq!(report["savingsRateBp"], 8_870);
    assert_eq!(report["envelopes"][0]["name"], "Lazer");
    assert_eq!(report["envelopes"][0]["status"], "OVER");
    assert_eq!(report["envelopes"][1]["status"], "WARNING");

    let (_, accounts) = call(&app, Method::GET, "/api/accounts", Some(&token), None).await;
    assert_eq!(accounts[0]["balanceCents"], 887_000);
}

#[tokio::test]
async fn goals_and_roles() {
    let app = require_db!();
    let (owner, _) = register(&app, "Família Metas").await;

    let (status, goal) = call(
        &app,
        Method::POST,
        "/api/goals",
        Some(&owner),
        Some(json!({ "name": "Viagem", "targetCents": 500_000, "targetDate": "2027-07-01" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let goal_url = format!("/api/goals/{}/contributions", goal["id"].as_str().unwrap());

    let (_, g) = call(
        &app,
        Method::POST,
        &goal_url,
        Some(&owner),
        Some(json!({ "amountCents": 30_000 })),
    )
    .await;
    assert_eq!(g["savedCents"], 30_000);
    let (status, _) = call(
        &app,
        Method::POST,
        &goal_url,
        Some(&owner),
        Some(json!({ "amountCents": -40_000 })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "resgate maior que o saldo é barrado pelo CHECK"
    );

    let member_email = format!("{}@teste.dev", Uuid::new_v4());
    let (status, _) = call(
        &app,
        Method::POST,
        "/api/members",
        Some(&owner),
        Some(json!({ "name": "Ana", "email": member_email, "password": "senha-da-ana-1" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, login) = call(
        &app,
        Method::POST,
        "/api/auth/login",
        None,
        Some(json!({ "email": member_email, "password": "senha-da-ana-1" })),
    )
    .await;
    let member = login["token"].as_str().unwrap();

    // Membro vê as metas da família, mas não cadastra outros membros.
    let (_, goals) = call(&app, Method::GET, "/api/goals", Some(member), None).await;
    assert_eq!(goals[0]["name"], "Viagem");
    let (status, _) = call(
        &app,
        Method::POST,
        "/api/members",
        Some(member),
        Some(json!({ "name": "X", "email": "x@teste.dev", "password": "senha-qualquer-1" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn rate_limits_login_attempts() {
    let app = require_db!();
    let mut last = StatusCode::OK;
    for _ in 0..11 {
        let (status, _) = call(
            &app,
            Method::POST,
            "/api/auth/login",
            None,
            Some(json!({ "email": "x@teste.dev", "password": "chute-errado-1" })),
        )
        .await;
        last = status;
    }
    assert_eq!(last, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn rejects_unknown_fields_and_security_headers_present() {
    let app = require_db!();
    let (token, _) = register(&app, "Família Headers").await;
    let (status, _) = call(
        &app,
        Method::POST,
        "/api/accounts",
        Some(&token),
        Some(json!({ "name": "C", "kind": "CASH", "familyId": Uuid::new_v4() })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "familyId do cliente é rejeitado"
    );

    let res = app
        .clone()
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.headers()["x-content-type-options"], "nosniff");
    assert!(res.headers().contains_key("x-request-id"));
}

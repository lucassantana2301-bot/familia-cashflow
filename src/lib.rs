//! familia-cashflow: finanças da família com orçamento por envelope,
//! metas e importação de extratos bancários brasileiros.

pub mod auth;
pub mod categorize;
pub mod config;
pub mod error;
pub mod http;
pub mod importer;
pub mod money;
pub mod rate_limit;
pub mod report;
pub mod text;

/// Migrations embutidas no binário em tempo de compilação.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

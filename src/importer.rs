//! Leitura de extratos bancários: OFX (1.x SGML e 2.x XML) e CSV.
//!
//! O resultado é uma lista de [`ParsedTransaction`] com `external_id` estável,
//! usado para não duplicar lançamentos ao reimportar o mesmo arquivo.

use std::collections::HashMap;

use chrono::NaiveDate;
use sha2::{Digest, Sha256};

use crate::money::parse_brl;
use crate::text::sanitize_description;

const MAX_DESCRIPTION: usize = 200;
/// Teto de linhas por arquivo: protege a API de extratos gigantes ou maliciosos.
pub const MAX_ROWS: usize = 5_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTransaction {
    pub occurred_on: NaiveDate,
    pub amount_cents: i64,
    pub description: String,
    pub external_id: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ImportError {
    #[error("formato de arquivo não reconhecido (use .ofx ou .csv)")]
    UnknownFormat,
    #[error("linha {line}: {reason}")]
    InvalidRow { line: usize, reason: String },
    #[error("arquivo sem lançamentos")]
    Empty,
    #[error("arquivo com mais de {MAX_ROWS} lançamentos")]
    TooManyRows,
}

pub fn parse_statement(
    file_name: &str,
    bytes: &[u8],
) -> Result<Vec<ParsedTransaction>, ImportError> {
    let text = decode(bytes);
    let lower = file_name.to_ascii_lowercase();
    let looks_ofx = lower.ends_with(".ofx") || text.contains("<OFX>") || text.contains("<STMTTRN>");
    let parsed = if looks_ofx {
        parse_ofx(&text)?
    } else if lower.ends_with(".csv") || lower.ends_with(".txt") {
        parse_csv(&text)?
    } else {
        return Err(ImportError::UnknownFormat);
    };
    if parsed.is_empty() {
        return Err(ImportError::Empty);
    }
    if parsed.len() > MAX_ROWS {
        return Err(ImportError::TooManyRows);
    }
    Ok(parsed)
}

/// Bancos brasileiros ainda exportam OFX/CSV em Latin-1/Windows-1252.
/// Se não for UTF-8 válido, decodifica byte a byte como Latin-1 (cobre os acentos do português).
fn decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

// ---------------------------------------------------------------- OFX

/// Valor de uma tag no estilo SGML (`<TAG>valor`) ou XML (`<TAG>valor</TAG>`).
fn tag_value<'a>(block: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let start = block.find(&open)? + open.len();
    let rest = &block[start..];
    let end = rest.find(['<', '\n', '\r']).unwrap_or(rest.len());
    let value = rest[..end].trim();
    (!value.is_empty()).then_some(value)
}

fn parse_ofx_date(raw: &str) -> Option<NaiveDate> {
    // DTPOSTED: YYYYMMDD[HHMMSS[.XXX]][[-3:BRT]] — só a data interessa.
    let digits: String = raw.chars().take(8).collect();
    NaiveDate::parse_from_str(&digits, "%Y%m%d").ok()
}

fn parse_ofx(text: &str) -> Result<Vec<ParsedTransaction>, ImportError> {
    let mut out = Vec::new();
    let mut fallback_ids: HashMap<String, usize> = HashMap::new();

    for (index, chunk) in text.split("<STMTTRN>").skip(1).enumerate() {
        if index >= MAX_ROWS {
            return Err(ImportError::TooManyRows);
        }
        let block = chunk.split("</STMTTRN>").next().unwrap_or(chunk);
        let line = index + 1;
        let invalid = |reason: &str| ImportError::InvalidRow {
            line,
            reason: reason.to_string(),
        };

        let occurred_on = tag_value(block, "DTPOSTED")
            .and_then(parse_ofx_date)
            .ok_or_else(|| invalid("DTPOSTED ausente ou inválido"))?;
        let amount_cents = tag_value(block, "TRNAMT")
            .ok_or_else(|| invalid("TRNAMT ausente"))
            .and_then(|v| parse_brl(v).map_err(|_| invalid("TRNAMT inválido")))?;
        if amount_cents == 0 {
            continue; // lançamentos zerados (estornos informativos) não entram
        }
        let raw_description = tag_value(block, "MEMO")
            .or_else(|| tag_value(block, "NAME"))
            .unwrap_or("Sem descrição");
        let description = sanitize_description(raw_description, MAX_DESCRIPTION);

        let external_id = match tag_value(block, "FITID") {
            Some(fitid) => format!("ofx:{}", fitid.chars().take(100).collect::<String>()),
            None => stable_id(&mut fallback_ids, occurred_on, amount_cents, &description),
        };
        out.push(ParsedTransaction {
            occurred_on,
            amount_cents,
            description,
            external_id,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------- CSV

fn parse_csv_date(raw: &str) -> Option<NaiveDate> {
    let raw = raw.trim();
    NaiveDate::parse_from_str(raw, "%d/%m/%Y")
        .or_else(|_| NaiveDate::parse_from_str(raw, "%Y-%m-%d"))
        .or_else(|_| NaiveDate::parse_from_str(raw, "%d/%m/%y"))
        .ok()
}

/// CSV com colunas `data, descrição, valor` (nessa ordem). Delimitador `;` ou `,`
/// detectado pela primeira linha; cabeçalho opcional.
fn parse_csv(text: &str) -> Result<Vec<ParsedTransaction>, ImportError> {
    let first_line = text.lines().next().unwrap_or_default();
    // Bancos brasileiros usam ";" porque a vírgula é o separador decimal.
    let delimiter = if first_line.contains(';') { b';' } else { b',' };

    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(text.as_bytes());

    let mut out = Vec::new();
    let mut ids: HashMap<String, usize> = HashMap::new();

    for (index, record) in reader.records().enumerate() {
        let line = index + 1;
        let record = record.map_err(|e| ImportError::InvalidRow {
            line,
            reason: e.to_string(),
        })?;
        if record.iter().all(str::is_empty) {
            continue;
        }
        if out.len() >= MAX_ROWS {
            return Err(ImportError::TooManyRows);
        }
        let field = |i: usize| record.get(i).unwrap_or_default();

        let Some(occurred_on) = parse_csv_date(field(0)) else {
            if line == 1 {
                continue; // cabeçalho
            }
            return Err(ImportError::InvalidRow {
                line,
                reason: format!("data inválida: {:?}", field(0)),
            });
        };
        let amount_cents = parse_brl(field(2)).map_err(|_| ImportError::InvalidRow {
            line,
            reason: format!("valor inválido: {:?}", field(2)),
        })?;
        if amount_cents == 0 {
            continue;
        }
        let description = sanitize_description(field(1), MAX_DESCRIPTION);
        let description = if description.is_empty() {
            "Sem descrição".to_string()
        } else {
            description
        };
        let external_id = stable_id(&mut ids, occurred_on, amount_cents, &description);
        out.push(ParsedTransaction {
            occurred_on,
            amount_cents,
            description,
            external_id,
        });
    }
    Ok(out)
}

/// Hash de (data, valor, descrição, ocorrência). A ocorrência diferencia dois
/// cafés iguais no mesmo dia e continua estável se o mesmo arquivo for reimportado.
fn stable_id(
    seen: &mut HashMap<String, usize>,
    date: NaiveDate,
    cents: i64,
    description: &str,
) -> String {
    let key = format!("{date}|{cents}|{description}");
    let occurrence = seen.entry(key.clone()).or_insert(0);
    *occurrence += 1;
    let digest = Sha256::digest(format!("{key}|{occurrence}").as_bytes());
    let hex: String = digest.iter().take(16).map(|b| format!("{b:02x}")).collect();
    format!("hash:{hex}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const OFX_SGML: &str = "OFXHEADER:100\nDATA:OFXSGML\nCHARSET:1252\n\n<OFX><BANKMSGSRSV1><STMTTRNRS><STMTRS><BANKTRANLIST>\n\
<STMTTRN>\n<TRNTYPE>DEBIT\n<DTPOSTED>20261103120000[-3:BRT]\n<TRNAMT>-45.90\n<FITID>202611030001\n<MEMO>PAG*Padaria Sao Joao\n</STMTTRN>\n\
<STMTTRN>\n<TRNTYPE>CREDIT\n<DTPOSTED>20261105\n<TRNAMT>8500.00\n<FITID>202611050002\n<NAME>SALARIO ACME LTDA\n</STMTTRN>\n\
</BANKTRANLIST></STMTRS></STMTTRNRS></BANKMSGSRSV1></OFX>";

    #[test]
    fn parses_ofx_sgml() {
        let txs = parse_statement("extrato.ofx", OFX_SGML.as_bytes()).unwrap();
        assert_eq!(txs.len(), 2);
        assert_eq!(
            txs[0].occurred_on,
            NaiveDate::from_ymd_opt(2026, 11, 3).unwrap()
        );
        assert_eq!(txs[0].amount_cents, -4_590);
        assert_eq!(txs[0].description, "PAG*Padaria Sao Joao");
        assert_eq!(txs[0].external_id, "ofx:202611030001");
        assert_eq!(txs[1].description, "SALARIO ACME LTDA");
    }

    #[test]
    fn parses_ofx_xml_with_closing_tags() {
        let xml = "<OFX><STMTTRN><DTPOSTED>20261110</DTPOSTED><TRNAMT>-12.00</TRNAMT><FITID>X1</FITID><MEMO>Uber</MEMO></STMTTRN></OFX>";
        let txs = parse_statement("a.ofx", xml.as_bytes()).unwrap();
        assert_eq!(txs[0].amount_cents, -1_200);
        assert_eq!(txs[0].description, "Uber");
    }

    #[test]
    fn decodes_latin1() {
        let mut bytes = b"<OFX><STMTTRN><DTPOSTED>20261110<TRNAMT>-1.00<MEMO>A".to_vec();
        bytes.push(0xC7); // Ç em Latin-1
        bytes.extend_from_slice(b"OUGUE</MEMO></STMTTRN>");
        let txs = parse_statement("a.ofx", &bytes).unwrap();
        assert_eq!(txs[0].description, "AÇOUGUE");
    }

    #[test]
    fn parses_brazilian_csv_with_header_and_stable_ids() {
        let csv = "Data;Descrição;Valor\n03/11/2026;Mercado Extra;-1.234,56\n03/11/2026;Café;-8,50\n03/11/2026;Café;-8,50\n05/11/2026;Salário;8.500,00\n";
        let txs = parse_statement("extrato.csv", csv.as_bytes()).unwrap();
        assert_eq!(txs.len(), 4);
        assert_eq!(txs[0].amount_cents, -123_456);
        // Dois cafés iguais no mesmo dia: ids diferentes, mas determinísticos.
        assert_ne!(txs[1].external_id, txs[2].external_id);
        let again = parse_statement("extrato.csv", csv.as_bytes()).unwrap();
        assert_eq!(txs, again);
    }

    #[test]
    fn parses_comma_csv_with_iso_dates() {
        let csv = "2026-11-03,Padaria,-45.90\n2026-11-04,\"Farmácia, centro\",-30.00\n";
        let txs = parse_statement("x.csv", csv.as_bytes()).unwrap();
        assert_eq!(txs[1].description, "Farmácia, centro");
        assert_eq!(txs[1].amount_cents, -3_000);
    }

    #[test]
    fn reports_line_of_invalid_row() {
        let csv = "03/11/2026;Mercado;-10,00\n31/02/2026;Erro;-1,00\n";
        assert_eq!(
            parse_statement("x.csv", csv.as_bytes()),
            Err(ImportError::InvalidRow {
                line: 2,
                reason: "data inválida: \"31/02/2026\"".into()
            })
        );
    }

    #[test]
    fn rejects_unknown_and_empty() {
        assert_eq!(
            parse_statement("foto.png", b"\x89PNG"),
            Err(ImportError::UnknownFormat)
        );
        assert_eq!(
            parse_statement("x.csv", b"Data;Desc;Valor\n"),
            Err(ImportError::Empty)
        );
    }
}

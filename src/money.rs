//! Dinheiro em centavos (`i64`). Nunca `f64`: 0,1 + 0,2 não fecha o mês.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("valor monetário inválido: {0:?}")]
pub struct ParseMoneyError(pub String);

/// Converte texto de extrato/formulário para centavos.
///
/// Regras de separador:
/// - com vírgula: padrão brasileiro, pontos são milhar (`"1.234,56"`, `"-45,9"`);
/// - sem vírgula: ponto é decimal, padrão do OFX (`"-45.90"`, `"1234.5"`).
///
/// Aceita prefixo `R$`, espaços e sinal. Rejeita mais de 2 casas decimais.
pub fn parse_brl(input: &str) -> Result<i64, ParseMoneyError> {
    let err = || ParseMoneyError(input.to_string());
    let compact: String = input
        .trim()
        .trim_start_matches("R$")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();

    // Um único sinal: "-", "+" ou o "(valor)" contábil.
    let (negative, s) = if let Some(rest) = compact.strip_prefix('-') {
        (true, rest.to_string())
    } else if let Some(rest) = compact.strip_prefix('+') {
        (false, rest.to_string())
    } else if compact.len() > 2 && compact.starts_with('(') && compact.ends_with(')') {
        (true, compact[1..compact.len() - 1].to_string())
    } else {
        (false, compact)
    };
    if s.is_empty() {
        return Err(err());
    }

    let (int_part, frac_part) = if s.contains(',') {
        let mut it = s.splitn(2, ',');
        let int = it.next().unwrap_or_default().replace('.', "");
        (int, it.next().unwrap_or_default().to_string())
    } else if let Some((int, frac)) = s.rsplit_once('.') {
        (int.to_string(), frac.to_string())
    } else {
        (s.clone(), String::new())
    };

    let int_valid = !int_part.is_empty() && int_part.chars().all(|c| c.is_ascii_digit());
    let frac_valid = frac_part.len() <= 2 && frac_part.chars().all(|c| c.is_ascii_digit());
    if !int_valid || !frac_valid {
        return Err(err());
    }

    let reais: i64 = int_part.parse().map_err(|_| err())?;
    let cents: i64 = match frac_part.len() {
        0 => 0,
        1 => frac_part.parse::<i64>().map_err(|_| err())? * 10,
        _ => frac_part.parse().map_err(|_| err())?,
    };
    let total = reais
        .checked_mul(100)
        .and_then(|v| v.checked_add(cents))
        .ok_or_else(err)?;
    Ok(if negative { -total } else { total })
}

/// Formata centavos como `R$ 1.234,56` (ou `-R$ 45,90`).
pub struct Brl(pub i64);

impl fmt::Display for Brl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let abs = self.0.unsigned_abs();
        let reais = (abs / 100).to_string();
        let mut grouped = String::new();
        for (i, ch) in reais.chars().enumerate() {
            if i > 0 && (reais.len() - i) % 3 == 0 {
                grouped.push('.');
            }
            grouped.push(ch);
        }
        let sign = if self.0 < 0 { "-" } else { "" };
        write!(f, "{sign}R$ {grouped},{:02}", abs % 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_brazilian_and_ofx_formats() {
        assert_eq!(parse_brl("1.234,56"), Ok(123_456));
        assert_eq!(parse_brl("R$ 10,00"), Ok(1_000));
        assert_eq!(parse_brl("-45,9"), Ok(-4_590));
        assert_eq!(parse_brl("-45.90"), Ok(-4_590));
        assert_eq!(parse_brl("1234.5"), Ok(123_450));
        assert_eq!(parse_brl("(12,00)"), Ok(-1_200));
        assert_eq!(parse_brl("7"), Ok(700));
    }

    #[test]
    fn rejects_garbage_and_extra_precision() {
        for bad in ["", "abc", "1,234,56", "10,999", "R$", "--1", "1e5"] {
            assert!(parse_brl(bad).is_err(), "deveria rejeitar {bad:?}");
        }
    }

    #[test]
    fn rejects_overflow() {
        assert!(parse_brl("99999999999999999999").is_err());
    }

    #[test]
    fn formats_with_thousands() {
        assert_eq!(Brl(123_456).to_string(), "R$ 1.234,56");
        assert_eq!(Brl(-4_590).to_string(), "-R$ 45,90");
        assert_eq!(Brl(5).to_string(), "R$ 0,05");
        assert_eq!(Brl(100_000_000).to_string(), "R$ 1.000.000,00");
    }
}

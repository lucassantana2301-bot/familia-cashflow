//! Relatório mensal: entradas, saídas, taxa de poupança e uso de cada envelope.
//! Função pura sobre dados já carregados, para ser testada sem banco.

use std::collections::HashMap;

use serde::Serialize;
use uuid::Uuid;

/// Envelope a partir do qual o front alerta (80% do orçamento).
pub const WARNING_BP: i64 = 8_000;

#[derive(Debug, Clone)]
pub struct TxRow {
    pub amount_cents: i64,
    pub category_id: Option<Uuid>,
    pub member_id: Option<Uuid>,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct CategoryRow {
    pub id: Uuid,
    pub name: String,
    pub is_expense: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnvelopeStatus {
    NoBudget,
    Ok,
    Warning,
    Over,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Envelope {
    pub category_id: Option<Uuid>,
    pub name: String,
    pub spent_cents: i64,
    pub limit_cents: Option<i64>,
    /// Uso do orçamento em pontos-base (10000 = 100%). Inteiro para não depender de float.
    pub used_bp: Option<i64>,
    pub status: EnvelopeStatus,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberSpend {
    pub member_id: Option<Uuid>,
    pub name: String,
    pub spent_cents: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopExpense {
    pub description: String,
    pub amount_cents: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonthlyReport {
    pub month: String,
    pub income_cents: i64,
    pub expense_cents: i64,
    pub net_cents: i64,
    /// (entradas - saídas) / entradas, em pontos-base. `None` sem entradas no mês.
    pub savings_rate_bp: Option<i64>,
    pub envelopes: Vec<Envelope>,
    pub members: Vec<MemberSpend>,
    pub top_expenses: Vec<TopExpense>,
}

fn ratio_bp(part: i64, whole: i64) -> Option<i64> {
    (whole > 0).then(|| ((part as i128 * 10_000) / whole as i128) as i64)
}

pub fn build_monthly_report(
    month: &str,
    txs: &[TxRow],
    categories: &[CategoryRow],
    budgets: &HashMap<Uuid, i64>,
    members: &HashMap<Uuid, String>,
) -> MonthlyReport {
    let income_cents: i64 = txs
        .iter()
        .filter(|t| t.amount_cents > 0)
        .map(|t| t.amount_cents)
        .sum();
    let expense_cents: i64 = txs
        .iter()
        .filter(|t| t.amount_cents < 0)
        .map(|t| -t.amount_cents)
        .sum();
    let net_cents = income_cents - expense_cents;

    let mut spent_by_category: HashMap<Option<Uuid>, i64> = HashMap::new();
    let mut spent_by_member: HashMap<Option<Uuid>, i64> = HashMap::new();
    for t in txs.iter().filter(|t| t.amount_cents < 0) {
        *spent_by_category.entry(t.category_id).or_default() += -t.amount_cents;
        *spent_by_member.entry(t.member_id).or_default() += -t.amount_cents;
    }

    let mut envelopes: Vec<Envelope> = categories
        .iter()
        .filter(|c| c.is_expense)
        .filter_map(|c| {
            let spent = spent_by_category.get(&Some(c.id)).copied().unwrap_or(0);
            let limit = budgets.get(&c.id).copied();
            if spent == 0 && limit.is_none() {
                return None;
            }
            let used_bp = limit.and_then(|l| ratio_bp(spent, l));
            let status = match used_bp {
                None => EnvelopeStatus::NoBudget,
                Some(bp) if bp > 10_000 => EnvelopeStatus::Over,
                Some(bp) if bp >= WARNING_BP => EnvelopeStatus::Warning,
                Some(_) => EnvelopeStatus::Ok,
            };
            Some(Envelope {
                category_id: Some(c.id),
                name: c.name.clone(),
                spent_cents: spent,
                limit_cents: limit,
                used_bp,
                status,
            })
        })
        .collect();

    // Gasto em categoria que não é de despesa ou sem categoria vai para "Sem categoria".
    let known: std::collections::HashSet<Uuid> = categories
        .iter()
        .filter(|c| c.is_expense)
        .map(|c| c.id)
        .collect();
    let uncategorized: i64 = spent_by_category
        .iter()
        .filter(|(id, _)| id.map_or(true, |id| !known.contains(&id)))
        .map(|(_, v)| *v)
        .sum();
    if uncategorized > 0 {
        envelopes.push(Envelope {
            category_id: None,
            name: "Sem categoria".into(),
            spent_cents: uncategorized,
            limit_cents: None,
            used_bp: None,
            status: EnvelopeStatus::NoBudget,
        });
    }
    // Estourados primeiro, depois por uso e por gasto.
    envelopes.sort_by(|a, b| {
        b.used_bp
            .unwrap_or(-1)
            .cmp(&a.used_bp.unwrap_or(-1))
            .then(b.spent_cents.cmp(&a.spent_cents))
    });

    let mut members_out: Vec<MemberSpend> = spent_by_member
        .into_iter()
        .map(|(id, spent)| MemberSpend {
            member_id: id,
            name: id
                .and_then(|id| members.get(&id).cloned())
                .unwrap_or_else(|| "Conta conjunta".into()),
            spent_cents: spent,
        })
        .collect();
    members_out.sort_by_key(|m| std::cmp::Reverse(m.spent_cents));

    let mut expenses: Vec<&TxRow> = txs.iter().filter(|t| t.amount_cents < 0).collect();
    expenses.sort_by_key(|t| t.amount_cents);
    let top_expenses = expenses
        .into_iter()
        .take(5)
        .map(|t| TopExpense {
            description: t.description.clone(),
            amount_cents: -t.amount_cents,
        })
        .collect();

    MonthlyReport {
        month: month.to_string(),
        income_cents,
        expense_cents,
        net_cents,
        savings_rate_bp: ratio_bp(net_cents, income_cents),
        envelopes,
        members: members_out,
        top_expenses,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn tx(amount: i64, category: Option<u128>, member: Option<u128>, d: &str) -> TxRow {
        TxRow {
            amount_cents: amount,
            category_id: category.map(id),
            member_id: member.map(id),
            description: d.into(),
        }
    }

    #[test]
    fn computes_totals_envelopes_and_savings_rate() {
        let categories = vec![
            CategoryRow {
                id: id(1),
                name: "Mercado".into(),
                is_expense: true,
            },
            CategoryRow {
                id: id(2),
                name: "Lazer".into(),
                is_expense: true,
            },
            CategoryRow {
                id: id(3),
                name: "Transporte".into(),
                is_expense: true,
            },
            CategoryRow {
                id: id(9),
                name: "Salário".into(),
                is_expense: false,
            },
        ];
        let budgets = HashMap::from([(id(1), 100_000), (id(2), 20_000), (id(3), 50_000)]);
        let members = HashMap::from([(id(100), "Lucas".to_string()), (id(101), "Ana".to_string())]);
        let txs = vec![
            tx(1_000_000, Some(9), Some(100), "Salário"),
            tx(-85_000, Some(1), Some(101), "Mercado Extra"),
            tx(-25_000, Some(2), Some(100), "Cinema e jantar"),
            tx(-3_000, None, None, "Tarifa"),
        ];

        let r = build_monthly_report("2026-11", &txs, &categories, &budgets, &members);
        assert_eq!(r.income_cents, 1_000_000);
        assert_eq!(r.expense_cents, 113_000);
        assert_eq!(r.net_cents, 887_000);
        assert_eq!(r.savings_rate_bp, Some(8_870));

        // Lazer estourou (125%) e vem primeiro; Mercado em alerta (85%); Transporte zerado mas com orçamento.
        let names: Vec<_> = r
            .envelopes
            .iter()
            .map(|e| (e.name.as_str(), &e.status))
            .collect();
        assert_eq!(
            names,
            vec![
                ("Lazer", &EnvelopeStatus::Over),
                ("Mercado", &EnvelopeStatus::Warning),
                ("Transporte", &EnvelopeStatus::Ok),
                ("Sem categoria", &EnvelopeStatus::NoBudget),
            ]
        );
        assert_eq!(r.envelopes[0].used_bp, Some(12_500));

        assert_eq!(r.members[0].name, "Ana");
        assert_eq!(
            r.members
                .iter()
                .find(|m| m.member_id.is_none())
                .unwrap()
                .name,
            "Conta conjunta"
        );
        assert_eq!(r.top_expenses[0].description, "Mercado Extra");
    }

    #[test]
    fn handles_month_without_income() {
        let r = build_monthly_report(
            "2026-12",
            &[tx(-1_000, None, None, "x")],
            &[],
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(r.savings_rate_bp, None);
        assert_eq!(r.net_cents, -1_000);
    }
}

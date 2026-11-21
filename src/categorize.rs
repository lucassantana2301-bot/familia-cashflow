//! Categorização automática por regras: "se a descrição contém X, categoria Y".

use uuid::Uuid;

use crate::text::normalize;

#[derive(Debug, Clone)]
pub struct Rule {
    pub pattern: String,
    pub category_id: Uuid,
    pub priority: i32,
}

/// Regras pré-compiladas: padrões normalizados e ordenados por prioridade
/// (maior primeiro) e, no empate, pelo padrão mais específico (mais longo).
pub struct Categorizer {
    rules: Vec<(String, Uuid)>,
}

impl Categorizer {
    pub fn new(mut rules: Vec<Rule>) -> Self {
        rules.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| b.pattern.chars().count().cmp(&a.pattern.chars().count()))
        });
        Self {
            rules: rules
                .into_iter()
                .map(|r| (normalize(&r.pattern), r.category_id))
                .filter(|(p, _)| !p.is_empty())
                .collect(),
        }
    }

    pub fn categorize(&self, description: &str) -> Option<Uuid> {
        let haystack = normalize(description);
        self.rules
            .iter()
            .find(|(pattern, _)| haystack.contains(pattern.as_str()))
            .map(|(_, id)| *id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(pattern: &str, id: u128, priority: i32) -> Rule {
        Rule {
            pattern: pattern.into(),
            category_id: Uuid::from_u128(id),
            priority,
        }
    }

    #[test]
    fn matches_ignoring_case_and_accents() {
        let c = Categorizer::new(vec![rule("padaria", 1, 0), rule("Farmácia", 2, 0)]);
        assert_eq!(
            c.categorize("PAG*PADARIA SAO JOAO"),
            Some(Uuid::from_u128(1))
        );
        assert_eq!(
            c.categorize("FARMACIA DROGA RAIA"),
            Some(Uuid::from_u128(2))
        );
        assert_eq!(c.categorize("POSTO SHELL"), None);
    }

    #[test]
    fn priority_then_specificity_wins() {
        let c = Categorizer::new(vec![
            rule("uber", 1, 0),
            rule("uber eats", 2, 0),
            rule("ifood", 3, 10),
        ]);
        assert_eq!(c.categorize("UBER EATS *PEDIDO"), Some(Uuid::from_u128(2)));
        assert_eq!(c.categorize("UBER *TRIP"), Some(Uuid::from_u128(1)));
        // Prioridade maior vence mesmo com padrão mais curto.
        let c = Categorizer::new(vec![rule("ifood uber", 1, 0), rule("ifood", 3, 10)]);
        assert_eq!(c.categorize("IFOOD UBER"), Some(Uuid::from_u128(3)));
    }
}

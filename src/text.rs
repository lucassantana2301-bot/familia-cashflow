//! Normalização de texto para comparar descrições de extrato.

/// Minúsculas, sem acento e com espaços colapsados:
/// `"PAG*Padaria  São João"` -> `"pag*padaria sao joao"`.
pub fn normalize(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_space = true;
    for ch in input.chars().flat_map(char::to_lowercase) {
        let base = match ch {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            c => c,
        };
        if base.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(base);
            last_space = false;
        }
    }
    out.trim_end().to_string()
}

/// Remove caracteres de controle e limita o tamanho (descrições vêm de arquivos externos).
pub fn sanitize_description(input: &str, max_chars: usize) -> String {
    let cleaned: String = input
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_accents_case_and_spaces() {
        assert_eq!(
            normalize("  PAG*Padaria  São João "),
            "pag*padaria sao joao"
        );
        assert_eq!(normalize("AÇOUGUE"), "acougue");
    }

    #[test]
    fn sanitizes_descriptions() {
        assert_eq!(
            sanitize_description("UBER\t*TRIP\r\nSP", 50),
            "UBER *TRIP SP"
        );
        assert_eq!(sanitize_description("abcdef", 3), "abc");
    }
}

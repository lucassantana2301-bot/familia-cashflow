//! Senhas com Argon2id e sessões com token opaco.
//!
//! Por que não JWT: sessão opaca pode ser revogada no logout e não carrega
//! dados no cliente. O banco guarda só o SHA-256 do token.

use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};

pub const MIN_PASSWORD_LEN: usize = 10;
pub const MAX_PASSWORD_LEN: usize = 128;

fn argon2() -> Argon2<'static> {
    // Parâmetros recomendados pela OWASP para Argon2id (19 MiB, 2 iterações).
    let params = Params::new(19 * 1024, 2, 1, None).expect("parâmetros Argon2 válidos");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

pub fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(argon2()
        .hash_password(password.as_bytes(), &salt)?
        .to_string())
}

pub fn verify_password(password: &str, stored_hash: &str) -> bool {
    PasswordHash::new(stored_hash)
        .map(|parsed| {
            argon2()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
        .unwrap_or(false)
}

/// Hash fixo usado quando o e-mail não existe, para o login levar o mesmo
/// tempo nos dois casos (evita enumeração de usuários por tempo de resposta).
pub fn dummy_hash() -> &'static str {
    static DUMMY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    DUMMY.get_or_init(|| hash_password("senha-que-nunca-sera-usada").expect("hash do dummy"))
}

pub fn validate_password(password: &str) -> Result<(), &'static str> {
    let len = password.chars().count();
    if len < MIN_PASSWORD_LEN {
        return Err("senha precisa de pelo menos 10 caracteres");
    }
    if len > MAX_PASSWORD_LEN {
        return Err("senha muito longa");
    }
    Ok(())
}

/// Token de 256 bits, enviado ao cliente uma única vez.
pub fn new_session_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn token_hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

pub fn normalize_email(email: &str) -> Option<String> {
    let email = email.trim().to_lowercase();
    let (local, domain) = email.split_once('@')?;
    let valid = !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && email.len() <= 254
        && !email.chars().any(char::is_whitespace);
    valid.then_some(email)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_and_verifies() {
        let hash = hash_password("correta-cavalo-bateria").unwrap();
        assert!(hash.starts_with("$argon2id$"));
        assert!(verify_password("correta-cavalo-bateria", &hash));
        assert!(!verify_password("errada", &hash));
        assert!(!verify_password("x", "lixo"));
    }

    #[test]
    fn tokens_are_unique_and_hash_is_stable() {
        let a = new_session_token();
        assert_ne!(a, new_session_token());
        assert_eq!(a.len(), 43);
        assert_eq!(token_hash(&a), token_hash(&a));
    }

    #[test]
    fn validates_inputs() {
        assert!(validate_password("curta").is_err());
        assert!(validate_password("suficientemente-longa").is_ok());
        assert_eq!(
            normalize_email(" Lucas@Exemplo.com "),
            Some("lucas@exemplo.com".into())
        );
        assert_eq!(normalize_email("sem-arroba"), None);
        assert_eq!(normalize_email("a@b"), None);
    }
}

//! Limite de tentativas por IP em janela fixa, para login e cadastro.
//! Em memória: suficiente para uma instância; com várias réplicas, trocar por Redis.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct RateLimiter {
    max: u32,
    window: Duration,
    hits: Mutex<HashMap<String, (Instant, u32)>>,
}

impl RateLimiter {
    pub fn new(max: u32, window: Duration) -> Self {
        Self {
            max,
            window,
            hits: Mutex::new(HashMap::new()),
        }
    }

    /// `true` se a requisição pode seguir.
    pub fn check(&self, key: &str) -> bool {
        let now = Instant::now();
        let mut hits = self
            .hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // Limpeza preguiçosa para o mapa não crescer sem limite.
        if hits.len() > 10_000 {
            hits.retain(|_, (start, _)| now.duration_since(*start) < self.window);
        }

        let entry = hits.entry(key.to_string()).or_insert((now, 0));
        if now.duration_since(entry.0) >= self.window {
            *entry = (now, 0);
        }
        entry.1 += 1;
        entry.1 <= self.max
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_after_limit_per_key() {
        let rl = RateLimiter::new(2, Duration::from_secs(60));
        assert!(rl.check("1.1.1.1"));
        assert!(rl.check("1.1.1.1"));
        assert!(!rl.check("1.1.1.1"));
        assert!(rl.check("2.2.2.2"));
    }

    #[test]
    fn resets_after_window() {
        let rl = RateLimiter::new(1, Duration::from_millis(20));
        assert!(rl.check("k"));
        assert!(!rl.check("k"));
        std::thread::sleep(Duration::from_millis(30));
        assert!(rl.check("k"));
    }
}

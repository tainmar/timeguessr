use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sqlx::PgPool;
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub http: reqwest::Client,
    /// Bounds concurrent image decodes so a burst of uploads can't exhaust memory.
    pub decode_permits: Arc<Semaphore>,
    pub pin_guard: Arc<PinGuard>,
}

impl AppState {
    pub fn new(db: PgPool, config: Config) -> Self {
        Self {
            db,
            config: Arc::new(config),
            http: reqwest::Client::builder()
                .user_agent("timeguessr/1.0")
                .build()
                .expect("http client"),
            decode_permits: Arc::new(Semaphore::new(2)),
            pin_guard: Arc::new(PinGuard::default()),
        }
    }
}

/// Throttles team PIN guesses (4 digits) to make brute forcing impractical.
#[derive(Default)]
pub struct PinGuard {
    failures: Mutex<HashMap<Uuid, (u32, Instant)>>,
}

const MAX_PIN_FAILURES: u32 = 8;
const PIN_WINDOW: Duration = Duration::from_secs(15 * 60);

impl PinGuard {
    pub fn is_locked(&self, team: Uuid) -> bool {
        let map = self.failures.lock().unwrap();
        matches!(map.get(&team), Some((n, since)) if *n >= MAX_PIN_FAILURES && since.elapsed() < PIN_WINDOW)
    }

    pub fn record_failure(&self, team: Uuid) {
        let mut map = self.failures.lock().unwrap();
        let entry = map.entry(team).or_insert((0, Instant::now()));
        if entry.1.elapsed() >= PIN_WINDOW {
            *entry = (0, Instant::now());
        }
        entry.0 += 1;
    }

    pub fn clear(&self, team: Uuid) {
        self.failures.lock().unwrap().remove(&team);
    }
}

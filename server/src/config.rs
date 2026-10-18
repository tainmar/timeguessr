use std::env;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MailDriver {
    /// Send through the Resend HTTP API.
    Resend,
    /// Only log the email (local development and E2E tests).
    Log,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub port: u16,
    /// Public base URL used to build links in emails, without trailing slash.
    pub app_url: String,
    pub static_dir: String,
    pub mail_driver: MailDriver,
    pub resend_api_key: Option<String>,
    pub mail_from: String,
    /// Optional allowlist of organizer emails (`alice@x.com`) or domains (`@x.com`).
    /// Empty means anyone can sign up.
    pub organizer_allowlist: Vec<String>,
    pub max_participants_per_game: i64,
    pub max_emails_per_organizer_per_day: i64,
    pub outbox_interval_ms: u64,
    pub scheduler_interval_secs: u64,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let app_url = env::var("APP_URL")
            .unwrap_or_else(|_| "http://localhost:8080".into())
            .trim_end_matches('/')
            .to_string();
        let mail_driver = match env::var("MAIL_DRIVER").unwrap_or_else(|_| "log".into()).as_str() {
            "resend" => MailDriver::Resend,
            "log" => MailDriver::Log,
            other => anyhow::bail!("unknown MAIL_DRIVER {other:?} (expected resend|log)"),
        };
        let resend_api_key = env::var("RESEND_API_KEY").ok().filter(|k| !k.is_empty());
        if mail_driver == MailDriver::Resend && resend_api_key.is_none() {
            anyhow::bail!("MAIL_DRIVER=resend requires RESEND_API_KEY");
        }
        let organizer_allowlist = env::var("ORGANIZER_EMAILS")
            .unwrap_or_default()
            .split([',', ';', ' ', '\n'])
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();

        Ok(Self {
            database_url: env::var("DATABASE_URL")
                .map_err(|_| anyhow::anyhow!("DATABASE_URL must be set"))?,
            port: parse_env("PORT", 8080),
            app_url,
            static_dir: env::var("STATIC_DIR").unwrap_or_else(|_| "../web/dist".into()),
            mail_driver,
            resend_api_key,
            mail_from: env::var("MAIL_FROM")
                .unwrap_or_else(|_| "TimeGuessr <noreply@timeguessr.procrastination.land>".into()),
            organizer_allowlist,
            max_participants_per_game: parse_env("MAX_PARTICIPANTS_PER_GAME", 200),
            max_emails_per_organizer_per_day: parse_env("MAX_EMAILS_PER_ORGANIZER_PER_DAY", 400),
            outbox_interval_ms: parse_env("OUTBOX_INTERVAL_MS", 2000),
            scheduler_interval_secs: parse_env("SCHEDULER_INTERVAL_SECS", 30),
        })
    }

    pub fn secure_cookies(&self) -> bool {
        self.app_url.starts_with("https://")
    }

    pub fn organizer_allowed(&self, email: &str) -> bool {
        if self.organizer_allowlist.is_empty() {
            return true;
        }
        let email = email.to_lowercase();
        self.organizer_allowlist.iter().any(|entry| {
            if entry.starts_with('@') {
                email.ends_with(entry.as_str())
            } else {
                *entry == email
            }
        })
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.app_url, path)
    }
}

fn parse_env<T: std::str::FromStr>(key: &str, default: T) -> T {
    env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

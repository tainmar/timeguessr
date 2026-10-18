use std::time::Duration;

use serde_json::json;

use crate::config::{Config, MailDriver};

/// What the outbox row needs to be delivered.
pub struct Delivery<'a> {
    pub id: uuid::Uuid,
    pub to: &'a str,
    pub subject: &'a str,
    pub html: &'a str,
    pub text: &'a str,
    pub reply_to: Option<&'a str>,
}

#[derive(Debug)]
pub enum SendError {
    /// Try again later (rate limited, provider or network error).
    Retry {
        message: String,
        after: Option<Duration>,
    },
    /// The provider's daily quota is exhausted: retry after midnight UTC.
    QuotaExceeded(String),
    /// Will never succeed (invalid address, unverified domain...).
    Permanent(String),
}

pub async fn send(
    http: &reqwest::Client,
    config: &Config,
    email: &Delivery<'_>,
) -> Result<Option<String>, SendError> {
    match config.mail_driver {
        MailDriver::Log => {
            tracing::info!(to = %email.to, subject = %email.subject, "email (log driver)");
            Ok(None)
        }
        MailDriver::Resend => send_resend(http, config, email).await.map(Some),
    }
}

async fn send_resend(
    http: &reqwest::Client,
    config: &Config,
    email: &Delivery<'_>,
) -> Result<String, SendError> {
    let key = config.resend_api_key.as_deref().unwrap_or_default();
    let mut body = json!({
        "from": config.mail_from,
        "to": [email.to],
        "subject": email.subject,
        "html": email.html,
        "text": email.text,
    });
    if let Some(reply_to) = email.reply_to {
        body["reply_to"] = json!(reply_to);
    }
    let response = http
        .post("https://api.resend.com/emails")
        .bearer_auth(key)
        // Resend dedupes identical requests with the same key for 24h, so a retry after
        // a timeout (or two workers racing during a deploy) never sends twice.
        .header("Idempotency-Key", format!("outbox-{}", email.id))
        .json(&body)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| SendError::Retry {
            message: format!("network error: {e}"),
            after: None,
        })?;

    let status = response.status();
    let retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs);
    let payload: serde_json::Value = response.json().await.unwrap_or_default();

    if status.is_success() {
        return payload["id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| SendError::Retry {
                message: "missing id in Resend response".into(),
                after: None,
            });
    }
    let name = payload["name"].as_str().unwrap_or_default();
    let message = format!(
        "Resend {}: {}",
        status.as_u16(),
        payload["message"].as_str().unwrap_or(name)
    );
    match status.as_u16() {
        429 if name.contains("quota") => Err(SendError::QuotaExceeded(message)),
        429 | 408 | 409 => Err(SendError::Retry {
            message,
            after: retry_after,
        }),
        s if s >= 500 => Err(SendError::Retry {
            message,
            after: retry_after,
        }),
        _ => Err(SendError::Permanent(message)),
    }
}

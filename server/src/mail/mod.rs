//! Outgoing email: rendered into the `emails` outbox table, delivered by the worker.

pub mod sender;
pub mod templates;

use sqlx::PgConnection;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone)]
pub struct OutgoingEmail {
    pub kind: &'static str,
    pub to: String,
    pub subject: String,
    pub html: String,
    pub text: String,
    pub reply_to: Option<String>,
    pub organizer_id: Option<Uuid>,
    pub game_id: Option<Uuid>,
    pub participant_id: Option<Uuid>,
}

/// Queue an email in the outbox (inside the caller's transaction when there is one).
pub async fn enqueue(conn: &mut PgConnection, email: &OutgoingEmail) -> AppResult<Uuid> {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO emails (kind, to_email, subject, html, text_body, reply_to, organizer_id, game_id, participant_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING id",
    )
    .bind(email.kind)
    .bind(&email.to)
    .bind(&email.subject)
    .bind(&email.html)
    .bind(&email.text)
    .bind(&email.reply_to)
    .bind(email.organizer_id)
    .bind(email.game_id)
    .bind(email.participant_id)
    .fetch_one(conn)
    .await?;
    Ok(id)
}

/// Refuse to queue `count` more emails for an organizer above the daily cap.
/// Protects the sending domain (and the Resend quota) from abuse.
pub async fn check_daily_cap(
    conn: &mut PgConnection,
    organizer_id: Uuid,
    count: i64,
    cap: i64,
) -> AppResult<()> {
    let sent: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM emails WHERE organizer_id = $1 AND kind IN ('invite', 'reminder', 'play') AND created_at > now() - interval '24 hours'",
    )
    .bind(organizer_id)
    .fetch_one(conn)
    .await?;
    if sent + count > cap {
        return Err(AppError::TooManyRequests(format!(
            "Daily email limit reached ({cap} emails per 24h). Try again later or share the links manually."
        )));
    }
    Ok(())
}

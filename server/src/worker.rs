//! Background jobs: email outbox delivery and the reminder/deadline scheduler.
//! Rows are claimed with `FOR UPDATE SKIP LOCKED` + a lease, so overlapping
//! instances (e.g. during a deploy) never process the same row twice.

use std::time::Duration;

use chrono::{DateTime, Days, Utc};
use uuid::Uuid;

use crate::domain::{self, CollectionEmailKind};
use crate::error::AppResult;
use crate::mail::sender::{self, Delivery, SendError};
use crate::notifications::{self, DeadlineReached, EmailFailed};
use crate::state::AppState;

const MAX_ATTEMPTS: i32 = 6;

pub fn spawn(state: AppState) {
    let outbox_state = state.clone();
    tokio::spawn(async move {
        let interval = Duration::from_millis(outbox_state.config.outbox_interval_ms);
        loop {
            if let Err(e) = process_outbox(&outbox_state).await {
                tracing::error!(error = ?e, "outbox worker failed");
            }
            tokio::time::sleep(interval).await;
        }
    });
    tokio::spawn(async move {
        let interval = Duration::from_secs(state.config.scheduler_interval_secs);
        loop {
            if let Err(e) = run_scheduler(&state).await {
                tracing::error!(error = ?e, "scheduler failed");
            }
            tokio::time::sleep(interval).await;
        }
    });
}

#[derive(sqlx::FromRow)]
struct QueuedEmail {
    id: Uuid,
    to_email: String,
    subject: String,
    html: String,
    text_body: String,
    reply_to: Option<String>,
    attempts: i32,
    organizer_id: Option<Uuid>,
    game_id: Option<Uuid>,
    participant_id: Option<Uuid>,
}

pub async fn process_outbox(state: &AppState) -> AppResult<usize> {
    let batch = sqlx::query_as::<_, QueuedEmail>(
        "UPDATE emails SET status = 'sending', attempts = attempts + 1, locked_until = now() + interval '2 minutes'
         WHERE id IN (
             SELECT id FROM emails
             WHERE (status = 'pending' AND next_attempt_at <= now())
                OR (status = 'sending' AND locked_until < now())
             ORDER BY created_at
             LIMIT 20
             FOR UPDATE SKIP LOCKED
         )
         RETURNING id, to_email, subject, html, text_body, reply_to, attempts, organizer_id, game_id, participant_id",
    )
    .fetch_all(&state.db)
    .await?;

    let count = batch.len();
    for email in batch {
        let delivery = Delivery {
            id: email.id,
            to: &email.to_email,
            subject: &email.subject,
            html: &email.html,
            text: &email.text_body,
            reply_to: email.reply_to.as_deref(),
        };
        match sender::send(&state.http, &state.config, &delivery).await {
            Ok(provider_id) => {
                sqlx::query(
                    "UPDATE emails SET status = 'sent', sent_at = now(), provider_id = $2, last_error = NULL, locked_until = NULL WHERE id = $1",
                )
                .bind(email.id)
                .bind(provider_id)
                .execute(&state.db)
                .await?;
            }
            Err(SendError::QuotaExceeded(message)) => {
                // Provider's daily quota: wait until the next UTC day.
                let tomorrow = next_utc_midnight(Utc::now()) + chrono::Duration::minutes(5);
                tracing::warn!(email = %email.id, %message, "email quota exceeded, deferring");
                reschedule(state, email.id, tomorrow, &format!("Deferred: {message}"), true).await?;
            }
            Err(SendError::Retry { message, after }) if email.attempts < MAX_ATTEMPTS => {
                let backoff = after.unwrap_or(Duration::from_secs(30 * (email.attempts as u64).pow(2)));
                let at = Utc::now() + chrono::Duration::from_std(backoff).unwrap_or_default();
                tracing::warn!(email = %email.id, %message, "email send failed, retrying");
                reschedule(state, email.id, at, &message, false).await?;
            }
            Err(SendError::Retry { message, .. }) | Err(SendError::Permanent(message)) => {
                tracing::error!(email = %email.id, %message, "email permanently failed");
                sqlx::query("UPDATE emails SET status = 'failed', last_error = $2, locked_until = NULL WHERE id = $1")
                    .bind(email.id)
                    .bind(&message)
                    .execute(&state.db)
                    .await?;
                if let (Some(organizer_id), true) = (email.organizer_id, email.participant_id.is_some()) {
                    let mut conn = state.db.acquire().await?;
                    notifications::notify(
                        &mut conn,
                        &state.config,
                        organizer_id,
                        &EmailFailed { game_id: email.game_id, recipient: email.to_email.clone(), error: message },
                    )
                    .await?;
                }
            }
        }
        // Stay well under Resend's per-second rate limit.
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Ok(count)
}

async fn reschedule(
    state: &AppState,
    id: Uuid,
    at: DateTime<Utc>,
    error: &str,
    refund_attempt: bool,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE emails SET status = 'pending', next_attempt_at = $2, last_error = $3, locked_until = NULL,
                attempts = attempts - CASE WHEN $4 THEN 1 ELSE 0 END
         WHERE id = $1",
    )
    .bind(id)
    .bind(at)
    .bind(error)
    .bind(refund_attempt)
    .execute(&state.db)
    .await?;
    Ok(())
}

fn next_utc_midnight(now: DateTime<Utc>) -> DateTime<Utc> {
    (now.date_naive() + Days::new(1))
        .and_hms_opt(0, 0, 0)
        .expect("midnight")
        .and_utc()
}

pub async fn run_scheduler(state: &AppState) -> AppResult<()> {
    send_due_reminders(state).await?;
    notify_deadlines(state).await?;
    sqlx::query("DELETE FROM sessions WHERE expires_at < now()")
        .execute(&state.db)
        .await?;
    sqlx::query("DELETE FROM login_tokens WHERE expires_at < now() - interval '1 day'")
        .execute(&state.db)
        .await?;
    Ok(())
}

async fn send_due_reminders(state: &AppState) -> AppResult<()> {
    loop {
        let mut tx = state.db.begin().await?;
        let due: Option<(Uuid, Uuid)> = sqlx::query_as(
            "SELECT id, game_id FROM reminders
             WHERE status = 'scheduled' AND send_at <= now()
             ORDER BY send_at DESC LIMIT 1 FOR UPDATE SKIP LOCKED",
        )
        .fetch_optional(&mut *tx)
        .await?;
        let Some((reminder_id, game_id)) = due else {
            return Ok(());
        };

        // After downtime several reminders of a game can be overdue: send only one.
        sqlx::query(
            "UPDATE reminders SET status = 'skipped', processed_at = now()
             WHERE game_id = $1 AND id <> $2 AND status = 'scheduled' AND send_at <= now()",
        )
        .bind(game_id)
        .bind(reminder_id)
        .execute(&mut *tx)
        .await?;

        let game = domain::lock_game(&mut tx, game_id, false).await?;
        let mut recipients = 0i64;
        let status = if game.status == domain::STATUS_COLLECTING {
            let sender = domain::load_sender(&mut tx, game.organizer_id).await?;
            let stats = domain::collection_stats(&mut tx, game.id).await?;
            let pending: Vec<_> = domain::game_progress(&mut tx, game.id)
                .await?
                .into_iter()
                .filter(|p| !p.ready)
                .collect();
            for participant in &pending {
                domain::enqueue_collection_email(
                    &mut tx,
                    &state.config,
                    &game,
                    &sender,
                    participant,
                    stats,
                    CollectionEmailKind::Reminder,
                )
                .await?;
            }
            recipients = pending.len() as i64;
            "sent"
        } else {
            "skipped"
        };
        sqlx::query("UPDATE reminders SET status = $2, processed_at = now(), recipients = $3 WHERE id = $1")
            .bind(reminder_id)
            .bind(status)
            .bind(recipients as i32)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        tracing::info!(%reminder_id, %game_id, recipients, status, "reminder processed");
    }
}

async fn notify_deadlines(state: &AppState) -> AppResult<()> {
    let mut tx = state.db.begin().await?;
    let due: Vec<Uuid> = sqlx::query_scalar(
        "UPDATE games SET deadline_notified_at = now()
         WHERE status = 'collecting' AND deadline_at <= now() AND deadline_notified_at IS NULL
         RETURNING id",
    )
    .fetch_all(&mut *tx)
    .await?;
    for game_id in due {
        let game = domain::load_game(&mut tx, game_id).await?;
        let stats = domain::collection_stats(&mut tx, game_id).await?;
        notifications::notify(
            &mut tx,
            &state.config,
            game.organizer_id,
            &DeadlineReached {
                game_id,
                game_name: game.name.clone(),
                ready: stats.ready,
                total: stats.total,
                photos: stats.photos,
            },
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

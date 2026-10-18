//! Game domain helpers shared by routes and the background worker.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::config::Config;
use crate::error::AppResult;
use crate::mail::{self, templates, OutgoingEmail};
use crate::notifications::{self, CollectionComplete, ParticipantReady};
use crate::schedule;

pub const STATUS_DRAFT: &str = "draft";
pub const STATUS_COLLECTING: &str = "collecting";
pub const STATUS_READY: &str = "ready";
pub const STATUS_FINISHED: &str = "finished";

pub const GAME_COLUMNS: &str = "id, organizer_id, name, status, photos_per_participant, message, timezone, deadline_at, \
     collection_started_at, ready_at, finished_at, play_code, joining_closed, created_at, updated_at";

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Game {
    pub id: Uuid,
    #[serde(skip)]
    pub organizer_id: Uuid,
    pub name: String,
    pub status: String,
    pub photos_per_participant: i32,
    pub message: Option<String>,
    pub timezone: String,
    pub deadline_at: Option<DateTime<Utc>>,
    pub collection_started_at: Option<DateTime<Utc>>,
    pub ready_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub play_code: Option<String>,
    pub joining_closed: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Game {
    pub fn deadline_label(&self) -> Option<String> {
        self.deadline_at
            .map(|d| schedule::format_deadline(d, schedule::tz_or_utc(&self.timezone)))
    }

    pub fn play_url(&self, config: &Config) -> Option<String> {
        self.play_code.as_ref().map(|c| config.url(&format!("/play/{c}")))
    }
}

pub async fn load_game(conn: &mut PgConnection, id: Uuid) -> AppResult<Game> {
    Ok(sqlx::query_as::<_, Game>(&format!("SELECT {GAME_COLUMNS} FROM games WHERE id = $1"))
        .bind(id)
        .fetch_one(conn)
        .await?)
}

/// Row lock on the game: `FOR UPDATE` for state transitions, `FOR SHARE` for writes
/// that must not interleave with a transition (e.g. an upload racing "game ready").
pub async fn lock_game(conn: &mut PgConnection, id: Uuid, exclusive: bool) -> AppResult<Game> {
    let lock = if exclusive { "FOR UPDATE" } else { "FOR SHARE" };
    Ok(sqlx::query_as::<_, Game>(&format!("SELECT {GAME_COLUMNS} FROM games WHERE id = $1 {lock}"))
        .bind(id)
        .fetch_one(conn)
        .await?)
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Progress {
    pub id: Uuid,
    pub game_id: Uuid,
    pub email: String,
    pub name: Option<String>,
    pub token: String,
    pub done_at: Option<DateTime<Utc>>,
    pub ready_notified_at: Option<DateTime<Utc>>,
    pub invited_at: Option<DateTime<Utc>>,
    pub photo_count: i64,
    pub ready: bool,
}

pub const PROGRESS_COLUMNS: &str =
    "id, game_id, email, name, token, done_at, ready_notified_at, invited_at, photo_count, ready";

impl Progress {
    /// `not_started`, `partial` or `ready`.
    pub fn status(&self) -> &'static str {
        if self.ready {
            "ready"
        } else if self.photo_count > 0 {
            "partial"
        } else {
            "not_started"
        }
    }

    pub fn display_name(&self) -> String {
        self.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| self.email.clone())
    }
}

pub async fn participant_progress(conn: &mut PgConnection, participant_id: Uuid) -> AppResult<Progress> {
    Ok(sqlx::query_as::<_, Progress>(&format!(
        "SELECT {PROGRESS_COLUMNS} FROM participant_progress WHERE id = $1"
    ))
    .bind(participant_id)
    .fetch_one(conn)
    .await?)
}

pub async fn game_progress(conn: &mut PgConnection, game_id: Uuid) -> AppResult<Vec<Progress>> {
    Ok(sqlx::query_as::<_, Progress>(&format!(
        "SELECT {PROGRESS_COLUMNS} FROM participant_progress WHERE game_id = $1 ORDER BY created_at, email"
    ))
    .bind(game_id)
    .fetch_all(conn)
    .await?)
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct CollectionStats {
    pub total: i64,
    pub ready: i64,
    pub pending: i64,
    pub photos: i64,
}

pub async fn collection_stats(conn: &mut PgConnection, game_id: Uuid) -> AppResult<CollectionStats> {
    let (total, ready, photos): (i64, i64, i64) = sqlx::query_as(
        "SELECT count(*), count(*) FILTER (WHERE ready), coalesce(sum(photo_count), 0)::bigint
         FROM participant_progress WHERE game_id = $1",
    )
    .bind(game_id)
    .fetch_one(conn)
    .await?;
    Ok(CollectionStats { total, ready, pending: total - ready, photos })
}

pub struct Sender {
    pub organizer_id: Uuid,
    pub display_name: String,
    pub email: String,
}

pub async fn load_sender(conn: &mut PgConnection, organizer_id: Uuid) -> AppResult<Sender> {
    let (email, name): (String, Option<String>) =
        sqlx::query_as("SELECT email, name FROM organizers WHERE id = $1")
            .bind(organizer_id)
            .fetch_one(conn)
            .await?;
    Ok(Sender {
        organizer_id,
        display_name: name.filter(|n| !n.is_empty()).unwrap_or_else(|| email.clone()),
        email,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CollectionEmailKind {
    Invite,
    Reminder,
}

/// Queue an invitation or reminder for one participant.
pub async fn enqueue_collection_email(
    conn: &mut PgConnection,
    config: &Config,
    game: &Game,
    sender: &Sender,
    participant: &Progress,
    stats: CollectionStats,
    kind: CollectionEmailKind,
) -> AppResult<()> {
    let url = config.url(&format!("/c/{}", participant.token));
    let deadline = game.deadline_label();
    let data = templates::CollectionEmail {
        game_name: &game.name,
        organizer: &sender.display_name,
        participant_name: participant.name.as_deref(),
        message: game.message.as_deref().filter(|m| !m.trim().is_empty()),
        photos_required: game.photos_per_participant,
        photos_uploaded: participant.photo_count,
        deadline: deadline.as_deref(),
        pending: stats.pending,
        total: stats.total,
        url: &url,
    };
    let (rendered, kind_name) = match kind {
        CollectionEmailKind::Invite => (templates::invitation(&data), "invite"),
        CollectionEmailKind::Reminder => (templates::reminder(&data), "reminder"),
    };
    mail::enqueue(
        &mut *conn,
        &OutgoingEmail {
            kind: kind_name,
            to: participant.email.clone(),
            subject: rendered.subject,
            html: rendered.html,
            text: rendered.text,
            reply_to: Some(sender.email.clone()),
            organizer_id: Some(sender.organizer_id),
            game_id: Some(game.id),
            participant_id: Some(participant.id),
        },
    )
    .await?;
    if kind == CollectionEmailKind::Invite {
        sqlx::query("UPDATE participants SET invited_at = now() WHERE id = $1")
            .bind(participant.id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

pub async fn enqueue_play_emails(
    conn: &mut PgConnection,
    config: &Config,
    game: &Game,
    sender: &Sender,
) -> AppResult<usize> {
    let Some(url) = game.play_url(config) else {
        return Ok(0);
    };
    let participants = game_progress(&mut *conn, game.id).await?;
    mail::check_daily_cap(
        &mut *conn,
        sender.organizer_id,
        participants.len() as i64,
        config.max_emails_per_organizer_per_day,
    )
    .await?;
    for p in &participants {
        let rendered = templates::play_link(&game.name, &sender.display_name, p.name.as_deref(), &url);
        mail::enqueue(
            &mut *conn,
            &OutgoingEmail {
                kind: "play",
                to: p.email.clone(),
                subject: rendered.subject,
                html: rendered.html,
                text: rendered.text,
                reply_to: Some(sender.email.clone()),
                organizer_id: Some(sender.organizer_id),
                game_id: Some(game.id),
                participant_id: Some(p.id),
            },
        )
        .await?;
    }
    Ok(participants.len())
}

/// Fire the "participant ready" / "everyone ready" notifications exactly once per
/// transition. Call inside the transaction that changed the participant's progress.
pub async fn after_progress_change(
    conn: &mut PgConnection,
    config: &Config,
    game: &Game,
    participant_id: Uuid,
) -> AppResult<()> {
    let progress = participant_progress(&mut *conn, participant_id).await?;
    if !progress.ready {
        if progress.ready_notified_at.is_some() {
            sqlx::query("UPDATE participants SET ready_notified_at = NULL WHERE id = $1")
                .bind(participant_id)
                .execute(&mut *conn)
                .await?;
        }
        return Ok(());
    }

    let newly_ready: Option<Uuid> = sqlx::query_scalar(
        "UPDATE participants SET ready_notified_at = now() WHERE id = $1 AND ready_notified_at IS NULL RETURNING id",
    )
    .bind(participant_id)
    .fetch_optional(&mut *conn)
    .await?;
    if newly_ready.is_none() {
        return Ok(());
    }

    let stats = collection_stats(&mut *conn, game.id).await?;
    notifications::notify(
        &mut *conn,
        config,
        game.organizer_id,
        &ParticipantReady {
            game_id: game.id,
            game_name: game.name.clone(),
            participant: progress.display_name(),
            declared_done: progress.photo_count < game.photos_per_participant as i64,
            photos: progress.photo_count,
            ready: stats.ready,
            total: stats.total,
        },
    )
    .await?;

    if stats.pending == 0 {
        let first_time: Option<Uuid> = sqlx::query_scalar(
            "UPDATE games SET all_ready_notified_at = now() WHERE id = $1 AND all_ready_notified_at IS NULL RETURNING id",
        )
        .bind(game.id)
        .fetch_optional(&mut *conn)
        .await?;
        if first_time.is_some() {
            notifications::notify(
                &mut *conn,
                config,
                game.organizer_id,
                &CollectionComplete {
                    game_id: game.id,
                    game_name: game.name.clone(),
                    total: stats.total,
                    photos: stats.photos,
                },
            )
            .await?;
        }
    }
    Ok(())
}

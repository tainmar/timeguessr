//! Organizer endpoints: game setup, participants, collection, play lifecycle, moderation.

use axum::extract::{Path, State};
use axum::Json;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::auth::Organizer;
use crate::domain::{
    self, CollectionEmailKind, Game, STATUS_COLLECTING, STATUS_DRAFT, STATUS_FINISHED, STATUS_READY,
};
use crate::error::{AppError, AppResult};
use crate::mail;
use crate::schedule;
use crate::state::AppState;
use crate::util::{clean_line, clean_text, parse_recipients, random_code};

async fn owned_game(
    conn: &mut PgConnection,
    organizer: &Organizer,
    id: Uuid,
    lock: bool,
) -> AppResult<Game> {
    let game = if lock {
        domain::lock_game(conn, id, true).await?
    } else {
        domain::load_game(conn, id).await?
    };
    if game.organizer_id != organizer.id {
        return Err(AppError::NotFound);
    }
    Ok(game)
}

#[derive(Serialize, sqlx::FromRow)]
pub struct GameListItem {
    id: Uuid,
    name: String,
    status: String,
    photos_per_participant: i32,
    deadline_at: Option<DateTime<Utc>>,
    timezone: String,
    created_at: DateTime<Utc>,
    participants: i64,
    ready_participants: i64,
    photos: i64,
    teams: i64,
}

pub async fn list(
    State(state): State<AppState>,
    organizer: Organizer,
) -> AppResult<Json<Vec<GameListItem>>> {
    let games = sqlx::query_as::<_, GameListItem>(
        "SELECT g.id, g.name, g.status, g.photos_per_participant, g.deadline_at, g.timezone, g.created_at,
                (SELECT count(*) FROM participant_progress p WHERE p.game_id = g.id) AS participants,
                (SELECT count(*) FROM participant_progress p WHERE p.game_id = g.id AND p.ready) AS ready_participants,
                (SELECT count(*) FROM photos ph WHERE ph.game_id = g.id) AS photos,
                (SELECT count(*) FROM teams t WHERE t.game_id = g.id) AS teams
         FROM games g WHERE g.organizer_id = $1 ORDER BY g.created_at DESC",
    )
    .bind(organizer.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(games))
}

#[derive(Deserialize)]
pub struct CreateGame {
    name: String,
}

pub async fn create(
    State(state): State<AppState>,
    organizer: Organizer,
    Json(body): Json<CreateGame>,
) -> AppResult<Json<Value>> {
    let name = clean_line(&body.name, 100);
    if name.is_empty() {
        return Err(AppError::bad("Please give your game a name."));
    }
    let recent: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM games WHERE organizer_id = $1 AND created_at > now() - interval '1 day'",
    )
    .bind(organizer.id)
    .fetch_one(&state.db)
    .await?;
    if recent >= 20 {
        return Err(AppError::TooManyRequests(
            "You've created a lot of games today. Please try again tomorrow.".into(),
        ));
    }
    let mut tx = state.db.begin().await?;
    let id: Uuid =
        sqlx::query_scalar("INSERT INTO games (organizer_id, name) VALUES ($1, $2) RETURNING id")
            .bind(organizer.id)
            .bind(name)
            .fetch_one(&mut *tx)
            .await?;
    // The organizer plays along by default (they can remove themselves before the game starts).
    sqlx::query("INSERT INTO participants (game_id, email, name, token) VALUES ($1, $2, $3, $4)")
        .bind(id)
        .bind(&organizer.email)
        .bind(&organizer.name)
        .bind(crate::util::random_token(24))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({ "id": id })))
}

/// participant_id, kind, status, last_error, created_at
type LastEmailRow = (Uuid, String, String, Option<String>, DateTime<Utc>);

#[derive(Serialize)]
struct LastEmail {
    kind: String,
    status: String,
    error: Option<String>,
    at: DateTime<Utc>,
}

#[derive(Serialize)]
struct ParticipantView {
    id: Uuid,
    email: String,
    name: Option<String>,
    status: &'static str,
    photos: i64,
    done: bool,
    collect_url: String,
    invited_at: Option<DateTime<Utc>>,
    last_email: Option<LastEmail>,
}

#[derive(Serialize, sqlx::FromRow)]
struct ReminderView {
    id: Uuid,
    days_before: i32,
    send_at: DateTime<Utc>,
    status: String,
    recipients: Option<i32>,
}

#[derive(Serialize, sqlx::FromRow)]
struct TeamView {
    id: Uuid,
    name: String,
    pin: String,
    created_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
    rounds_played: i64,
    total_score: i64,
}

pub async fn show(
    State(state): State<AppState>,
    organizer: Organizer,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let mut conn = state.db.acquire().await?;
    let game = owned_game(&mut conn, &organizer, id, false).await?;
    let progress = domain::game_progress(&mut conn, id).await?;

    let last_emails: Vec<LastEmailRow> = sqlx::query_as(
        "SELECT DISTINCT ON (participant_id) participant_id, kind, status, last_error, created_at
         FROM emails WHERE game_id = $1 AND participant_id IS NOT NULL
         ORDER BY participant_id, created_at DESC",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;

    let participants: Vec<ParticipantView> = progress
        .iter()
        .map(|p| ParticipantView {
            id: p.id,
            email: p.email.clone(),
            name: p.name.clone(),
            status: p.status(),
            photos: p.photo_count,
            done: p.done_at.is_some(),
            collect_url: state.config.url(&format!("/c/{}", p.token)),
            invited_at: p.invited_at,
            last_email: last_emails.iter().find(|e| e.0 == p.id).map(|e| LastEmail {
                kind: e.1.clone(),
                status: e.2.clone(),
                error: e.3.clone(),
                at: e.4,
            }),
        })
        .collect();

    let reminders = sqlx::query_as::<_, ReminderView>(
        "SELECT id, days_before, send_at, status, recipients FROM reminders WHERE game_id = $1 ORDER BY send_at",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;

    let teams = sqlx::query_as::<_, TeamView>(
        "SELECT t.id, t.name, t.pin, t.created_at, t.finished_at,
                count(g.id) AS rounds_played,
                coalesce(sum(g.year_score + g.location_score), 0)::bigint AS total_score
         FROM teams t LEFT JOIN guesses g ON g.team_id = t.id
         WHERE t.game_id = $1 GROUP BY t.id ORDER BY total_score DESC, t.created_at",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;

    let stats = domain::collection_stats(&mut conn, id).await?;
    let pending_emails: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM emails WHERE game_id = $1 AND status IN ('pending', 'sending')",
    )
    .bind(id)
    .fetch_one(&mut *conn)
    .await?;

    Ok(Json(json!({
        "game": game,
        "play_url": game.play_url(&state.config),
        "leaderboard_url": game.play_code.as_ref().map(|c| state.config.url(&format!("/play/{c}/leaderboard"))),
        "deadline_label": game.deadline_label(),
        "participants": participants,
        "reminders": reminders,
        "teams": teams,
        "stats": stats,
        "pending_emails": pending_emails,
        "max_participants": state.config.max_participants_per_game,
    })))
}

#[derive(Deserialize)]
pub struct UpdateGame {
    name: Option<String>,
    photos_per_participant: Option<i32>,
    message: Option<String>,
}

pub async fn update(
    State(state): State<AppState>,
    organizer: Organizer,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateGame>,
) -> AppResult<Json<Value>> {
    let mut tx = state.db.begin().await?;
    let game = owned_game(&mut tx, &organizer, id, true).await?;
    let name = match body.name {
        Some(n) => {
            let n = clean_line(&n, 100);
            if n.is_empty() {
                return Err(AppError::bad("The game name can't be empty."));
            }
            n
        }
        None => game.name.clone(),
    };
    let photos = body
        .photos_per_participant
        .unwrap_or(game.photos_per_participant);
    if !(1..=10).contains(&photos) {
        return Err(AppError::bad(
            "Photos per participant must be between 1 and 10.",
        ));
    }
    if photos != game.photos_per_participant
        && ![STATUS_DRAFT, STATUS_COLLECTING].contains(&game.status.as_str())
    {
        return Err(AppError::conflict(
            "The number of photos can't change once the game is ready.",
        ));
    }
    let message = match body.message {
        Some(m) => Some(clean_text(&m, 1000)).filter(|m| !m.is_empty()),
        None => game.message.clone(),
    };
    sqlx::query(
        "UPDATE games SET name = $2, photos_per_participant = $3, message = $4, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(name)
    .bind(photos)
    .bind(message)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn delete(
    State(state): State<AppState>,
    organizer: Organizer,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let result = sqlx::query("DELETE FROM games WHERE id = $1 AND organizer_id = $2")
        .bind(id)
        .bind(organizer.id)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct AddParticipants {
    entries: String,
}

pub async fn add_participants(
    State(state): State<AppState>,
    organizer: Organizer,
    Path(id): Path<Uuid>,
    Json(body): Json<AddParticipants>,
) -> AppResult<Json<Value>> {
    let (parsed, invalid) = parse_recipients(&body.entries);
    if parsed.is_empty() && invalid.is_empty() {
        return Err(AppError::bad("Enter at least one email address."));
    }
    let mut tx = state.db.begin().await?;
    let game = owned_game(&mut tx, &organizer, id, true).await?;
    if ![STATUS_DRAFT, STATUS_COLLECTING].contains(&game.status.as_str()) {
        return Err(AppError::conflict(
            "Participants can't be added once the game is ready.",
        ));
    }
    let existing: i64 = sqlx::query_scalar("SELECT count(*) FROM participants WHERE game_id = $1")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;

    let mut added = Vec::new();
    let mut duplicates = Vec::new();
    for recipient in parsed {
        if existing + added.len() as i64 >= state.config.max_participants_per_game {
            return Err(AppError::bad(format!(
                "A game can have at most {} participants.",
                state.config.max_participants_per_game
            )));
        }
        let inserted: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO participants (game_id, email, name, token) VALUES ($1, $2, $3, $4)
             ON CONFLICT (game_id, lower(email)) DO NOTHING RETURNING id",
        )
        .bind(id)
        .bind(&recipient.email)
        .bind(&recipient.name)
        .bind(crate::util::random_token(24))
        .fetch_optional(&mut *tx)
        .await?;
        match inserted {
            Some(pid) => added.push(pid),
            None => duplicates.push(recipient.email),
        }
    }

    if game.status == STATUS_COLLECTING && !added.is_empty() {
        mail::check_daily_cap(
            &mut tx,
            organizer.id,
            added.len() as i64,
            state.config.max_emails_per_organizer_per_day,
        )
        .await?;
        let sender = domain::load_sender(&mut tx, organizer.id).await?;
        let stats = domain::collection_stats(&mut tx, id).await?;
        for pid in &added {
            let progress = domain::participant_progress(&mut tx, *pid).await?;
            domain::enqueue_collection_email(
                &mut tx,
                &state.config,
                &game,
                &sender,
                &progress,
                stats,
                CollectionEmailKind::Invite,
            )
            .await?;
        }
    }
    // New participants mean the collection isn't complete anymore.
    if !added.is_empty() {
        sqlx::query(
            "UPDATE games SET all_ready_notified_at = NULL, updated_at = now() WHERE id = $1",
        )
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(Json(
        json!({ "added": added.len(), "duplicates": duplicates, "invalid": invalid }),
    ))
}

pub async fn remove_participant(
    State(state): State<AppState>,
    organizer: Organizer,
    Path((id, pid)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Value>> {
    let mut tx = state.db.begin().await?;
    let game = owned_game(&mut tx, &organizer, id, true).await?;
    if ![STATUS_DRAFT, STATUS_COLLECTING].contains(&game.status.as_str()) {
        return Err(AppError::conflict(
            "Participants can't be removed once the game is ready. Delete individual photos instead.",
        ));
    }
    let result = sqlx::query("DELETE FROM participants WHERE id = $1 AND game_id = $2")
        .bind(pid)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn resend_invite(
    State(state): State<AppState>,
    organizer: Organizer,
    Path((id, pid)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Value>> {
    let mut tx = state.db.begin().await?;
    let game = owned_game(&mut tx, &organizer, id, true).await?;
    if game.status != STATUS_COLLECTING {
        return Err(AppError::conflict(
            "Invitations can only be sent while collecting photos.",
        ));
    }
    let progress = domain::participant_progress(&mut tx, pid).await?;
    if progress.game_id != id {
        return Err(AppError::NotFound);
    }
    let recent: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM emails WHERE participant_id = $1 AND created_at > now() - interval '2 minutes'",
    )
    .bind(pid)
    .fetch_one(&mut *tx)
    .await?;
    if recent > 0 {
        return Err(AppError::TooManyRequests(
            "An email was just sent to this participant. Please wait a moment.".into(),
        ));
    }
    mail::check_daily_cap(
        &mut tx,
        organizer.id,
        1,
        state.config.max_emails_per_organizer_per_day,
    )
    .await?;
    let sender = domain::load_sender(&mut tx, organizer.id).await?;
    let stats = domain::collection_stats(&mut tx, id).await?;
    domain::enqueue_collection_email(
        &mut tx,
        &state.config,
        &game,
        &sender,
        &progress,
        stats,
        CollectionEmailKind::Invite,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct ScheduleBody {
    deadline_at: DateTime<Utc>,
    timezone: String,
    #[serde(default)]
    reminder_days: Vec<i32>,
}

fn validate_schedule(body: &ScheduleBody) -> AppResult<chrono_tz::Tz> {
    let tz =
        schedule::parse_tz(&body.timezone).ok_or_else(|| AppError::bad("Unknown time zone."))?;
    if body.deadline_at <= Utc::now() + Duration::minutes(10) {
        return Err(AppError::bad("The deadline must be in the future."));
    }
    if body.deadline_at > Utc::now() + Duration::days(365) {
        return Err(AppError::bad("The deadline must be within a year."));
    }
    if body
        .reminder_days
        .iter()
        .any(|d| !(0..=schedule::MAX_REMINDER_DAYS).contains(d))
    {
        return Err(AppError::bad("Reminders must be between D-0 and D-30."));
    }
    Ok(tz)
}

async fn insert_reminders(
    conn: &mut PgConnection,
    game_id: Uuid,
    deadline: DateTime<Utc>,
    tz: chrono_tz::Tz,
    days: &[i32],
    not_before: DateTime<Utc>,
) -> AppResult<()> {
    let already_sent: Vec<i32> = sqlx::query_scalar(
        "SELECT days_before FROM reminders WHERE game_id = $1 AND status = 'sent'",
    )
    .bind(game_id)
    .fetch_all(&mut *conn)
    .await?;
    for planned in schedule::plan_reminders(deadline, tz, days, not_before) {
        if planned.skipped && already_sent.contains(&planned.days_before) {
            continue;
        }
        sqlx::query(
            "INSERT INTO reminders (game_id, days_before, send_at, status) VALUES ($1, $2, $3, $4)",
        )
        .bind(game_id)
        .bind(planned.days_before)
        .bind(planned.send_at)
        .bind(if planned.skipped {
            "skipped"
        } else {
            "scheduled"
        })
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

pub async fn start_collecting(
    State(state): State<AppState>,
    organizer: Organizer,
    Path(id): Path<Uuid>,
    Json(body): Json<ScheduleBody>,
) -> AppResult<Json<Value>> {
    let tz = validate_schedule(&body)?;
    let mut tx = state.db.begin().await?;
    let game = owned_game(&mut tx, &organizer, id, true).await?;
    if game.status != STATUS_DRAFT {
        return Err(AppError::conflict("Photo collection has already started."));
    }
    let participants = domain::game_progress(&mut tx, id).await?;
    if participants.is_empty() {
        return Err(AppError::bad("Add at least one participant first."));
    }
    mail::check_daily_cap(
        &mut tx,
        organizer.id,
        participants.len() as i64,
        state.config.max_emails_per_organizer_per_day,
    )
    .await?;

    sqlx::query(
        "UPDATE games SET status = 'collecting', deadline_at = $2, timezone = $3, collection_started_at = now(),
                deadline_notified_at = NULL, updated_at = now()
         WHERE id = $1",
    )
    .bind(id)
    .bind(body.deadline_at)
    .bind(&body.timezone)
    .execute(&mut *tx)
    .await?;
    // Reminders that would land within 12h of the invitation are pointless: skip them.
    insert_reminders(
        &mut tx,
        id,
        body.deadline_at,
        tz,
        &body.reminder_days,
        Utc::now() + Duration::hours(12),
    )
    .await?;

    let game = domain::load_game(&mut tx, id).await?;
    let sender = domain::load_sender(&mut tx, organizer.id).await?;
    let stats = domain::collection_stats(&mut tx, id).await?;
    for p in &participants {
        domain::enqueue_collection_email(
            &mut tx,
            &state.config,
            &game,
            &sender,
            p,
            stats,
            CollectionEmailKind::Invite,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(json!({ "ok": true, "invited": participants.len() })))
}

pub async fn update_schedule(
    State(state): State<AppState>,
    organizer: Organizer,
    Path(id): Path<Uuid>,
    Json(body): Json<ScheduleBody>,
) -> AppResult<Json<Value>> {
    let tz = validate_schedule(&body)?;
    let mut tx = state.db.begin().await?;
    let game = owned_game(&mut tx, &organizer, id, true).await?;
    if game.status != STATUS_COLLECTING {
        return Err(AppError::conflict(
            "The schedule can only change while collecting photos.",
        ));
    }
    sqlx::query(
        "UPDATE games SET deadline_at = $2, timezone = $3, deadline_notified_at = NULL, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(body.deadline_at)
    .bind(&body.timezone)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM reminders WHERE game_id = $1 AND status IN ('scheduled', 'skipped')")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    insert_reminders(
        &mut tx,
        id,
        body.deadline_at,
        tz,
        &body.reminder_days,
        Utc::now(),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct ReadyBody {
    #[serde(default)]
    notify_participants: bool,
}

pub async fn mark_ready(
    State(state): State<AppState>,
    organizer: Organizer,
    Path(id): Path<Uuid>,
    Json(body): Json<ReadyBody>,
) -> AppResult<Json<Value>> {
    let mut tx = state.db.begin().await?;
    let game = owned_game(&mut tx, &organizer, id, true).await?;
    if game.status != STATUS_COLLECTING {
        return Err(AppError::conflict(
            "Only a game that is collecting photos can be marked as ready.",
        ));
    }
    let photos: i64 = sqlx::query_scalar("SELECT count(*) FROM photos WHERE game_id = $1")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    if photos == 0 {
        return Err(AppError::bad(
            "There are no photos yet: the game needs at least one photo.",
        ));
    }
    sqlx::query(
        "UPDATE games SET status = 'ready', ready_at = now(), play_code = COALESCE(play_code, $2), updated_at = now()
         WHERE id = $1",
    )
    .bind(id)
    .bind(random_code(10))
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE reminders SET status = 'skipped', processed_at = now() WHERE game_id = $1 AND status = 'scheduled'")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let game = domain::load_game(&mut tx, id).await?;
    let mut emailed = 0;
    if body.notify_participants {
        let sender = domain::load_sender(&mut tx, organizer.id).await?;
        emailed = domain::enqueue_play_emails(&mut tx, &state.config, &game, &sender).await?;
    }
    tx.commit().await?;
    Ok(Json(
        json!({ "ok": true, "play_url": game.play_url(&state.config), "emailed": emailed }),
    ))
}

pub async fn notify_play(
    State(state): State<AppState>,
    organizer: Organizer,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let mut tx = state.db.begin().await?;
    let game = owned_game(&mut tx, &organizer, id, true).await?;
    if game.status != STATUS_READY {
        return Err(AppError::conflict("The game isn't open for play."));
    }
    let recent: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM emails WHERE game_id = $1 AND kind = 'play' AND created_at > now() - interval '10 minutes'",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if recent > 0 {
        return Err(AppError::TooManyRequests(
            "The play link was emailed less than 10 minutes ago.".into(),
        ));
    }
    let sender = domain::load_sender(&mut tx, organizer.id).await?;
    let emailed = domain::enqueue_play_emails(&mut tx, &state.config, &game, &sender).await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true, "emailed": emailed })))
}

async fn transition(
    state: &AppState,
    organizer: &Organizer,
    id: Uuid,
    from: &str,
    to: &str,
    extra_check: Option<&str>,
) -> AppResult<()> {
    let mut tx = state.db.begin().await?;
    let game = owned_game(&mut tx, organizer, id, true).await?;
    if game.status != from {
        return Err(AppError::conflict(format!(
            "The game is {} (expected {}).",
            game.status, from
        )));
    }
    if extra_check == Some("no_teams") {
        let teams: i64 = sqlx::query_scalar("SELECT count(*) FROM teams WHERE game_id = $1")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
        if teams > 0 {
            return Err(AppError::conflict(
                "Teams have already started playing: delete them first to reopen the collection.",
            ));
        }
    }
    let timestamp = match to {
        STATUS_FINISHED => ", finished_at = now()",
        STATUS_READY => ", finished_at = NULL",
        _ => "",
    };
    sqlx::query(&format!(
        "UPDATE games SET status = $2, updated_at = now() {timestamp} WHERE id = $1"
    ))
    .bind(id)
    .bind(to)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn reopen(
    State(state): State<AppState>,
    organizer: Organizer,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    transition(
        &state,
        &organizer,
        id,
        STATUS_READY,
        STATUS_COLLECTING,
        Some("no_teams"),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn finish(
    State(state): State<AppState>,
    organizer: Organizer,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    transition(&state, &organizer, id, STATUS_READY, STATUS_FINISHED, None).await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn resume(
    State(state): State<AppState>,
    organizer: Organizer,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    transition(&state, &organizer, id, STATUS_FINISHED, STATUS_READY, None).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct JoiningBody {
    closed: bool,
}

pub async fn set_joining(
    State(state): State<AppState>,
    organizer: Organizer,
    Path(id): Path<Uuid>,
    Json(body): Json<JoiningBody>,
) -> AppResult<Json<Value>> {
    let result = sqlx::query("UPDATE games SET joining_closed = $3, updated_at = now() WHERE id = $1 AND organizer_id = $2")
        .bind(id)
        .bind(organizer.id)
        .bind(body.closed)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Serialize, sqlx::FromRow)]
pub struct OrganizerPhoto {
    id: Uuid,
    participant_id: Uuid,
    participant_name: Option<String>,
    participant_email: String,
    year: i32,
    lat: f64,
    lng: f64,
    caption: Option<String>,
    width: i32,
    height: i32,
    created_at: DateTime<Utc>,
}

pub async fn photos(
    State(state): State<AppState>,
    organizer: Organizer,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<OrganizerPhoto>>> {
    let mut conn = state.db.acquire().await?;
    owned_game(&mut conn, &organizer, id, false).await?;
    let photos = sqlx::query_as::<_, OrganizerPhoto>(
        "SELECT ph.id, ph.participant_id, p.name AS participant_name, p.email AS participant_email,
                ph.year, ph.lat, ph.lng, ph.caption, ph.width, ph.height, ph.created_at
         FROM photos ph JOIN participants p ON p.id = ph.participant_id
         WHERE ph.game_id = $1 ORDER BY ph.created_at",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(Json(photos))
}

pub async fn delete_photo(
    State(state): State<AppState>,
    organizer: Organizer,
    Path((id, photo_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Value>> {
    let mut tx = state.db.begin().await?;
    let game = owned_game(&mut tx, &organizer, id, true).await?;
    let participant_id: Option<Uuid> = sqlx::query_scalar(
        "DELETE FROM photos WHERE id = $1 AND game_id = $2 RETURNING participant_id",
    )
    .bind(photo_id)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(participant_id) = participant_id else {
        return Err(AppError::NotFound);
    };
    if game.status == STATUS_COLLECTING {
        domain::after_progress_change(&mut tx, &state.config, &game, participant_id).await?;
    }
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn delete_team(
    State(state): State<AppState>,
    organizer: Organizer,
    Path((id, team_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Value>> {
    let mut conn = state.db.acquire().await?;
    owned_game(&mut conn, &organizer, id, false).await?;
    let result = sqlx::query("DELETE FROM teams WHERE id = $1 AND game_id = $2")
        .bind(team_id)
        .bind(id)
        .execute(&mut *conn)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(Json(json!({ "ok": true })))
}

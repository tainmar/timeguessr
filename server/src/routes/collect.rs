//! Participant endpoints, authenticated by the personal token in the URL.

use axum::extract::{Multipart, Path, State};
use axum::Json;
use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::domain::{self, Game, Progress, STATUS_COLLECTING};
use crate::error::{AppError, AppResult};
use crate::images::{self, ImageProcessError, MAX_UPLOAD_BYTES};
use crate::notifications::{self, PhotoUploaded};
use crate::state::AppState;
use crate::util::{clean_line, opt_clean_line, validate_lat_lng};

pub const MIN_YEAR: i32 = 1850;

async fn participant_by_token(conn: &mut PgConnection, token: &str) -> AppResult<Progress> {
    Ok(sqlx::query_as::<_, Progress>(&format!(
        "SELECT {} FROM participant_progress WHERE token = $1",
        domain::PROGRESS_COLUMNS
    ))
    .bind(token)
    .fetch_one(conn)
    .await?)
}

#[derive(Serialize, sqlx::FromRow)]
pub struct PhotoView {
    id: Uuid,
    year: i32,
    lat: f64,
    lng: f64,
    caption: Option<String>,
    width: i32,
    height: i32,
    created_at: DateTime<Utc>,
}

const PHOTO_COLUMNS: &str = "id, year, lat, lng, caption, width, height, created_at";

pub async fn show(State(state): State<AppState>, Path(token): Path<String>) -> AppResult<Json<Value>> {
    let mut conn = state.db.acquire().await?;
    let participant = participant_by_token(&mut conn, &token).await?;
    let game = domain::load_game(&mut conn, participant.game_id).await?;
    let sender = domain::load_sender(&mut conn, game.organizer_id).await?;
    let stats = domain::collection_stats(&mut conn, game.id).await?;
    let photos = sqlx::query_as::<_, PhotoView>(&format!(
        "SELECT {PHOTO_COLUMNS} FROM photos WHERE participant_id = $1 ORDER BY created_at"
    ))
    .bind(participant.id)
    .fetch_all(&mut *conn)
    .await?;
    let open = game.status == STATUS_COLLECTING;
    Ok(Json(json!({
        "game": {
            "name": game.name,
            "status": game.status,
            "organizer": sender.display_name,
            "message": game.message,
            "photos_per_participant": game.photos_per_participant,
            "deadline_at": game.deadline_at,
            "deadline_label": game.deadline_label(),
            "deadline_passed": game.deadline_at.is_some_and(|d| d <= Utc::now()),
            "play_url": if open { None } else { game.play_url(&state.config) },
        },
        "participant": {
            "name": participant.name,
            "email": participant.email,
            "status": participant.status(),
            "done": participant.done_at.is_some(),
            "photo_count": participant.photo_count,
        },
        "photos": photos,
        "stats": stats,
        "can_edit": open,
        "can_upload": open && participant.photo_count < game.photos_per_participant as i64,
        "min_year": MIN_YEAR,
        "max_year": Utc::now().year(),
    })))
}

#[derive(Deserialize)]
pub struct UpdateParticipant {
    name: String,
}

pub async fn update(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Json(body): Json<UpdateParticipant>,
) -> AppResult<Json<Value>> {
    let name = clean_line(&body.name, 60);
    let result = sqlx::query("UPDATE participants SET name = NULLIF($2, '') WHERE token = $1")
        .bind(&token)
        .bind(name)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(Json(json!({ "ok": true })))
}

fn validate_year(year: i32) -> AppResult<i32> {
    let max = Utc::now().year();
    if !(MIN_YEAR..=max).contains(&year) {
        return Err(AppError::bad(format!("The year must be between {MIN_YEAR} and {max}.")));
    }
    Ok(year)
}

/// Lock the participant row and the game (shared) and make sure uploads are open.
async fn lock_for_edit(conn: &mut PgConnection, token: &str) -> AppResult<(Uuid, Game)> {
    let row: Option<(Uuid, Uuid)> =
        sqlx::query_as("SELECT id, game_id FROM participants WHERE token = $1 FOR UPDATE")
            .bind(token)
            .fetch_optional(&mut *conn)
            .await?;
    let (participant_id, game_id) = row.ok_or(AppError::NotFound)?;
    let game = domain::lock_game(conn, game_id, false).await?;
    if game.status != STATUS_COLLECTING {
        return Err(AppError::conflict(
            "Photo collection is closed: the organizer has started the game.",
        ));
    }
    Ok((participant_id, game))
}

pub async fn upload(
    State(state): State<AppState>,
    Path(token): Path<String>,
    mut multipart: Multipart,
) -> AppResult<Json<Value>> {
    // Cheap check before reading a potentially large body.
    {
        let mut conn = state.db.acquire().await?;
        let participant = participant_by_token(&mut conn, &token).await?;
        let game = domain::load_game(&mut conn, participant.game_id).await?;
        if game.status != STATUS_COLLECTING {
            return Err(AppError::conflict("Photo collection is closed: the organizer has started the game."));
        }
        if participant.photo_count >= game.photos_per_participant as i64 {
            return Err(AppError::conflict(format!(
                "You've already shared {} photo(s). Delete one to replace it.",
                game.photos_per_participant
            )));
        }
    }

    let mut file: Option<Vec<u8>> = None;
    let (mut year, mut lat, mut lng, mut caption) = (None, None, None, None);
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::bad(format!("Invalid upload: {e}")))?
    {
        let name = field.name().unwrap_or_default().to_string();
        if name == "file" {
            let bytes = field.bytes().await.map_err(|e| {
                if e.status() == axum::http::StatusCode::PAYLOAD_TOO_LARGE {
                    AppError::PayloadTooLarge("This photo is too big (20 MB max).".into())
                } else {
                    AppError::bad(format!("Upload interrupted: {e}"))
                }
            })?;
            if bytes.len() > MAX_UPLOAD_BYTES {
                return Err(AppError::PayloadTooLarge("This photo is too big (20 MB max).".into()));
            }
            file = Some(bytes.to_vec());
        } else {
            let value = field.text().await.map_err(|e| AppError::bad(e.to_string()))?;
            match name.as_str() {
                "year" => year = value.trim().parse::<i32>().ok(),
                "lat" => lat = value.trim().parse::<f64>().ok(),
                "lng" => lng = value.trim().parse::<f64>().ok(),
                "caption" => caption = Some(value),
                _ => {}
            }
        }
    }
    let file = file.ok_or_else(|| AppError::bad("Please choose a photo."))?;
    let year = validate_year(year.ok_or_else(|| AppError::bad("Please enter the year the photo was taken."))?)?;
    let (lat, lng) = match (lat, lng) {
        (Some(lat), Some(lng)) => validate_lat_lng(lat, lng).map_err(AppError::BadRequest)?,
        _ => return Err(AppError::bad("Please place the photo's location on the map.")),
    };
    let caption = opt_clean_line(caption.as_deref(), 200);

    let processed = {
        let _permit = state.decode_permits.acquire().await.expect("semaphore open");
        tokio::task::spawn_blocking(move || images::process(&file))
            .await
            .map_err(|e| AppError::Internal(e.into()))?
    }
    .map_err(|e| match e {
        ImageProcessError::Unsupported(m) => AppError::UnsupportedMedia(m),
        ImageProcessError::TooLarge => AppError::PayloadTooLarge(e.to_string()),
        ImageProcessError::Invalid(_) => AppError::BadRequest(e.to_string()),
    })?;

    let mut tx = state.db.begin().await?;
    let (participant_id, game) = lock_for_edit(&mut tx, &token).await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM photos WHERE participant_id = $1")
        .bind(participant_id)
        .fetch_one(&mut *tx)
        .await?;
    if count >= game.photos_per_participant as i64 {
        return Err(AppError::conflict(format!(
            "You've already shared {} photo(s). Delete one to replace it.",
            game.photos_per_participant
        )));
    }
    let photo = sqlx::query_as::<_, PhotoView>(&format!(
        "INSERT INTO photos (game_id, participant_id, year, lat, lng, caption, width, height, image, thumb)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING {PHOTO_COLUMNS}"
    ))
    .bind(game.id)
    .bind(participant_id)
    .bind(year)
    .bind(lat)
    .bind(lng)
    .bind(caption)
    .bind(processed.width as i32)
    .bind(processed.height as i32)
    .bind(processed.image)
    .bind(processed.thumb)
    .fetch_one(&mut *tx)
    .await?;

    let progress = domain::participant_progress(&mut tx, participant_id).await?;
    notifications::notify(
        &mut tx,
        &state.config,
        game.organizer_id,
        &PhotoUploaded {
            game_id: game.id,
            game_name: game.name.clone(),
            participant: progress.display_name(),
            uploaded: progress.photo_count,
            required: game.photos_per_participant,
        },
    )
    .await?;
    domain::after_progress_change(&mut tx, &state.config, &game, participant_id).await?;
    tx.commit().await?;
    Ok(Json(json!({ "photo": photo })))
}

#[derive(Deserialize)]
pub struct UpdatePhoto {
    year: Option<i32>,
    lat: Option<f64>,
    lng: Option<f64>,
    caption: Option<String>,
}

pub async fn update_photo(
    State(state): State<AppState>,
    Path((token, photo_id)): Path<(String, Uuid)>,
    Json(body): Json<UpdatePhoto>,
) -> AppResult<Json<Value>> {
    let mut tx = state.db.begin().await?;
    let (participant_id, _game) = lock_for_edit(&mut tx, &token).await?;
    let current = sqlx::query_as::<_, PhotoView>(&format!(
        "SELECT {PHOTO_COLUMNS} FROM photos WHERE id = $1 AND participant_id = $2"
    ))
    .bind(photo_id)
    .bind(participant_id)
    .fetch_one(&mut *tx)
    .await?;
    let year = validate_year(body.year.unwrap_or(current.year))?;
    let (lat, lng) = validate_lat_lng(body.lat.unwrap_or(current.lat), body.lng.unwrap_or(current.lng))
        .map_err(AppError::BadRequest)?;
    let caption = match body.caption {
        Some(c) => opt_clean_line(Some(&c), 200),
        None => current.caption,
    };
    let photo = sqlx::query_as::<_, PhotoView>(&format!(
        "UPDATE photos SET year = $2, lat = $3, lng = $4, caption = $5 WHERE id = $1 RETURNING {PHOTO_COLUMNS}"
    ))
    .bind(photo_id)
    .bind(year)
    .bind(lat)
    .bind(lng)
    .bind(caption)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "photo": photo })))
}

pub async fn delete_photo(
    State(state): State<AppState>,
    Path((token, photo_id)): Path<(String, Uuid)>,
) -> AppResult<Json<Value>> {
    let mut tx = state.db.begin().await?;
    let (participant_id, game) = lock_for_edit(&mut tx, &token).await?;
    let result = sqlx::query("DELETE FROM photos WHERE id = $1 AND participant_id = $2")
        .bind(photo_id)
        .bind(participant_id)
        .execute(&mut *tx)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    domain::after_progress_change(&mut tx, &state.config, &game, participant_id).await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

async fn set_done(state: &AppState, token: &str, done: bool) -> AppResult<Json<Value>> {
    let mut tx = state.db.begin().await?;
    let (participant_id, game) = lock_for_edit(&mut tx, token).await?;
    sqlx::query(
        "UPDATE participants SET done_at = CASE WHEN $2 THEN COALESCE(done_at, now()) ELSE NULL END WHERE id = $1",
    )
    .bind(participant_id)
    .bind(done)
    .execute(&mut *tx)
    .await?;
    domain::after_progress_change(&mut tx, &state.config, &game, participant_id).await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn done(State(state): State<AppState>, Path(token): Path<String>) -> AppResult<Json<Value>> {
    set_done(&state, &token, true).await
}

pub async fn undone(State(state): State<AppState>, Path(token): Path<String>) -> AppResult<Json<Value>> {
    set_done(&state, &token, false).await
}

//! Player endpoints: join with a team name, play rounds, leaderboard.
//! Answers (year, location, owner, caption) are only ever returned after the team
//! has guessed that photo, or on the leaderboard once the team or the game is finished.

use std::collections::{HashMap, HashSet};

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::{DateTime, Datelike, Utc};
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::domain::{self, Game, STATUS_FINISHED, STATUS_READY};
use crate::error::{AppError, AppResult};
use crate::notifications::{self, TeamFinished};
use crate::scoring;
use crate::state::AppState;
use crate::util::{clean_line, random_pin, random_token, validate_lat_lng};

const TEAM_HEADER: &str = "x-team-token";
const YEAR_MARGIN: i32 = 5;

async fn game_by_code(conn: &mut PgConnection, code: &str) -> AppResult<Game> {
    let game = sqlx::query_as::<_, Game>(&format!(
        "SELECT {} FROM games WHERE play_code = $1",
        domain::GAME_COLUMNS
    ))
    .bind(code.to_lowercase())
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(AppError::NotFound)?;
    if game.status != STATUS_READY && game.status != STATUS_FINISHED {
        return Err(AppError::NotFound);
    }
    Ok(game)
}

/// Slider range: oldest photo minus 5 years up to the current year.
async fn year_range(conn: &mut PgConnection, game_id: Uuid) -> AppResult<(i32, i32)> {
    let min: Option<i32> = sqlx::query_scalar("SELECT min(year) FROM photos WHERE game_id = $1")
        .bind(game_id)
        .fetch_one(conn)
        .await?;
    let max = Utc::now().year();
    Ok((min.unwrap_or(max) - YEAR_MARGIN, max))
}

pub async fn show(
    State(state): State<AppState>,
    Path(code): Path<String>,
) -> AppResult<Json<Value>> {
    let mut conn = state.db.acquire().await?;
    let game = game_by_code(&mut conn, &code).await?;
    let sender = domain::load_sender(&mut conn, game.organizer_id).await?;
    let photos: i64 = sqlx::query_scalar("SELECT count(*) FROM photos WHERE game_id = $1")
        .bind(game.id)
        .fetch_one(&mut *conn)
        .await?;
    let teams: i64 = sqlx::query_scalar("SELECT count(*) FROM teams WHERE game_id = $1")
        .bind(game.id)
        .fetch_one(&mut *conn)
        .await?;
    let (year_min, year_max) = year_range(&mut conn, game.id).await?;
    Ok(Json(json!({
        "name": game.name,
        "status": game.status,
        "organizer": sender.display_name,
        "photo_count": photos,
        "team_count": teams,
        "year_min": year_min,
        "year_max": year_max,
        "joining_open": game.status == STATUS_READY && !game.joining_closed,
    })))
}

#[derive(sqlx::FromRow)]
struct Team {
    id: Uuid,
    game_id: Uuid,
    name: String,
    token: String,
    pin: String,
    photo_order: Vec<Uuid>,
    finished_at: Option<DateTime<Utc>>,
}

const TEAM_COLUMNS: &str = "id, game_id, name, token, pin, photo_order, finished_at";

#[derive(Deserialize)]
pub struct JoinBody {
    name: String,
    pin: Option<String>,
}

pub async fn join(
    State(state): State<AppState>,
    Path(code): Path<String>,
    Json(body): Json<JoinBody>,
) -> AppResult<Json<Value>> {
    let name = clean_line(&body.name, 40);
    if name.chars().count() < 2 {
        return Err(AppError::bad("Team names need at least 2 characters."));
    }
    let mut tx = state.db.begin().await?;
    let game = game_by_code(&mut tx, &code).await?;

    let existing = sqlx::query_as::<_, Team>(&format!(
        "SELECT {TEAM_COLUMNS} FROM teams WHERE game_id = $1 AND lower(name) = lower($2)"
    ))
    .bind(game.id)
    .bind(&name)
    .fetch_optional(&mut *tx)
    .await?;

    if let Some(team) = existing {
        // Rejoin from another device (or after closing the tab) with the team PIN.
        let Some(pin) = body.pin.as_deref().map(str::trim).filter(|p| !p.is_empty()) else {
            return Err(AppError::conflict(
                "This team name is taken. If it's your team, enter its 4-digit PIN to join from this device.",
            ));
        };
        if state.pin_guard.is_locked(team.id) {
            return Err(AppError::TooManyRequests(
                "Too many wrong PINs. Try again in 15 minutes.".into(),
            ));
        }
        if pin != team.pin {
            state.pin_guard.record_failure(team.id);
            return Err(AppError::Forbidden("Wrong PIN for this team.".into()));
        }
        state.pin_guard.clear(team.id);
        return Ok(Json(
            json!({ "token": team.token, "team": { "id": team.id, "name": team.name, "pin": team.pin } }),
        ));
    }

    if game.status != STATUS_READY {
        return Err(AppError::conflict("This game has ended."));
    }
    if game.joining_closed {
        return Err(AppError::conflict(
            "The organizer has closed joining for new teams.",
        ));
    }
    let teams: i64 = sqlx::query_scalar("SELECT count(*) FROM teams WHERE game_id = $1")
        .bind(game.id)
        .fetch_one(&mut *tx)
        .await?;
    if teams >= 500 {
        return Err(AppError::conflict("This game is full."));
    }

    let mut order: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM photos WHERE game_id = $1")
        .bind(game.id)
        .fetch_all(&mut *tx)
        .await?;
    order.shuffle(&mut rand::rng());
    let token = random_token(24);
    let pin = random_pin();
    let team_id: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO teams (game_id, name, token, pin, photo_order) VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (game_id, lower(name)) DO NOTHING RETURNING id",
    )
    .bind(game.id)
    .bind(&name)
    .bind(&token)
    .bind(&pin)
    .bind(&order)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(team_id) = team_id else {
        return Err(AppError::conflict(
            "This team name was just taken. Pick another one.",
        ));
    };
    tx.commit().await?;
    Ok(Json(
        json!({ "token": token, "team": { "id": team_id, "name": name, "pin": pin } }),
    ))
}

fn team_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(TEAM_HEADER)
        .and_then(|v| v.to_str().ok())
        .filter(|t| !t.is_empty())
}

async fn team_for(
    conn: &mut PgConnection,
    game: &Game,
    headers: &HeaderMap,
    lock: bool,
) -> AppResult<Team> {
    let token = team_token(headers)
        .ok_or_else(|| AppError::Forbidden("Join the game with a team name first.".into()))?;
    let team = sqlx::query_as::<_, Team>(&format!(
        "SELECT {TEAM_COLUMNS} FROM teams WHERE token = $1 {}",
        if lock { "FOR UPDATE" } else { "" }
    ))
    .bind(token)
    .fetch_optional(conn)
    .await?
    .filter(|t| t.game_id == game.id)
    .ok_or_else(|| {
        AppError::Forbidden(
            "Your team was not found. Join again with your team name and PIN.".into(),
        )
    })?;
    Ok(team)
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct GuessRow {
    photo_id: Uuid,
    year: i32,
    lat: f64,
    lng: f64,
    distance_km: f64,
    year_score: i32,
    location_score: i32,
}

/// The team's rounds: photo order restricted to photos that still exist.
struct Progression {
    order: Vec<Uuid>,
    guesses: HashMap<Uuid, GuessRow>,
}

impl Progression {
    async fn load(conn: &mut PgConnection, team: &Team) -> AppResult<Self> {
        let existing: HashSet<Uuid> =
            sqlx::query_scalar("SELECT id FROM photos WHERE game_id = $1")
                .bind(team.game_id)
                .fetch_all(&mut *conn)
                .await?
                .into_iter()
                .collect();
        let order = team
            .photo_order
            .iter()
            .copied()
            .filter(|id| existing.contains(id))
            .collect();
        let guesses = sqlx::query_as::<_, GuessRow>(
            "SELECT photo_id, year, lat, lng, distance_km, year_score, location_score FROM guesses WHERE team_id = $1",
        )
        .bind(team.id)
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(|g| (g.photo_id, g))
        .collect();
        Ok(Self { order, guesses })
    }

    fn unanswered(&self) -> impl Iterator<Item = &Uuid> {
        self.order
            .iter()
            .filter(|id| !self.guesses.contains_key(id))
    }

    fn current(&self) -> Option<Uuid> {
        self.unanswered().next().copied()
    }

    fn played(&self) -> usize {
        self.order
            .iter()
            .filter(|id| self.guesses.contains_key(id))
            .count()
    }

    fn total_score(&self) -> i64 {
        self.order
            .iter()
            .filter_map(|id| self.guesses.get(id))
            .map(|g| (g.year_score + g.location_score) as i64)
            .sum()
    }
}

fn image_url(id: Uuid) -> String {
    format!("/api/images/{id}")
}

fn thumb_url(id: Uuid) -> String {
    format!("/api/images/{id}/thumb")
}

pub async fn state(
    State(state): State<AppState>,
    Path(code): Path<String>,
    headers: HeaderMap,
) -> AppResult<Json<Value>> {
    let mut conn = state.db.acquire().await?;
    let game = game_by_code(&mut conn, &code).await?;
    let team = team_for(&mut conn, &game, &headers, false).await?;
    let progression = Progression::load(&mut conn, &team).await?;
    let (year_min, year_max) = year_range(&mut conn, game.id).await?;
    let mut upcoming = progression.unanswered();
    let current = upcoming.next().copied();
    let next = upcoming.next().copied();
    if current.is_none() && team.finished_at.is_none() {
        sqlx::query("UPDATE teams SET finished_at = now() WHERE id = $1 AND finished_at IS NULL")
            .bind(team.id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(Json(json!({
        "team": { "id": team.id, "name": team.name, "pin": team.pin },
        "game": { "name": game.name, "status": game.status, "year_min": year_min, "year_max": year_max },
        "round": current.map(|id| json!({
            "number": progression.played() + 1,
            "total": progression.order.len(),
            "photo_id": id,
            "image_url": image_url(id),
        })),
        "next_image_url": next.map(image_url),
        "rounds_played": progression.played(),
        "total_rounds": progression.order.len(),
        "total_score": progression.total_score(),
        "finished": current.is_none(),
    })))
}

#[derive(Deserialize)]
pub struct GuessBody {
    photo_id: Uuid,
    year: i32,
    lat: f64,
    lng: f64,
}

#[derive(sqlx::FromRow)]
struct Answer {
    year: i32,
    lat: f64,
    lng: f64,
    caption: Option<String>,
    owner: Option<String>,
}

#[derive(Serialize)]
struct AnswerView {
    year: i32,
    lat: f64,
    lng: f64,
    caption: Option<String>,
    owner: String,
}

impl From<Answer> for AnswerView {
    fn from(a: Answer) -> Self {
        Self {
            year: a.year,
            lat: a.lat,
            lng: a.lng,
            caption: a.caption,
            owner: a
                .owner
                .filter(|o| !o.is_empty())
                .unwrap_or_else(|| "A mystery guest".into()),
        }
    }
}

async fn load_answer(conn: &mut PgConnection, photo_id: Uuid) -> AppResult<Answer> {
    Ok(sqlx::query_as::<_, Answer>(
        "SELECT ph.year, ph.lat, ph.lng, ph.caption, p.name AS owner
         FROM photos ph JOIN participants p ON p.id = ph.participant_id WHERE ph.id = $1",
    )
    .bind(photo_id)
    .fetch_one(conn)
    .await?)
}

pub async fn guess(
    State(state): State<AppState>,
    Path(code): Path<String>,
    headers: HeaderMap,
    Json(body): Json<GuessBody>,
) -> AppResult<Json<Value>> {
    let (lat, lng) = validate_lat_lng(body.lat, body.lng).map_err(AppError::BadRequest)?;
    if !(1000..=3000).contains(&body.year) {
        return Err(AppError::bad("Invalid year."));
    }
    let mut tx = state.db.begin().await?;
    let game = game_by_code(&mut tx, &code).await?;
    // Serialize submissions from several devices of the same team.
    let team = team_for(&mut tx, &game, &headers, true).await?;
    let mut progression = Progression::load(&mut tx, &team).await?;

    if !progression.guesses.contains_key(&body.photo_id) {
        if game.status != STATUS_READY {
            return Err(AppError::conflict("The organizer has ended the game."));
        }
        if progression.current() != Some(body.photo_id) {
            return Err(AppError::conflict(
                "This photo isn't your current round. Refresh the page.",
            ));
        }
        let answer = load_answer(&mut tx, body.photo_id).await?;
        let distance = scoring::distance_km(lat, lng, answer.lat, answer.lng);
        let row = GuessRow {
            photo_id: body.photo_id,
            year: body.year,
            lat,
            lng,
            distance_km: distance,
            year_score: scoring::year_score(body.year, answer.year),
            location_score: scoring::location_score(distance),
        };
        sqlx::query(
            "INSERT INTO guesses (team_id, photo_id, year, lat, lng, distance_km, year_score, location_score)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) ON CONFLICT (team_id, photo_id) DO NOTHING",
        )
        .bind(team.id)
        .bind(row.photo_id)
        .bind(row.year)
        .bind(row.lat)
        .bind(row.lng)
        .bind(row.distance_km)
        .bind(row.year_score)
        .bind(row.location_score)
        .execute(&mut *tx)
        .await?;
        progression.guesses.insert(body.photo_id, row);

        if progression.current().is_none() {
            let first_finish: Option<Uuid> = sqlx::query_scalar(
                "UPDATE teams SET finished_at = now() WHERE id = $1 AND finished_at IS NULL RETURNING id",
            )
            .bind(team.id)
            .fetch_optional(&mut *tx)
            .await?;
            if first_finish.is_some() {
                notifications::notify(
                    &mut tx,
                    &state.config,
                    game.organizer_id,
                    &TeamFinished {
                        game_id: game.id,
                        game_name: game.name.clone(),
                        team: team.name.clone(),
                        score: progression.total_score(),
                    },
                )
                .await?;
            }
        }
    } else if !progression.order.contains(&body.photo_id) {
        return Err(AppError::NotFound);
    }

    let answer: AnswerView = load_answer(&mut tx, body.photo_id).await?.into();
    tx.commit().await?;
    let g = &progression.guesses[&body.photo_id];
    let round_number = progression
        .order
        .iter()
        .position(|id| *id == body.photo_id)
        .map(|i| i + 1);
    Ok(Json(json!({
        "photo_id": body.photo_id,
        "image_url": image_url(body.photo_id),
        "guess": { "year": g.year, "lat": g.lat, "lng": g.lng },
        "answer": answer,
        "distance_km": g.distance_km,
        "year_score": g.year_score,
        "location_score": g.location_score,
        "score": g.year_score + g.location_score,
        "total_score": progression.total_score(),
        "round_number": round_number,
        "total_rounds": progression.order.len(),
        "finished": progression.current().is_none(),
    })))
}

#[derive(sqlx::FromRow)]
struct LeaderboardGuess {
    team_id: Uuid,
    team_name: String,
    photo_id: Uuid,
    year: i32,
    lat: f64,
    lng: f64,
    distance_km: f64,
    year_score: i32,
    location_score: i32,
}

#[derive(sqlx::FromRow)]
struct LeaderboardTeam {
    id: Uuid,
    name: String,
    photo_order: Vec<Uuid>,
    finished_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct LeaderboardPhoto {
    id: Uuid,
    year: i32,
    lat: f64,
    lng: f64,
    caption: Option<String>,
    owner: Option<String>,
}

pub async fn leaderboard(
    State(state): State<AppState>,
    Path(code): Path<String>,
    headers: HeaderMap,
) -> AppResult<Json<Value>> {
    let mut conn = state.db.acquire().await?;
    let game = game_by_code(&mut conn, &code).await?;
    let viewer = match team_token(&headers) {
        Some(_) => team_for(&mut conn, &game, &headers, false).await.ok(),
        None => None,
    };

    let photos = sqlx::query_as::<_, LeaderboardPhoto>(
        "SELECT ph.id, ph.year, ph.lat, ph.lng, ph.caption, p.name AS owner
         FROM photos ph JOIN participants p ON p.id = ph.participant_id
         WHERE ph.game_id = $1 ORDER BY ph.created_at, ph.id",
    )
    .bind(game.id)
    .fetch_all(&mut *conn)
    .await?;
    let photo_ids: HashSet<Uuid> = photos.iter().map(|p| p.id).collect();
    let teams = sqlx::query_as::<_, LeaderboardTeam>(
        "SELECT id, name, photo_order, finished_at, created_at FROM teams WHERE game_id = $1",
    )
    .bind(game.id)
    .fetch_all(&mut *conn)
    .await?;
    let guesses = sqlx::query_as::<_, LeaderboardGuess>(
        "SELECT g.team_id, t.name AS team_name, g.photo_id, g.year, g.lat, g.lng, g.distance_km, g.year_score, g.location_score
         FROM guesses g JOIN teams t ON t.id = g.team_id WHERE t.game_id = $1",
    )
    .bind(game.id)
    .fetch_all(&mut *conn)
    .await?;

    let viewer_finished = match &viewer {
        Some(team) => Progression::load(&mut conn, team)
            .await?
            .current()
            .is_none(),
        None => false,
    };
    let reveal = game.status == STATUS_FINISHED || viewer_finished;

    let mut rows: Vec<Value> = teams
        .iter()
        .map(|team| {
            let mine: Vec<&LeaderboardGuess> = guesses
                .iter()
                .filter(|g| g.team_id == team.id && photo_ids.contains(&g.photo_id))
                .collect();
            let total: i64 = mine.iter().map(|g| (g.year_score + g.location_score) as i64).sum();
            let scores: serde_json::Map<String, Value> = mine
                .iter()
                .map(|g| {
                    (
                        g.photo_id.to_string(),
                        json!({ "year_score": g.year_score, "location_score": g.location_score, "score": g.year_score + g.location_score }),
                    )
                })
                .collect();
            let rounds_total = team.photo_order.iter().filter(|id| photo_ids.contains(id)).count();
            json!({
                "id": team.id,
                "name": team.name,
                "total": total,
                "rounds_played": mine.len(),
                "rounds_total": rounds_total,
                "finished": mine.len() >= rounds_total,
                "finished_at": team.finished_at,
                "created_at": team.created_at,
                "scores": scores,
            })
        })
        .collect();
    rows.sort_by(|a, b| {
        b["total"].as_i64().cmp(&a["total"].as_i64()).then_with(|| {
            a["finished_at"]
                .as_str()
                .unwrap_or("~")
                .cmp(b["finished_at"].as_str().unwrap_or("~"))
        })
    });

    let photo_views: Vec<Value> = photos
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut view = json!({
                "id": p.id,
                "number": i + 1,
                "image_url": image_url(p.id),
                "thumb_url": thumb_url(p.id),
            });
            if reveal {
                view["answer"] = json!({
                    "year": p.year,
                    "lat": p.lat,
                    "lng": p.lng,
                    "caption": p.caption,
                    "owner": p.owner.clone().filter(|o| !o.is_empty()).unwrap_or_else(|| "A mystery guest".into()),
                });
                view["guesses"] = guesses
                    .iter()
                    .filter(|g| g.photo_id == p.id)
                    .map(|g| {
                        json!({
                            "team_id": g.team_id,
                            "team_name": g.team_name,
                            "year": g.year,
                            "lat": g.lat,
                            "lng": g.lng,
                            "distance_km": g.distance_km,
                            "score": g.year_score + g.location_score,
                        })
                    })
                    .collect();
            }
            view
        })
        .collect();

    Ok(Json(json!({
        "game": { "name": game.name, "status": game.status },
        "photos": photo_views,
        "teams": rows,
        "reveal_answers": reveal,
        "viewer_team_id": viewer.map(|t| t.id),
        "generated_at": Utc::now(),
    })))
}

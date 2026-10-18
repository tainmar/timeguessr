use axum::extract::State;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::auth::Organizer;
use crate::error::AppResult;
use crate::state::AppState;

#[derive(Serialize, sqlx::FromRow)]
pub struct NotificationView {
    id: Uuid,
    game_id: Option<Uuid>,
    kind: String,
    title: String,
    body: String,
    link: Option<String>,
    read_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

pub async fn list(State(state): State<AppState>, organizer: Organizer) -> AppResult<Json<Value>> {
    let items = sqlx::query_as::<_, NotificationView>(
        "SELECT id, game_id, kind, title, body, link, read_at, created_at
         FROM notifications WHERE organizer_id = $1 ORDER BY created_at DESC LIMIT 50",
    )
    .bind(organizer.id)
    .fetch_all(&state.db)
    .await?;
    let unread: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM notifications WHERE organizer_id = $1 AND read_at IS NULL",
    )
    .bind(organizer.id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(json!({ "items": items, "unread": unread })))
}

#[derive(Deserialize)]
pub struct MarkRead {
    #[serde(default)]
    ids: Vec<Uuid>,
    #[serde(default)]
    all: bool,
}

pub async fn mark_read(
    State(state): State<AppState>,
    organizer: Organizer,
    Json(body): Json<MarkRead>,
) -> AppResult<Json<Value>> {
    sqlx::query(
        "UPDATE notifications SET read_at = now()
         WHERE organizer_id = $1 AND read_at IS NULL AND ($2 OR id = ANY($3))",
    )
    .bind(organizer.id)
    .bind(body.all)
    .bind(&body.ids)
    .execute(&state.db)
    .await?;
    Ok(Json(json!({ "ok": true })))
}

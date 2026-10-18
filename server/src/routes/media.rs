use axum::extract::{Path, State};
use axum::http::header;
use axum::response::IntoResponse;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

async fn serve(state: &AppState, id: Uuid, column: &str) -> AppResult<impl IntoResponse> {
    let bytes: Vec<u8> = sqlx::query_scalar(&format!("SELECT {column} FROM photos WHERE id = $1"))
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok((
        [
            (header::CONTENT_TYPE, "image/jpeg"),
            // Ids are random and the bytes never change for a given id.
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        bytes,
    ))
}

pub async fn full(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    serve(&state, id, "image").await
}

pub async fn thumb(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    serve(&state, id, "thumb").await
}

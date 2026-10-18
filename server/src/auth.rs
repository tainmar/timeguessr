//! Organizer authentication: passwordless magic links + session cookie.

use axum::extract::{FromRef, FromRequestParts, OptionalFromRequestParts};
use axum::http::request::Parts;
use axum::http::HeaderMap;
use axum_extra::extract::cookie::CookieJar;
use serde::Serialize;
use uuid::Uuid;

use crate::error::AppError;
use crate::state::AppState;
use crate::util::sha256_hex;

pub const SESSION_COOKIE: &str = "tg_session";
pub const SESSION_DAYS: i64 = 30;
pub const LOGIN_TOKEN_MINUTES: i64 = 30;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Organizer {
    pub id: Uuid,
    pub email: String,
    pub name: Option<String>,
    pub email_notifications: String,
}

async fn organizer_from_parts(
    parts: &Parts,
    state: &AppState,
) -> Result<Option<Organizer>, AppError> {
    let jar = CookieJar::from_headers(&parts.headers);
    let Some(cookie) = jar.get(SESSION_COOKIE) else {
        return Ok(None);
    };
    let organizer = sqlx::query_as::<_, Organizer>(
        "SELECT o.id, o.email, o.name, o.email_notifications
         FROM sessions s JOIN organizers o ON o.id = s.organizer_id
         WHERE s.token_hash = $1 AND s.expires_at > now()",
    )
    .bind(sha256_hex(cookie.value()))
    .fetch_optional(&state.db)
    .await?;
    Ok(organizer)
}

impl<S> FromRequestParts<S> for Organizer
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let state = AppState::from_ref(state);
        organizer_from_parts(parts, &state)
            .await?
            .ok_or(AppError::Unauthorized)
    }
}

impl<S> OptionalFromRequestParts<S> for Organizer
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> Result<Option<Self>, Self::Rejection> {
        let state = AppState::from_ref(state);
        organizer_from_parts(parts, &state).await
    }
}

/// Best-effort client IP behind Railway's proxy (which appends to X-Forwarded-For).
pub fn client_ip(headers: &HeaderMap) -> String {
    if let Some(ip) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
        return ip.trim().to_string();
    }
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit(',').next())
        .map(|v| v.trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}

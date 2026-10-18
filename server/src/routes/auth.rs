use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use chrono::{Duration, Utc};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{self, client_ip, Organizer, LOGIN_TOKEN_MINUTES, SESSION_COOKIE, SESSION_DAYS};
use crate::error::{AppError, AppResult};
use crate::mail::{self, templates, OutgoingEmail};
use crate::state::AppState;
use crate::util::{clean_line, is_valid_email, random_token, sha256_hex};

#[derive(Deserialize)]
pub struct RequestLink {
    email: String,
}

pub async fn request_link(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RequestLink>,
) -> AppResult<Json<Value>> {
    let email = body.email.trim().to_lowercase();
    if !is_valid_email(&email) {
        return Err(AppError::bad("Please enter a valid email address."));
    }
    if !state.config.organizer_allowed(&email) {
        return Err(AppError::Forbidden(
            "This TimeGuessr instance only accepts invited organizers.".into(),
        ));
    }
    let ip = client_ip(&headers);
    let mut tx = state.db.begin().await?;
    let (per_email, per_ip): (i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE lower(email) = $1), count(*) FILTER (WHERE ip = $2)
         FROM login_tokens WHERE created_at > now() - interval '1 hour'",
    )
    .bind(&email)
    .bind(&ip)
    .fetch_one(&mut *tx)
    .await?;
    if per_email >= 5 || per_ip >= 20 {
        return Err(AppError::TooManyRequests(
            "Too many sign-in emails requested. Please wait a bit and check your inbox (and spam folder).".into(),
        ));
    }

    let token = random_token(32);
    sqlx::query("INSERT INTO login_tokens (email, token_hash, ip, expires_at) VALUES ($1, $2, $3, $4)")
        .bind(&email)
        .bind(sha256_hex(&token))
        .bind(&ip)
        .bind(Utc::now() + Duration::minutes(LOGIN_TOKEN_MINUTES))
        .execute(&mut *tx)
        .await?;
    // The token travels in the URL fragment: never sent to servers or logged by proxies.
    let url = state.config.url(&format!("/auth/verify#token={token}"));
    let rendered = templates::login(&url);
    mail::enqueue(
        &mut tx,
        &OutgoingEmail {
            kind: "login",
            to: email,
            subject: rendered.subject,
            html: rendered.html,
            text: rendered.text,
            reply_to: None,
            organizer_id: None,
            game_id: None,
            participant_id: None,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct Verify {
    token: String,
}

pub async fn verify(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<Verify>,
) -> AppResult<(CookieJar, Json<Organizer>)> {
    let mut tx = state.db.begin().await?;
    let email: Option<String> = sqlx::query_scalar(
        "UPDATE login_tokens SET used_at = now()
         WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now()
         RETURNING email",
    )
    .bind(sha256_hex(body.token.trim()))
    .fetch_optional(&mut *tx)
    .await?;
    let Some(email) = email else {
        return Err(AppError::bad(
            "This sign-in link is invalid or has expired. Please request a new one.",
        ));
    };

    let organizer = sqlx::query_as::<_, Organizer>(
        "INSERT INTO organizers (email) VALUES ($1)
         ON CONFLICT (lower(email)) DO UPDATE SET email = organizers.email
         RETURNING id, email, name, email_notifications",
    )
    .bind(&email)
    .fetch_one(&mut *tx)
    .await?;

    let session_token = random_token(32);
    sqlx::query("INSERT INTO sessions (organizer_id, token_hash, expires_at) VALUES ($1, $2, $3)")
        .bind(organizer.id)
        .bind(sha256_hex(&session_token))
        .bind(Utc::now() + Duration::days(SESSION_DAYS))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    let cookie = Cookie::build((SESSION_COOKIE, session_token))
        .path("/")
        .http_only(true)
        .secure(state.config.secure_cookies())
        .same_site(SameSite::Lax)
        .max_age(time_days(SESSION_DAYS))
        .build();
    Ok((jar.add(cookie), Json(organizer)))
}

fn time_days(days: i64) -> time::Duration {
    time::Duration::days(days)
}

pub async fn logout(State(state): State<AppState>, jar: CookieJar) -> AppResult<(CookieJar, Json<Value>)> {
    if let Some(cookie) = jar.get(SESSION_COOKIE) {
        sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
            .bind(sha256_hex(cookie.value()))
            .execute(&state.db)
            .await?;
    }
    let removal = Cookie::build((SESSION_COOKIE, "")).path("/").build();
    Ok((jar.remove(removal), Json(json!({ "ok": true }))))
}

pub async fn me(organizer: Option<Organizer>) -> Json<Value> {
    Json(json!({ "organizer": organizer }))
}

#[derive(Deserialize)]
pub struct UpdateMe {
    name: Option<String>,
    email_notifications: Option<String>,
}

pub async fn update_me(
    State(state): State<AppState>,
    organizer: auth::Organizer,
    Json(body): Json<UpdateMe>,
) -> AppResult<Json<Organizer>> {
    if let Some(pref) = &body.email_notifications {
        if !["all", "important", "none"].contains(&pref.as_str()) {
            return Err(AppError::bad("Invalid notification preference."));
        }
    }
    let name = body.name.as_deref().map(|n| clean_line(n, 80));
    let updated = sqlx::query_as::<_, Organizer>(
        "UPDATE organizers SET
            name = CASE WHEN $2 THEN NULLIF($3, '') ELSE name END,
            email_notifications = COALESCE($4, email_notifications)
         WHERE id = $1
         RETURNING id, email, name, email_notifications",
    )
    .bind(organizer.id)
    .bind(name.is_some())
    .bind(name.unwrap_or_default())
    .bind(body.email_notifications)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(updated))
}

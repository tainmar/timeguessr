mod auth;
mod collect;
mod games;
mod media;
mod notifications;
mod play;

use std::path::PathBuf;

use axum::extract::{DefaultBodyLimit, MatchedPath, Request};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde_json::json;
use tower_http::compression::CompressionLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::images::MAX_UPLOAD_BYTES;
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/health", get(health))
        .route("/auth/request", post(auth::request_link))
        .route("/auth/verify", post(auth::verify))
        .route("/auth/logout", post(auth::logout))
        .route("/me", get(auth::me).patch(auth::update_me))
        .route("/notifications", get(notifications::list))
        .route("/notifications/read", post(notifications::mark_read))
        .route("/games", get(games::list).post(games::create))
        .route("/games/{id}", get(games::show).patch(games::update).delete(games::delete))
        .route("/games/{id}/participants", post(games::add_participants))
        .route("/games/{id}/participants/{pid}", delete(games::remove_participant))
        .route("/games/{id}/participants/{pid}/invite", post(games::resend_invite))
        .route("/games/{id}/start", post(games::start_collecting))
        .route("/games/{id}/schedule", put(games::update_schedule))
        .route("/games/{id}/ready", post(games::mark_ready))
        .route("/games/{id}/reopen", post(games::reopen))
        .route("/games/{id}/finish", post(games::finish))
        .route("/games/{id}/resume", post(games::resume))
        .route("/games/{id}/joining", post(games::set_joining))
        .route("/games/{id}/notify-play", post(games::notify_play))
        .route("/games/{id}/photos", get(games::photos))
        .route("/games/{id}/photos/{photo_id}", delete(games::delete_photo))
        .route("/games/{id}/teams/{team_id}", delete(games::delete_team))
        .route("/collect/{token}", get(collect::show).patch(collect::update))
        .route(
            "/collect/{token}/photos",
            post(collect::upload).layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES + 512 * 1024)),
        )
        .route(
            "/collect/{token}/photos/{photo_id}",
            axum::routing::patch(collect::update_photo).delete(collect::delete_photo),
        )
        .route("/collect/{token}/done", post(collect::done))
        .route("/collect/{token}/undone", post(collect::undone))
        .route("/play/{code}", get(play::show))
        .route("/play/{code}/teams", post(play::join))
        .route("/play/{code}/state", get(play::state))
        .route("/play/{code}/guess", post(play::guess))
        .route("/play/{code}/leaderboard", get(play::leaderboard))
        .route("/images/{id}", get(media::full))
        .route("/images/{id}/thumb", get(media::thumb))
        .fallback(api_not_found)
        .layer(middleware::from_fn(no_store_api));

    let static_dir = PathBuf::from(&state.config.static_dir);
    let spa = ServeDir::new(&static_dir)
        .fallback(ServeFile::new(static_dir.join("index.html")));
    let spa = Router::new()
        .fallback_service(spa)
        .layer(middleware::from_fn(static_cache_headers));

    Router::new()
        .nest("/api", api)
        .merge(spa)
        .layer(middleware::from_fn(same_origin_writes))
        .layer(
            TraceLayer::new_for_http().make_span_with(|req: &Request| {
                // Log the route template, never the concrete path: paths carry secret tokens.
                let route = req
                    .extensions()
                    .get::<MatchedPath>()
                    .map(|p| p.as_str().to_string())
                    .unwrap_or_else(|| "static".into());
                tracing::info_span!("http", method = %req.method(), route = %route)
            })
            .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .layer(CompressionLayer::new())
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::REFERRER_POLICY,
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .with_state(state)
}

async fn health(axum::extract::State(state): axum::extract::State<AppState>) -> Response {
    match sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&state.db).await {
        Ok(_) => Json(json!({ "ok": true })).into_response(),
        Err(e) => {
            tracing::error!(error = ?e, "health check failed");
            (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "ok": false }))).into_response()
        }
    }
}

async fn api_not_found() -> Response {
    (StatusCode::NOT_FOUND, Json(json!({ "error": "Not found" }))).into_response()
}

/// Reject cross-site state-changing requests (CSRF defense on top of SameSite cookies).
async fn same_origin_writes(req: Request, next: Next) -> Response {
    if !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS) {
        if let Some(origin) = req.headers().get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
            let origin_host = origin.split("://").nth(1).unwrap_or_default();
            let host = req
                .headers()
                .get("x-forwarded-host")
                .or_else(|| req.headers().get(header::HOST))
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default();
            if origin_host != host {
                return (
                    StatusCode::FORBIDDEN,
                    Json(json!({ "error": "Cross-origin request blocked" })),
                )
                    .into_response();
            }
        }
    }
    next.run(req).await
}

async fn no_store_api(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    response
        .headers_mut()
        .entry(header::CACHE_CONTROL)
        .or_insert(HeaderValue::from_static("no-store"));
    response
}

/// Hashed build assets are immutable; everything else (index.html) must revalidate.
async fn static_cache_headers(req: Request, next: Next) -> Response {
    let immutable = req.uri().path().starts_with("/assets/");
    let mut response = next.run(req).await;
    let value = if immutable && response.status().is_success() {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static(value));
    response
}

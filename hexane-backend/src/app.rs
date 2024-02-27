use axum::{
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, post},
    Router,
};
use tower_http::{
    compression::CompressionLayer, services::ServeDir, timeout::TimeoutLayer, trace::TraceLayer,
};
use tower_sessions::{Expiry, SessionManagerLayer};
use tower_sessions_sqlx_store::PostgresStore;

use crate::handlers;
use crate::middlewares;
use crate::types::AppState;

pub fn app(state: AppState, session_store: PostgresStore) -> Router {
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false)
        .with_expiry(Expiry::OnInactivity(time::Duration::hours(24)));

    let protected_routes = Router::new()
        .route("/datasources", get(handlers::datasource::list))
        .route(
            "/datasource/file-action",
            post(handlers::datasource::file_action),
        )
        .route(
            "/datasources",
            // 50 MB body limit
            post(handlers::datasource::upload).layer(DefaultBodyLimit::max(50 * 1024 * 1024)),
        )
        .route("/query", get(handlers::query::query))
        .route("/query", post(handlers::query::query_post))
        .route("/account/logout", post(handlers::account::logout))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            middlewares::is_logged_in,
        ));

    let routes = Router::new()
        .route("/", get(handlers::home))
        .route("/account", get(handlers::account::account))
        .route("/account/login", get(handlers::account::login_get))
        .route("/account/login", post(handlers::account::login_post))
        .route("/account/register", get(handlers::account::register_get))
        .route("/account/register", post(handlers::account::register_post))
        .nest_service("/resources", ServeDir::new(&state.config.backend.resources));

    Router::new()
        .merge(routes)
        .merge(protected_routes)
        .fallback(handlers::not_found)
        .layer((
            session_layer,
            CompressionLayer::new(),
            // Graceful shutdown will wait for outstanding requests to complete.
            // Add a timeout so requests don't hang forever.
            TimeoutLayer::new(tokio::time::Duration::from_secs(120)),
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

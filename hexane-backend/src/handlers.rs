use crate::types::{AppState, UserSession};
use axum::{extract::State, response::Html};
use serde_json::json;

pub mod account;
pub mod datasource;
pub mod query;

pub async fn home(
    user_session: Option<UserSession>,
    State(state): State<AppState>,
) -> Html<String> {
    state
        .pages
        .render_index_body(json!({ "TEMPLATE": "pages/home" }), user_session.is_some())
}

pub async fn not_found(
    user_session: Option<UserSession>,
    State(state): State<AppState>,
) -> Html<String> {
    let page = json!({
        "title": "404 - Page Not Found",
        "body-main": {
            "TEMPLATE": "pages/404"
        }
    });

    state.pages.render_index(page, user_session.is_some())
}

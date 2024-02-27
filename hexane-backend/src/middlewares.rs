use axum::{
    extract::{OriginalUri, Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::types::{AppState, UserSession};

/// is_logged_in middleware runs Next if the user is logged in, otherwise it
/// returns a 401 - Unauthorized page.
pub async fn is_logged_in(
    user_session: Option<UserSession>,
    State(state): State<AppState>,
    OriginalUri(original_uri): OriginalUri,
    request: Request,
    next: Next,
) -> Response {
    if user_session.is_some() {
        next.run(request).await
    } else {
        let page = json!({
            "title": "401 - Unauthorized",
            "body-main": {
                "TEMPLATE": "pages/401",
                "return-to": original_uri.path()
            }
        });

        state
            .pages
            .render_index(page, user_session.is_some())
            .into_response()
    }
}

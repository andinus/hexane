use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};
use hexane_shared::Config;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPool;
use std::collections::HashSet;
use std::sync::Arc;
use tower_sessions::Session;
use uuid::Uuid;

use crate::pages::Pages;

/// App state for routers.
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub pages: Arc<Pages>,
    pub config: Arc<Config>,
    pub stop_words: Arc<HashSet<String>>,
}

#[derive(Default, Clone, Debug, Deserialize, Serialize)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub email: String,
}

#[derive(Debug)]
pub struct UserSession {
    user_data: User,
}

impl UserSession {
    const USER_DATA_KEY: &'static str = "user_data";

    pub fn id(&self) -> Uuid {
        self.user_data.id
    }

    pub fn username(&self) -> &str {
        &self.user_data.username
    }

    pub fn email(&self) -> &str {
        &self.user_data.email
    }

    pub async fn update_session(session: &Session, user_data: &User) {
        session
            .insert(Self::USER_DATA_KEY, user_data.clone())
            .await
            .unwrap()
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for UserSession
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(req: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let session = Session::from_request_parts(req, state).await?;
        if let Some(user_data) = session.get(Self::USER_DATA_KEY).await.unwrap() {
            Ok(Self { user_data })
        } else {
            Err((StatusCode::UNAUTHORIZED, "401 Unauthorized"))
        }
    }
}

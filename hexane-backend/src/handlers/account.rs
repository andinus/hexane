use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Redirect},
    Form,
};
use axum_htmx::HxRequest;
use email_address::EmailAddress;
use rand::distributions::{Alphanumeric, DistString};
use serde_json::json;
use sqlx::postgres::PgPool;
use tower_sessions::Session;

use crate::types::{AppState, User, UserSession};

/// Logged out users are redirected to the login page.
pub async fn account(
    State(state): State<AppState>,
    user_session: Option<UserSession>,
) -> impl IntoResponse {
    match user_session {
        Some(user) => {
            let credit = sqlx::query_file!("queries/account/credit.sql", user.id(),)
                .fetch_one(&state.pool)
                .await
                .unwrap()
                .credit
                .to_string();

            let page = json!({
                "title": "Account ~ Hexane",
                "body-main": {
                    "TEMPLATE": "pages/account",
                    "username": user.username(),
                    "email": user.email(),
                    "credits": credit
                }
            });

            state.pages.render_index(page, true).into_response()
        }
        None => Redirect::to("/account/login").into_response(),
    }
}

/// Logged in users are redirected to "/account".
pub async fn login_get(
    State(state): State<AppState>,
    user_session: Option<UserSession>,
) -> impl IntoResponse {
    match user_session {
        Some(_) => Redirect::to("/account").into_response(),
        None => {
            let page = json!({
                "title": "Login ~ Hexane",
                "body-main": {
                    "TEMPLATE": "pages/account/login",
                    "form-status": {
                        "TEMPLATE": "html/p-status",
                    },
                }
            });
            state.pages.render_index(page, false).into_response()
        }
    }
}

#[derive(serde::Deserialize)]
pub struct LoginForm {
    login: String,
    password: String,
}

/// Login controller needs form input, it verfies user credentials and initiates
/// their session.
pub async fn login_post(
    session: Session,
    State(state): State<AppState>,
    HxRequest(hx_request): HxRequest,
    Form(form): Form<LoginForm>,
) -> impl IntoResponse {
    let password_check = verify_user_password(form.login, form.password, state.pool).await;

    if password_check.is_err() {
        let message = "Authentication Failed";

        let status = state.pages.status_failed(message);
        if hx_request {
            return state.pages.render(status).into_response();
        }

        let page = json!({
            "title": "Login ~ Hexane",
            "body-main": {
                "TEMPLATE": "pages/account/login",
                "form-status": status
            }
        });

        return state.pages.render_index(page, false).into_response();
    }

    let user = password_check.unwrap();
    UserSession::update_session(&session, &user).await;

    match hx_request {
        true => [("HX-Redirect", "/")].into_response(),
        false => Redirect::to("/").into_response(),
    }
}

/// Logged in users are redirected to "/account".
pub async fn register_get(
    State(state): State<AppState>,
    user_session: Option<UserSession>,
) -> impl IntoResponse {
    match user_session {
        Some(_) => Redirect::to("/account").into_response(),
        None => {
            let page = json!({
                "title": "Register ~ Hexane",
                "body-main": {
                    "TEMPLATE": "pages/account/register",
                    "form-status": {
                        "TEMPLATE": "html/p-status",
                    },
                }
            });

            state.pages.render_index(page, false).into_response()
        }
    }
}

#[derive(serde::Deserialize)]
pub struct RegisterForm {
    email: String,
    password: String,
}

pub async fn register_post(
    session: Session,
    State(state): State<AppState>,
    HxRequest(hx_request): HxRequest,
    Form(form): Form<RegisterForm>,
) -> impl IntoResponse {
    let mut registration_err: Vec<&str> = vec![];

    if !EmailAddress::is_valid(&form.email) {
        registration_err.push("Invalid Email");
    }

    // Daily limit of 25 registrations.
    if sqlx::query_file!("queries/daily-registrations.sql",)
        .fetch_one(&state.pool)
        .await
        .unwrap()
        .count
        .unwrap_or(0)
        > 25
    {
        registration_err.push("Daily registrations limit reached, please come back tomorrow. Reach out to hexane@unfla.me for any queries.");
    }

    // Check if account exists already.
    if sqlx::query_file!("queries/account/user-exists.sql", &form.email,)
        .fetch_optional(&state.pool)
        .await
        .unwrap()
        .is_some()
    {
        registration_err.push("Account with this email already exists");
    }

    // Password validation.
    if form.password.len() < 8 {
        registration_err.push("Password must contain at least 8 characters");
    }

    if !registration_err.is_empty() {
        return state
            .pages
            .registration_failed(&registration_err.join(", "), hx_request)
            .into_response();
    }

    let (password_hash, username) = tokio::task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        let password_hash = Argon2::default()
            .hash_password(form.password.as_bytes(), &salt)
            .unwrap()
            .to_string();

        let username = Alphanumeric.sample_string(&mut rand::thread_rng(), 12);
        (password_hash, username)
    })
    .await
    .unwrap();

    match sqlx::query_file!(
        "queries/account/register.sql",
        username,
        &form.email,
        password_hash
    )
    .execute(&state.pool)
    .await
    {
        Ok(_) => {
            session.insert("prefill_email", &form.email).await.unwrap();

            let redirect_to = "/account/login";
            match hx_request {
                true => [("HX-Redirect", redirect_to)].into_response(),
                false => Redirect::to(redirect_to).into_response(),
            }
        }
        Err(_) => state
            .pages
            .registration_failed("Account registration failed.", hx_request)
            .into_response(),
    }
}

/// Logout handler flushes the session and redirects the user to homepage.
pub async fn logout(session: Session) -> impl IntoResponse {
    match session.flush().await {
        Ok(_) => Redirect::to("/").into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn verify_user_password(
    login: String,
    password: String,
    pool: PgPool,
) -> Result<User, String> {
    let row = sqlx::query_file!("queries/account/user-password.sql", &login)
        .fetch_one(&pool)
        .await;

    if row.is_err() {
        return Err(row.err().unwrap().to_string());
    }

    let row = row.unwrap();
    let verify_password = tokio::task::spawn_blocking(move || {
        let parsed_hash = PasswordHash::new(&row.password).unwrap();
        Argon2::default().verify_password(password.as_bytes(), &parsed_hash)
    })
    .await
    .unwrap();

    if verify_password.is_ok() {
        return Ok(User {
            id: row.id,
            username: row.username,
            email: row.email,
        });
    }

    Err("Invalid Password".to_string())
}

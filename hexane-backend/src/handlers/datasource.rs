use axum::{
    extract::{Form, Multipart, State},
    http::StatusCode,
    response::{IntoResponse, Redirect},
};
use axum_htmx::HxRequest;
use num_traits::cast::ToPrimitive;
use rand::distributions::{Alphanumeric, DistString};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::{
    fs,
    io::{AsyncWriteExt, BufWriter},
};

use crate::pages::datasource::Datasource;
use crate::types::{AppState, UserSession};

pub async fn list(
    State(state): State<AppState>,
    user_session: UserSession,
    HxRequest(hx_request): HxRequest,
) -> impl IntoResponse {
    if hx_request {
        return state
            .pages
            .render(Datasource::new(&state, &user_session).file_list().await);
    }

    state
        .pages
        .render_index(Datasource::new(&state, &user_session).page().await, true)
}

struct FileError {
    name: String,
    error: String,
}

pub async fn upload(
    user_session: UserSession,
    State(state): State<AppState>,
    HxRequest(hx_request): HxRequest,
    mut multipart: Multipart,
) -> impl IntoResponse {
    // Create user's drive directory.
    let user_drive = &state.config.file_store.join(&user_session.id().to_string());
    fs::create_dir_all(&user_drive).await.unwrap();

    // Users cannot upload more than 20 MB.
    let limits = sqlx::query_file!("queries/account/limits.sql", user_session.id(),)
        .fetch_one(&state.pool)
        .await
        .unwrap();

    if limits.file_uploaded.unwrap_or(0) > 20 * 1024 * 1024 {
        return StatusCode::BAD_REQUEST.into_response();
    }

    if limits.credit.to_f64().unwrap() <= 0 as f64 {
        return StatusCode::BAD_REQUEST.into_response();
    }

    let mut category: String = "default".to_string();

    // file_errors stores the files that weren't uploaded along with their errors.
    let mut file_uploads_count = 0;
    let mut file_errors: Vec<FileError> = vec![];

    // Parse uploaded form-data.
    while let Some(mut field) = multipart.next_field().await.unwrap() {
        let field_name = field.name().unwrap();

        // Update category.
        if field_name == "category" && category == "default" {
            let text = field.text().await.unwrap();
            if !text.is_empty() {
                category = text.to_lowercase();
            }
            continue;
        }

        // Only consider "file" fields.
        if field_name != "file" {
            continue;
        }

        let name = field.file_name().unwrap().to_string();

        // Verify file's content-type.
        let r#type = field.content_type().unwrap().to_string();
        match r#type.as_str() {
            "text/plain" | "application/pdf" => {}
            _ => {
                file_errors.push(FileError {
                    name,
                    error: format!("Unsupported file type ({})", &r#type),
                });
                break;
            }
        };

        // Create a temporary file to stream the upload.
        let path_tmp = user_drive.join(&format!(
            "{}.tmp",
            Alphanumeric.sample_string(&mut rand::thread_rng(), 16)
        ));

        let mut file = BufWriter::new(
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path_tmp)
                .await
                .unwrap(),
        );

        let mut size = 0;
        let mut hasher = Sha256::new();

        // Write the file to file-system and hash it's contents.
        while let Some(chunk) = field.chunk().await.unwrap() {
            size += chunk.len();
            hasher.update(&chunk);
            file.write_all(&chunk).await.unwrap();
        }

        file.flush().await.unwrap();

        let hash = hasher.finalize();
        let hash = hash
            .iter()
            .map(|b| format!("{:02x}", b).to_string())
            .collect::<String>();

        let path = user_drive.join(&format!(
            "{}-{}",
            hash,
            Alphanumeric.sample_string(&mut rand::thread_rng(), 8)
        ));
        let path_str = path
            .strip_prefix(&state.config.file_store)
            .unwrap()
            .to_string_lossy()
            .to_string();

        // Check if the file already exists, if not then add it as datasource.
        match sqlx::query_file!(
            "queries/datasource/file-by-hash.sql",
            user_session.id(),
            &hash
        )
        .fetch_optional(&state.pool)
        .await
        .unwrap()
        {
            Some(_) => {
                fs::remove_file(&path_tmp).await.unwrap();
                file_errors.push(FileError {
                    name,
                    error: "Duplicate file (Hash collision)".to_string(),
                });
            }
            None => {
                fs::rename(&path_tmp, &path).await.unwrap();
                sqlx::query_file!(
                    "queries/datasource/insert-file.sql",
                    user_session.id(),
                    name,
                    hash,
                    path_str,
                    size as i64,
                    r#type,
                    &category
                )
                .execute(&state.pool)
                .await
                .unwrap();

                file_uploads_count += 1;
            }
        }
    }

    let file_errors_html = file_errors
        .iter()
        .map(|x| {
            json!({
                "TEMPLATE": "html/li",
                "class": "fg-red",
                "text": format!("{}: {}", x.name, x.error)
            })
        })
        .collect::<Value>();

    let status = json!({
        "TEMPLATE": "pages/datasource/upload-status",
        "uploaded": file_uploads_count,
        "total-files": file_uploads_count + file_errors.len(),
        "file-errors": {
            "TEMPLATE": "html/ul",
            "items": file_errors_html
        }
    });

    if hx_request {
        return (
            [("HX-Trigger", "newDatasourceFile")],
            state.pages.render(status),
        )
            .into_response();
    }

    state
        .pages
        .render_index(
            Datasource::new(&state, &user_session)
                .with_status(status)
                .page()
                .await,
            true,
        )
        .into_response()
}

#[derive(Deserialize)]
pub struct FileActionForm {
    delete: String,
}

pub async fn file_action(
    user_session: UserSession,
    State(state): State<AppState>,
    HxRequest(hx_request): HxRequest,
    Form(form): Form<FileActionForm>,
) -> impl IntoResponse {
    let deleted_file = sqlx::query_file!(
        "queries/datasource/delete-file.sql",
        user_session.id(),
        form.delete
    )
    .fetch_one(&state.pool)
    .await
    .unwrap();

    fs::remove_file(state.config.file_store.join(deleted_file.path))
        .await
        .unwrap();

    if hx_request {
        return ([("HX-Trigger", "newDatasourceFile")], "File deleted.").into_response();
    }

    Redirect::to("/datasources").into_response()
}

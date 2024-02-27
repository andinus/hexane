use crate::types::{AppState, UserSession};
use human_bytes::human_bytes;
use num_traits::cast::ToPrimitive;
use serde_json::{json, Value};
use uuid::Uuid;

struct DatasourceFile {
    name: String,
    category: String,
    processed: Option<String>,
    hash: String,
    size: i64,
}

pub struct Datasource {
    state: AppState,
    user_id: Uuid,
    status: Option<Value>,
}

impl Datasource {
    pub fn new(state: &AppState, user_session: &UserSession) -> Datasource {
        Datasource {
            state: state.clone(),
            user_id: user_session.id(),
            status: None,
        }
    }

    pub fn with_status(mut self, status: Value) -> Datasource {
        self.status = Some(status);
        self
    }

    pub async fn page(&self) -> Value {
        let file_list = self.file_list().await;

        let categories =
            sqlx::query_file!("queries/datasource/select-categories.sql", self.user_id)
                .fetch_all(&self.state.pool)
                .await
                .unwrap()
                .iter()
                .map(|x| {
                    json!({
                        "TEMPLATE": "html/option",
                        "value": &x.category
                    })
                })
                .collect::<Vec<Value>>();

        let limits = sqlx::query_file!("queries/account/limits.sql", self.user_id)
            .fetch_one(&self.state.pool)
            .await
            .unwrap();

        let usage_limits = if limits.file_uploaded.unwrap_or(0) > 20 * 1024 * 1024 {
            Some("Cannot upload files, max size limit reached.")
        } else if limits.credit.to_f64().unwrap() <= 0 as f64 {
            Some("Cannot upload files, you've run out of credits. Reach out to hexane@unfla.me for additional credits.")
        } else {
            None
        };

        let form_class = if usage_limits.is_some() {
            "form-disabled"
        } else {
            ""
        };

        json!({
            "body-main": {
                "TEMPLATE": "pages/datasource",
                "status": self.status,
                "file-list": file_list,
                "category-options": categories,
                "upload-form": {
                    "TEMPLATE": "pages/datasource/upload-form",
                    "usage-limits": usage_limits,
                    "class": form_class
                }
            }
        })
    }

    pub async fn file_list(&self) -> Value {
        let datasources: Vec<DatasourceFile> =
            sqlx::query_file_as!(DatasourceFile, "queries/datasource/list.sql", self.user_id)
                .fetch_all(&self.state.pool)
                .await
                .unwrap();

        if datasources.is_empty() {
            return json!({
                "TEMPLATE": "html/p-status",
                "text": "No files uploaded."
            });
        }

        let total_size: i64 = datasources.iter().map(|x| x.size).sum();
        let processed_file_count = datasources.iter().filter(|x| x.processed.is_some()).count();

        let files = datasources
            .iter()
            .map(|x| {
                let (processed, class) = match &x.processed {
                    Some(processed) => (processed.as_str(), ""),
                    None => ("Not Processed", "datasource-file-unprocessed"),
                };

                json!({
                    "TEMPLATE": "pages/datasource/file-list-entry",
                    "class": class,
                    "name": x.name,
                    "hash": x.hash,
                    "category": x.category,
                    "processed": processed
                })
            })
            .collect::<Vec<Value>>();

        json!({
            "TEMPLATE": "pages/datasource/file-list",
            "files": files,
            "total-size": human_bytes(total_size as f64),
            "total-file-count": datasources.len(),
            "processed-file-count": processed_file_count,
        })
    }
}

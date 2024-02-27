use crate::types::{AppState, UserSession};
use axum::response::Html;
use serde_json::{json, Value};
use uuid::Uuid;

pub struct Query {
    state: AppState,
    user_id: Uuid,
    query: Option<String>,
    query_response: Option<Value>,
    category: Option<String>,
}

impl Query {
    pub fn new(state: &AppState, user_session: &UserSession) -> Query {
        Query {
            state: state.clone(),
            user_id: user_session.id(),
            query: None,
            query_response: None,
            category: None,
        }
    }

    pub fn with_query(mut self, query: &str) -> Query {
        self.query = Some(query.to_string());
        self
    }

    pub fn with_query_response(mut self, query_response: Value) -> Query {
        self.query_response = Some(query_response);
        self
    }

    pub fn with_query_failure(mut self, message: &str) -> Query {
        self.query_response = Some(json!({
            "TEMPLATE": "html/p-status",
            "class": "status-failed",
            "text": message.to_string()
        }));
        self
    }

    pub fn with_selected_category(mut self, category: &str) -> Query {
        self.category = Some(category.to_string());
        self
    }

    pub async fn page(&self) -> Value {
        json!({
            "TEMPLATE": "pages/query",
            "query-form": self.query_form().await
        })
    }

    pub async fn page_rendered(&self, hx_request: bool) -> Html<String> {
        if hx_request {
            self.state.pages.render(self.query_form().await)
        } else {
            self.state.pages.render_index_body(self.page().await, true)
        }
    }

    pub async fn query_form(&self) -> Value {
        json!({
            "TEMPLATE": "pages/query/query-form",
            "query": self.query,
            "query-response": self.query_response,
            "category-options": self.categories().await

        })
    }

    async fn categories(&self) -> Vec<Value> {
        let rows = sqlx::query_file!("queries/datasource/select-categories.sql", self.user_id)
            .fetch_all(&self.state.pool)
            .await
            .unwrap();

        rows.iter()
            .map(|x| {
                let selected = match &self.category {
                    Some(selected_category) => x.category == *selected_category,
                    None => x.category.is_empty(),
                };

                json!({
                    "TEMPLATE": "html/option",
                    "attributes": if selected { "selected" } else { "" },
                    "value": &x.category
                })
            })
            .collect::<Vec<Value>>()
    }
}

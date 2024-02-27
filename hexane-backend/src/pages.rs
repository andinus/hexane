use axum::response::Html;
use hexane_shared::merge_json;
use serde_json::{json, Value};
use template_nest::TemplateNest;

pub mod datasource;
pub mod query;

pub struct Pages {
    pub nest: TemplateNest,
}

impl Pages {
    fn index(&self, options: Value, logged_in: bool) -> Value {
        let navigation_links = if logged_in {
            json!({ "TEMPLATE": "navigation/logged-in" })
        } else {
            json!({ "TEMPLATE": "navigation/default" })
        };

        let mut page = json!({
            "TEMPLATE": "index",
            "header": {
                "TEMPLATE": "navigation",
                "links": navigation_links
            },
            "title": "Hexane - LLMs on your data"
        });
        merge_json(&mut page, &options);
        page
    }

    pub fn render(&self, to_render: Value) -> Html<String> {
        Html(self.nest.render(&to_render).unwrap())
    }

    pub fn render_index(&self, to_render: Value, logged_in: bool) -> Html<String> {
        let page = self.index(to_render, logged_in);
        Html(self.nest.render(&page).unwrap())
    }

    pub fn render_index_body(&self, body_main: Value, logged_in: bool) -> Html<String> {
        let page = self.index(
            json!({
                "body-main": body_main
            }),
            logged_in,
        );
        Html(self.nest.render(&page).unwrap())
    }

    pub fn status_failed(&self, message: &str) -> Value {
        json!({
            "TEMPLATE": "html/p-status",
            "class": "status-failed",
            "text": &message
        })
    }

    pub fn registration_failed(&self, message: &str, hx_request: bool) -> Html<String> {
        let status = self.status_failed(message);

        if hx_request {
            return Html(self.nest.render(&status).unwrap());
        }

        let page = json!({
            "title": "Register ~ Hexane",
            "body-main": {
                "TEMPLATE": "pages/account/register",
                "form-status": status
            }
        });

        let page = self.index(page, false);
        Html(self.nest.render(&page).unwrap())
    }
}

use axum::{extract::State, response::Html, Form};
use axum_htmx::HxRequest;
use num_traits::cast::ToPrimitive;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{types::BigDecimal, Row};
use std::{collections::HashSet, convert::TryFrom};
use tokio::time::{Duration, Instant};

use hexane_shared::{get_embeddings, merge_json};

use crate::pages::query::Query;
use crate::types::{AppState, UserSession};

fn process_query(q: &str, stop_words: &HashSet<String>) -> String {
    q.split(' ')
        .filter(|w| !stop_words.contains(&w.to_string().to_lowercase()))
        .collect::<Vec<&str>>()
        .join(" ")
        .replace(&['(', ')', ',', '\"', '.', ';', ':', '\'', '?'][..], "")
}

pub async fn query(user_session: UserSession, State(state): State<AppState>) -> Html<String> {
    state
        .pages
        .render_index_body(Query::new(&state, &user_session).page().await, true)
}

#[derive(Serialize, Deserialize, Debug)]
struct QueryReferences {
    file: String,
    text: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct QueryForm {
    query: String,
    category: String,
}

pub async fn query_post(
    user_session: UserSession,
    State(state): State<AppState>,
    HxRequest(hx_request): HxRequest,
    Form(form): Form<QueryForm>,
) -> Html<String> {
    let query_page = Query::new(&state, &user_session)
        .with_selected_category(&form.category)
        .with_query(&form.query);

    if form.query.len() > 1024 {
        return query_page
            .with_query_failure("Query too long")
            .page_rendered(hx_request)
            .await;
    }

    let credit = sqlx::query_file!("queries/account/credit.sql", user_session.id(),)
        .fetch_one(&state.pool)
        .await
        .unwrap()
        .credit;

    if credit.to_f64().unwrap() <= 0 as f64 {
        return query_page
            .with_query_failure(
                "You've run out of credits. Reach out to hexane@unfla.me for additional credits.",
            )
            .page_rendered(hx_request)
            .await;
    }

    let query_processed = process_query(&form.query, &state.stop_words);
    let query_embedding = &get_embeddings(
        json!(&query_processed),
        &state.config,
        &user_session.id(),
        &mut state.pool.acquire().await.unwrap(),
    )
    .await[0];

    let sql_query = format!(
        "
SELECT text, file.name
FROM datasource.embedding JOIN datasource.file ON file.id = embedding.file_id
WHERE file.user_id = $1
  AND embedding.created = file.processed
  {}
  AND (embedding <-> $2::vector) < 1.20
ORDER BY (embedding <-> $2::vector)
LIMIT 5;",
        if form.category.is_empty() {
            ""
        } else {
            "AND category = $3"
        }
    );

    let mut query_builder = sqlx::query(&sql_query)
        .bind(user_session.id())
        .bind(query_embedding);

    if !form.category.is_empty() {
        query_builder = query_builder.bind(form.category);
    }

    let context_vec: Vec<QueryReferences> = match query_builder.fetch_all(&state.pool).await {
        Ok(rows) => rows
            .iter()
            .map(|r| QueryReferences {
                file: r.try_get::<String, _>("name").unwrap(),
                text: r.try_get::<String, _>("text").unwrap(),
            })
            .collect(),
        Err(err) => panic!("{}", err),
    };

    // If we don't have any data from the context then we cannot answer this
    // query.
    if context_vec.is_empty() {
        let message = "Sorry, we cannot answer this query. We don't have any document that contains relevant information. Including more keywords in the query might help.";

        return query_page
            .with_query_failure(message)
            .page_rendered(hx_request)
            .await;
    }

    let context: String = context_vec
        .iter()
        .map(|r| format!("filename: {}\n{}", r.file, r.text))
        .collect::<Vec<String>>()
        .join("\n\n");

    let mut body_params = json!({
        "messages": [
            {
                "role": "system",
                "content": &state.config.backend.system_prompt
            },
            {
                "role": "system",
                "content": format!("CONTEXT:\n{}", context)
            },
            {
                "role": "user",
                "content": &form.query
            }
        ]
    });
    merge_json(&mut body_params, &state.config.chat_completion.body_param);

    let model_start = Instant::now();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    let model_response = client
        .post(&state.config.chat_completion.api)
        .header(
            "Authorization",
            format!("Bearer {}", &state.config.chat_completion.key),
        )
        .json(&body_params)
        .send()
        .await;

    let model_response = match model_response {
        Ok(response) => match response.json::<serde_json::Value>().await {
            Ok(body) => Some(body),
            Err(_) => None,
        },
        Err(_) => None,
    };

    if model_response.is_none() {
        return query_page
            .with_query_failure("Failed to generate a response.")
            .page_rendered(hx_request)
            .await;
    }

    let res = model_response.unwrap();
    let usage = &res["usage"];
    let message = res["choices"][0]["message"]["content"]
        .to_string()
        .replace("\\n", "\n");

    let pricing = &state.config.chat_completion.pricing;

    let total_cost = (usage["prompt_tokens"].as_f64().unwrap() * pricing.input)
        + (usage["completion_tokens"].as_f64().unwrap() * pricing.output);
    let total_cost = total_cost / 1000.0;

    sqlx::query_file!(
        "queries/account/credit-decrement.sql",
        user_session.id(),
        BigDecimal::try_from(total_cost).unwrap()
    )
    .execute(&state.pool)
    .await
    .unwrap();

    let references = {
        let items = context_vec
            .iter()
            .map(|r| &r.file)
            .collect::<HashSet<&String>>()
            .iter()
            .map(|file| {
                json!({
                    "TEMPLATE": "html/li",
                    "text": file
                })
            })
            .collect::<Vec<Value>>();

        json!({
            "TEMPLATE": "pages/query/query-response-references",
            "items": items
        })
    };

    let query_response = json!({
        "TEMPLATE": "pages/query/query-response",
        "response-time": model_start.elapsed().as_secs(),
        "references": references,
        "tokens-total": usage["total_tokens"],
        "tokens-prompt": usage["prompt_tokens"],
        "tokens-completion": usage["completion_tokens"],
        "response": message[1..message.len() - 1].to_string(),
    });

    query_page
        .with_query_response(query_response)
        .page_rendered(hx_request)
        .await
}

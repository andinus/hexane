use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::postgres::PgConnection;
use sqlx::types::BigDecimal;
use std::{collections::HashSet, path::PathBuf, time::Duration};
use uuid::Uuid;

pub fn merge_json(a: &mut Value, b: &Value) {
    match (a, b) {
        (&mut Value::Object(ref mut a), Value::Object(ref b)) => {
            for (k, v) in b {
                merge_json(a.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (a, b) => {
            *a = b.clone();
        }
    }
}

pub async fn get_embeddings(
    chunks: Value,
    config: &Config,
    user_id: &Uuid,
    pool: &mut PgConnection,
) -> Vec<Vec<f64>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(30))
        .build()
        .unwrap();

    let res = client
        .post(&config.embedding.api)
        .header("Authorization", format!("Bearer {}", &config.embedding.key))
        .json(&json!({
            "model": &config.embedding.model,
            "input": chunks // can be string or an array
        }))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();

    let usage = &res["usage"];
    let total_cost = (usage["total_tokens"].as_f64().unwrap() * config.embedding.pricing) / 1000.0;

    sqlx::query_file!(
        "queries/account/credit-decrement.sql",
        user_id,
        BigDecimal::try_from(total_cost).unwrap()
    )
    .execute(pool)
    .await
    .unwrap();

    res["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| {
            x["embedding"]
                .as_array()
                .unwrap()
                .iter()
                .map(|y| y.as_f64().unwrap())
                .collect::<Vec<f64>>()
        })
        .collect::<Vec<Vec<f64>>>()
}

/// Shared configuration state for hexane programs.
#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub file_store: PathBuf,
    pub backend: Backend,
    pub file_processor: FileProcessor,
    pub embedding: Embedding,
    pub chat_completion: ChatCompletion,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Backend {
    pub template_directory: PathBuf,
    pub resources: PathBuf,
    pub stop_words: PathBuf,
    pub system_prompt: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct FileProcessor {
    pub max_active_process: u32,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ChatCompletion {
    pub api: String,
    pub key: String,
    pub body_param: Value,
    pub pricing: Pricing,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Pricing {
    pub input: f64,
    pub output: f64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Embedding {
    pub api: String,
    pub model: String,
    pub key: String,
    pub pricing: f64,
}

impl Config {
    pub fn get_stop_words(&self) -> HashSet<String> {
        std::fs::read_to_string(&self.backend.stop_words)
            .unwrap_or_else(|e| panic!("reading stop-words.txt: {}", e))
            .split('\n')
            .map(|w| w.to_string())
            .collect()
    }
}

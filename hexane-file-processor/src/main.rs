use clap::Parser;
use serde_json::json;
use sqlx::{
    postgres::{PgListener, PgPoolOptions},
    Pool, Postgres, QueryBuilder,
};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};
use text_splitter::TextSplitter;
use tokio::time::{sleep, Duration};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

use hexane_file_processor::pdf_to_text;
use hexane_shared::{get_embeddings, Config};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// database URL
    #[arg(long, env)]
    database_url: String,

    /// Path to config file
    #[arg(long, env, default_value = "config.toml")]
    config: PathBuf,
}

struct Embedding<'a>(Uuid, &'a str, &'a Vec<f64>);

struct Datasource {
    pub id: Uuid,
    pub user_id: Uuid,
    pub path: String,
    pub r#type: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "hexane_file_processor=trace".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args = Args::parse();
    let config: Arc<Config> = Arc::new(
        toml::from_str(
            &fs::read_to_string(args.config)
                .unwrap_or_else(|e| panic!("reading config file: {}", e)),
        )
        .unwrap(),
    );

    let pool = PgPoolOptions::new()
        .max_connections(20)
        .min_connections(4)
        .connect(&args.database_url)
        .await
        .unwrap_or_else(|_| panic!("connect to postgres db: {}", args.database_url));

    let initial_files = sqlx::query_file!("queries/datasource/get-unprocessed-file-count.sql")
        .fetch_one(&pool)
        .await
        .unwrap();

    let files_to_process = Arc::new(Mutex::new(initial_files.count.unwrap() as u32));

    {
        let pool = pool.clone();
        let files_to_process = Arc::clone(&files_to_process);

        let mut listener = PgListener::connect_with(&pool).await.unwrap();
        listener
            .listen_all(vec!["datasource_insert"])
            .await
            .unwrap();

        tokio::spawn(async move {
            loop {
                let _notification = listener.recv().await.unwrap();
                *files_to_process.lock().unwrap() += 1;
            }
        });
    }

    let active_process = Arc::new(Mutex::new(0_u32));
    let mut last_check = Instant::now();
    loop {
        // Check for unprocessed files every 5 minutes.
        if last_check.elapsed().as_secs() > 300 {
            *files_to_process.lock().unwrap() =
                sqlx::query_file!("queries/datasource/get-unprocessed-file-count.sql")
                    .fetch_one(&pool)
                    .await
                    .unwrap()
                    .count
                    .unwrap() as u32;
        }

        // Sleep for 3 seconds if no files to process or max_active_process is
        // running.
        if *files_to_process.lock().unwrap() == 0
            && *active_process.lock().unwrap() < config.file_processor.max_active_process
        {
            // Sleep for 3 seconds in between checks.
            sleep(Duration::from_millis(3000)).await;
            continue;
        }
        last_check = Instant::now();

        while *files_to_process.lock().unwrap() > 0
            && *active_process.lock().unwrap() < config.file_processor.max_active_process
        {
            *files_to_process.lock().unwrap() -= 1;
            *active_process.lock().unwrap() += 1;

            let pool = pool.clone();
            let config = Arc::clone(&config);

            let active_process = Arc::clone(&active_process);
            let files_to_process = Arc::clone(&files_to_process);

            tokio::spawn(async move {
                tracing::trace!("polling the database");
                process_file(config, pool, files_to_process).await;
                *active_process.lock().unwrap() -= 1;
            });
        }
    }
}

/// process_file calls the database to get a file from the queue, it exists if
/// there is no file in the queue, otherwise it processes the file it pulled
/// from the queue.
async fn process_file(
    config: Arc<Config>,
    pool: Pool<Postgres>,
    files_to_process: Arc<Mutex<u32>>,
) {
    let mut tx = pool.begin().await.unwrap();
    let to_process =
        sqlx::query_file_as!(Datasource, "queries/datasource/get-unprocessed-file.sql")
            .fetch_one(&mut *tx)
            .await;

    // Reset the counter to 0 if there are no files to process.
    if to_process.is_err() {
        tracing::trace!("no files to process, resetting count");
        *files_to_process.lock().unwrap() = 0;
        return;
    }
    let to_process = to_process.unwrap();

    tracing::debug!("processing file: {}", &to_process.id);
    let file_text = match to_process.r#type.as_str() {
        "text/plain" => fs::read_to_string(&config.file_store.join(&to_process.path)).unwrap(),
        "application/pdf" => pdf_to_text(&config.file_store.join(&to_process.path))
            .await
            .unwrap(),
        _ => panic!("cannot handle file type: `{}'", &to_process.r#type),
    };

    tracing::debug!("got file's text data: {}", &to_process.id);

    // A helpful rule of thumb is that one token generally corresponds to ~4
    // characters of text for common English text. This translates to roughly ¾
    // of a word (so 100 tokens ~= 75 words).
    // https://platform.openai.com/tokenizer
    //
    // We're going to chunk by 500 tokens, i.e. ~2000 characters.
    let splitter = TextSplitter::default().with_trim_chunks(true);
    let max_characters = 1800..2000;
    let chunks = splitter
        .chunks(&file_text, max_characters)
        .collect::<Vec<&str>>();

    let embeddings_vec: Vec<Vec<f64>> =
        get_embeddings(json!(chunks), &config, &to_process.user_id, &mut tx).await;

    let mut embeddings: Vec<Embedding> = vec![];
    for x in 0..chunks.len() {
        embeddings.push(Embedding(to_process.id, chunks[x], &embeddings_vec[x]));
    }

    let mut query_builder =
        QueryBuilder::new("INSERT INTO datasource.embedding (file_id, text, embedding) ");

    query_builder.push_values(embeddings, |mut b, new| {
        b.push_bind(new.0).push_bind(new.1).push_bind(new.2);
    });
    query_builder.build().execute(&mut *tx).await.unwrap();

    sqlx::query_file!("queries/datasource/set-processed.sql", &to_process.id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();

    tx.commit().await.unwrap();
}

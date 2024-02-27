use clap::Parser;
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use std::{collections::HashSet, path::PathBuf};
use template_nest::{TemplateNest, TemplateNestOption};
use tokio::signal;
use tokio_util::sync::CancellationToken;
use tower_sessions::ExpiredDeletion;
use tower_sessions_sqlx_store::PostgresStore;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use hexane_shared::Config;

mod app;
mod handlers;
mod middlewares;
mod pages;
mod types;

use crate::pages::Pages;
use crate::types::AppState;

/// Server for Hexane
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = "127.0.0.1")]
    address: String,

    #[arg(short, long, default_value_t = 34701)]
    port: u16,

    #[arg(long, env)]
    database_url: String,

    /// Path to config file
    #[arg(long, env, default_value = "config.toml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "hexane_backend=trace,tower_http=trace,axum::rejection=trace".into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args = Args::parse();
    let config: Config = toml::from_str(
        &std::fs::read_to_string(args.config)
            .unwrap_or_else(|e| panic!("reading config file: {}", e)),
    )
    .unwrap();

    // connect to the database.
    let pool = PgPoolOptions::new()
        .min_connections(2)
        .max_connections(10)
        .connect(&args.database_url)
        .await
        .unwrap_or_else(|_| panic!("connect to postgres db: {}", args.database_url));

    // run migrations.
    sqlx::migrate!()
        .run(&pool)
        .await
        .unwrap_or_else(|err| panic!("running sqlx migrations: {}: {}", args.database_url, err));

    // initialize app state.
    let stop_words: HashSet<String> = config.get_stop_words();

    let nest = TemplateNest::new(TemplateNestOption {
        directory: config.backend.template_directory.clone(),
        ..Default::default()
    })
    .expect("failed to create nest object");

    let state = AppState {
        config: Arc::new(config),
        stop_words: Arc::new(stop_words),
        pool: pool.clone(),
        pages: Arc::new(Pages { nest }),
    };

    let session_store = PostgresStore::new(pool.clone());
    session_store.migrate().await.unwrap();

    // bind to port and serve the app.
    let listener = tokio::net::TcpListener::bind(&format!("{}:{}", args.address, args.port))
        .await
        .unwrap();
    tracing::debug!("listening on {}", listener.local_addr().unwrap());

    // run axum web service and session store continuous deletion tasks.
    let token = CancellationToken::new();
    let axum_token = token.clone();
    let axum_task = axum::serve(listener, app::app(state, session_store.clone()))
        .with_graceful_shutdown(async move { axum_token.cancelled().await });

    let cloned_token = token.clone();
    let deletion_task = tokio::task::spawn(async move {
        tokio::select! {
            _ = cloned_token.cancelled() => { }
            _ = session_store
                .continuously_delete_expired(tokio::time::Duration::from_secs(120)) => { }
        }
    });

    tokio::spawn(async move {
        shutdown_signal().await;
        tracing::debug!("shutting down axum_task & deletion_task");
        token.cancel();
    });

    axum_task.await.unwrap();
    deletion_task.await.unwrap();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

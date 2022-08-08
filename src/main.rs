use crate::netem::{NetEm, Output};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get_service, post};
use axum::{Json, Router, Server};
use clap::Parser;
use log::LevelFilter;
use std::net::SocketAddr;
use std::path::PathBuf;
use tower_http::services::ServeDir;

mod netem;

#[derive(Debug, Parser)]
#[clap(name = "taco")]
struct Opts {
    #[clap(short, long, default_value = "88")]
    port: u16,
    #[clap(short, long)]
    web: PathBuf,
    #[clap(short, long, default_value = "INFO")]
    log_level: LevelFilter,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let Opts {
        port,
        web,
        log_level,
    } = Opts::parse();

    env_logger::builder().filter_level(log_level).try_init()?;

    let router = Router::new()
        .route("/api", post(api))
        .fallback(get_service(ServeDir::new(web)).handle_error(handle_error));

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    log::info!("Taco server is running on {}...", port);
    Server::bind(&addr)
        .serve(router.into_make_service())
        .await?;

    Ok(())
}

async fn handle_error(err: std::io::Error) -> impl IntoResponse {
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

async fn api(Json(netem): Json<NetEm>) -> Json<Output> {
    Json(netem.execute().await)
}

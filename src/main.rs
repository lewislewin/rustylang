mod cli;
mod config;
mod diff;
mod errors;
mod json_utils;
mod openai_client;

use anyhow::Result;
use cli::{Cli, Commands};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    // Load env variables from .env if present
    let _ = dotenvy::dotenv();
    // init logging
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .compact()
        .init();

    let cli = <Cli as clap::Parser>::parse();

    match cli.command {
        Commands::Set(args) => cli::handle_set(args).await,
        Commands::Translate(args) => cli::handle_translate(args).await,
    }
}

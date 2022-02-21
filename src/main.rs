use crate::desk::Desk;

use anyhow::Context;
use clap::{Parser, Subcommand};
use std::convert::identity;
use std::time::Duration;
use tokio::time;
use tokio::time::timeout;

mod desk;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
#[clap(propagate_version = true)]
struct Args {
    #[clap(subcommand)]
    command: Commands,
    /// Set the timeout in seconds, 0 for infinite
    #[clap(long, default_value_t = 60)]
    timeout: u64,
    /// Set the environment log level
    #[clap(long, env = env_logger::DEFAULT_FILTER_ENV, default_value_t = String::from("info"))]
    log_level: String,
    /// Set the environment log style
    #[clap(long, env = env_logger::DEFAULT_WRITE_STYLE_ENV)]
    log_style: Option<String>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Sit or use `save` to store the current height
    Sit {
        #[clap(subcommand)]
        save: Option<SaveCommand>,
    },
    /// Stand or use `save` to store the current height
    Stand {
        #[clap(subcommand)]
        save: Option<SaveCommand>,
    },
    /// Get the current desk height
    Query,
    /// Sit -> Stand or Stand -> Sit
    Toggle,
    /// Listen for height changes
    Listen,
}

#[derive(Subcommand, Debug)]
enum SaveCommand {
    Save,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let args = Args::parse();

    setup_logging(&args)?;

    let runner = run_command(&args);
    if args.timeout > 0 {
        timeout(Duration::from_secs(args.timeout), runner)
            .await
            .context("Operation timed out")
            .and_then(identity)?
    } else {
        runner.await?
    }

    Ok(())
}

fn setup_logging(args: &Args) -> Result<(), anyhow::Error> {
    let mut builder = env_logger::Builder::new();
    builder.parse_filters(&args.log_level);

    if let Some(s) = &args.log_style {
        builder.parse_write_style(s);
    }

    builder.try_init().context("Failed to setup logger")
}

async fn run_command(args: &Args) -> Result<(), anyhow::Error> {
    let desk = Desk::new().await?;

    match &args.command {
        Commands::Sit { save } => {
            if save.is_some() {
                desk.save_sit().await?;
            } else {
                desk.sit().await?;
            }

            // let the packet actually send
            desk.query_height().await?;
        }
        Commands::Stand { save } => {
            if save.is_some() {
                desk.save_stand().await?;
            } else {
                desk.stand().await?;
            }

            // let the packet actually send
            desk.query_height().await?;
        }
        Commands::Query => {
            println!("{}", desk.query_height().await?);
        }
        Commands::Toggle => {
            let height = desk.query_height().await?;
            if height > 255 {
                desk.sit().await?;
            } else {
                desk.stand().await?;
            }

            // let the packet actually send
            desk.query_height().await?;
        }
        Commands::Listen => {
            let mut height = 0;
            loop {
                let next_height = desk.height();
                if height != next_height {
                    let (low, high) = desk.raw_height();
                    println!("height: ({:x},{:x}) -> {}", low, high, next_height);
                }
                height = next_height;

                time::sleep(Duration::from_millis(100)).await;
            }
        }
    }

    Ok(())
}

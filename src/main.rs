use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use tokio::time;

use crate::desk::Desk;
use anyhow::Context;
use std::convert::identity;
use std::time::Duration;
use tokio::time::timeout;

mod desk;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let matches = App::new("uplift-cli")
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .arg(
            Arg::with_name("log-level")
                .long("log-level")
                .help("Set the environment log level")
                .env(env_logger::DEFAULT_FILTER_ENV)
                .default_value("info"),
        )
        .arg(
            Arg::with_name("log-style")
                .long("log-style")
                .help("Set the environment log style")
                .env(env_logger::DEFAULT_WRITE_STYLE_ENV),
        )
        .arg(
            Arg::with_name("timeout")
                .long("timeout")
                .help("Set the timeout in seconds. 0 for infinite")
                .default_value("60"),
        )
        .subcommand(SubCommand::with_name("listen"))
        // .subcommand(SubCommand::with_name("set").arg(Arg::with_name("height").required(true)))
        .subcommand(SubCommand::with_name("sit").arg(Arg::with_name("save")))
        .subcommand(SubCommand::with_name("stand").arg(Arg::with_name("save")))
        .subcommand(SubCommand::with_name("toggle"))
        .subcommand(SubCommand::with_name("query").arg(Arg::with_name("signal")))
        .get_matches();

    setup_logging(&matches)?;

    let timeout_seconds = matches
        .value_of("timeout")
        .unwrap()
        .parse::<u64>()
        .context("Couldn't parse timeout")?;

    let runner = run_command(&matches);
    if timeout_seconds > 0 {
        timeout(Duration::from_secs(timeout_seconds), runner)
            .await
            .context("Operation timed out")
            .and_then(identity)
    } else {
        runner.await
    }
}

fn setup_logging(matches: &ArgMatches) -> Result<(), anyhow::Error> {
    let log_level = matches.value_of("log-level").unwrap();
    let log_style = matches.value_of("log-style");

    let mut builder = env_logger::Builder::new();
    builder.parse_filters(log_level);

    if let Some(s) = log_style {
        builder.parse_write_style(s);
    }

    builder.try_init().context("Failed to setup logger")
}

async fn run_command(matches: &ArgMatches<'_>) -> Result<(), anyhow::Error> {
    let desk = Desk::new().await?;

    match matches.subcommand() {
        ("listen", _) => {
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
        ("sit", Some(sub_matches)) => {
            if sub_matches.value_of("save").is_some() {
                desk.save_sit().await?;
            } else {
                desk.sit().await?;
            }

            // let the packet actually send
            desk.query_height().await?;
        }
        ("stand", Some(sub_matches)) => {
            if sub_matches.value_of("save").is_some() {
                desk.save_stand().await?;
            } else {
                desk.stand().await?;
            }

            // let the packet actually send
            desk.query_height().await?;
        }
        ("toggle", _) => {
            let height = desk.query_height().await?;
            if height > 255 {
                desk.sit().await?;
            } else {
                desk.stand().await?;
            }

            // let the packet actually send
            desk.query_height().await?;
        }
        ("query", Some(_sub_matches)) => {
            // if sub_matches.value_of("signal").is_some() {
            //     println!("{}", desk.rssi());
            // } else {
            println!("{}", desk.query_height().await?);
            // }
        }
        _ => unreachable!(),
    }

    Ok(())
}

use btleplug::api::Manager as _;
use btleplug::platform::Manager;
use futures::StreamExt;
use log::LevelFilter;
use std::time::Duration;
use tokio::time::sleep;
use uom::fmt::DisplayStyle;
use uom::si::length::{foot, inch};
use uplift_lib::Command;
use uplift_lib::UpliftDesk;
use uplift_lib::UpliftDeskId;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    env_logger::builder()
        .filter_module("uplift_lib", LevelFilter::Trace)
        .init();

    let manager = Manager::new().await.unwrap();

    let adapters = manager.adapters().await.unwrap();
    let adapter = adapters.into_iter().next().unwrap();

    // let id = UpliftDeskId::scan(&adapter)
    //     .await
    //     .unwrap()
    //     .next()
    //     .await
    //     .unwrap();

    let id = UpliftDeskId::from(Uuid::parse_str("0ab28845-04db-6dd1-ddf7-58a1b26ccf31")?);

    let mut desk = UpliftDesk::new(id, &adapter).await.unwrap();
    // let mut desk = UpliftDesk::new(id, &adapter).await.unwrap();

    let mut height_stream = desk.height_stream().await?;
    while let Some(height) = height_stream.next().await {
        desk.command(Command::Unknown {
            cmd: 0xfe,
            data: &[],
        })
        .await
        .unwrap();
        let rssi = desk.rssi();

        println!(
            "height: {} {} rssi: {rssi:?}",
            height
                .trunc::<foot>()
                .into_format_args(foot, DisplayStyle::Abbreviation),
            height
                .fract::<foot>()
                .into_format_args(inch, DisplayStyle::Abbreviation)
        )
    }

    // let mut interval = interval(Duration::from_millis(500));
    // for _ in 0..10 {
    //     interval.tick().await;
    //     println!("name: {:?}", desk.name().await.unwrap());
    //     println!("height: {:?}", desk.height().await.unwrap());
    // }

    drop(desk);

    sleep(Duration::from_secs(20)).await;

    Ok(())
}

// use std::convert::identity;
// use std::future::Future;
// use std::time::Duration;
//
// use anyhow::{anyhow, Context};
// use clap::{Parser, Subcommand};
// use tokio::time;
// use tokio::time::timeout;
//
// use crate::desk::{Desk, AVG_MID_HEIGHT, AVG_SITTING_HEIGHT, AVG_STANDING_HEIGHT};
//
// mod desk;
//
// const FORCE_ATTEMPTS: usize = 5;
//
// #[derive(Parser, Debug)]
// #[clap(author, version, about, long_about = None)]
// #[clap(propagate_version = true)]
// struct Args {
//     #[clap(subcommand)]
//     command: Commands,
//     /// Set the timeout in seconds, 0 for infinite
//     #[clap(long, default_value_t = 60)]
//     timeout: u64,
//     /// Set the environment log level
//     #[clap(long, env = env_logger::DEFAULT_FILTER_ENV, default_value_t = String::from("info"))]
//     log_level: String,
//     /// Set the environment log style
//     #[clap(long, env = env_logger::DEFAULT_WRITE_STYLE_ENV)]
//     log_style: Option<String>,
// }
//
// #[derive(Subcommand, Debug)]
// enum Commands {
//     /// Sit or use `save` to store the current height
//     Sit {
//         #[clap(subcommand)]
//         save: Option<SaveCommand>,
//     },
//     /// Retry the Sit operation 5 times if the desk doesn't complete it
//     ForceSit,
//     /// Stand or use `save` to store the current height
//     Stand {
//         #[clap(subcommand)]
//         save: Option<SaveCommand>,
//     },
//     /// Retry the Stand operation 5 times if the desk doesn't complete it
//     ForceStand,
//     /// Get the estimated desk height in inches
//     Query,
//     /// Sit -> Stand or Stand -> Sit
//     Toggle,
//     /// Retry the Toggle operation 5 times if the desk doesn't complete it
//     ForceToggle,
//     /// Listen for height changes
//     Listen,
// }
//
// #[derive(Subcommand, Debug)]
// enum SaveCommand {
//     Save,
// }
//
// #[tokio::main]
// async fn main() -> Result<(), anyhow::Error> {
//     let args = Args::parse();
//
//     setup_logging(&args)?;
//
//     let runner = run_command(&args);
//     if args.timeout > 0 {
//         timeout(Duration::from_secs(args.timeout), runner)
//             .await
//             .context("Operation timed out")
//             .and_then(identity)?
//     } else {
//         runner.await?
//     }
//
//     Ok(())
// }
//
// fn setup_logging(args: &Args) -> Result<(), anyhow::Error> {
//     let mut builder = env_logger::Builder::new();
//     builder.parse_filters(&args.log_level);
//
//     if let Some(s) = &args.log_style {
//         builder.parse_write_style(s);
//     }
//
//     builder.try_init().context("Failed to setup logger")
// }
//
// async fn run_command(args: &Args) -> Result<(), anyhow::Error> {
//     let desk = Desk::new().await?;
//
//     match &args.command {
//         Commands::Sit { save } => {
//             if save.is_some() {
//                 desk.save_sit().await?;
//             } else {
//                 desk.sit().await?;
//             }
//
//             // let the packet actually send
//             desk.query_height().await?;
//         }
//         Commands::ForceSit => {
//             force_sit(&desk).await?;
//         }
//         Commands::Stand { save } => {
//             if save.is_some() {
//                 desk.save_stand().await?;
//             } else {
//                 desk.stand().await?;
//             }
//
//             // let the packet actually send
//             desk.query_height().await?;
//         }
//         Commands::ForceStand => {
//             force_stand(&desk).await?;
//         }
//         Commands::Query => {
//             println!("{}", desk.query_height().await? as f32 / 10.0);
//         }
//         Commands::Toggle => {
//             let height = desk.query_height().await?;
//             if height > AVG_MID_HEIGHT {
//                 desk.sit().await?;
//             } else {
//                 desk.stand().await?;
//             }
//
//             // let the packet actually send
//             desk.query_height().await?;
//         }
//         Commands::ForceToggle => {
//             let height = desk.query_height().await?;
//             if height > AVG_MID_HEIGHT {
//                 force_sit(&desk).await?;
//             } else {
//                 force_stand(&desk).await?;
//             }
//         }
//         Commands::Listen => {
//             let mut height = 0;
//             loop {
//                 let next_height = desk.height();
//                 if height != next_height {
//                     let (low, high) = desk.raw_height();
//                     println!("height: ({low:x},{high:x}) -> {next_height}");
//                 }
//                 height = next_height;
//
//                 time::sleep(Duration::from_millis(100)).await;
//             }
//         }
//     }
//
//     Ok(())
// }
//
// async fn force_sit(desk: &Desk) -> Result<(), anyhow::Error> {
//     force(
//         || async { desk.sit().await },
//         |height| height < (AVG_MID_HEIGHT + AVG_SITTING_HEIGHT) / 2,
//         desk,
//     )
//         .await
// }
//
// async fn force_stand(desk: &Desk) -> Result<(), anyhow::Error> {
//     force(
//         || async { desk.stand().await },
//         |height| height > (AVG_MID_HEIGHT + AVG_STANDING_HEIGHT) / 2,
//         desk,
//     )
//         .await
// }
//
// async fn force<AFut>(
//     mut action: impl FnMut() -> AFut,
//     mut done: impl FnMut(isize) -> bool,
//     desk: &Desk,
// ) -> Result<(), anyhow::Error>
// where
//     AFut: Future<Output = Result<(), anyhow::Error>>,
// {
//     let mut attempts = 0;
//     let mut previous_height = desk.query_height().await?;
//
//     while attempts < FORCE_ATTEMPTS {
//         attempts += 1;
//         log::trace!("Running forced attempt {attempts}");
//         action().await?;
//
//         'query_height: loop {
//             time::sleep(Duration::from_millis(1000)).await;
//             let next_height = desk.height();
//             log::trace!("Height moved from: {previous_height} -> {next_height}");
//
//             // we've stopped moving so check our height
//             if previous_height == next_height {
//                 if done(next_height) {
//                     return Ok(());
//                 } else {
//                     break 'query_height;
//                 }
//             }
//             previous_height = next_height;
//         }
//     }
//
//     Err(anyhow!(
//         "Failed to force the desk to the intended height after {attempts} attempts"
//     ))
// }

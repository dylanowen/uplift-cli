use anyhow::{Context, anyhow};
use btleplug::api::Manager as _;
use btleplug::platform::{Adapter, Manager};
use clap::Parser;
use futures::StreamExt;
use log::trace;
use std::time::Duration;
use tokio::time::{Instant, sleep, timeout, timeout_at};
use uplift_cli::{Args, Commands, Config, format_height, load_conf, save_conf};
use uplift_lib::{Command, UpliftDeskScanner};

use uom::si::f32::Length;
use uom::si::length::inch;
use uplift_lib::{UpliftDesk, UpliftDeskId};
use uuid::Uuid;

const FORCE_ATTEMPTS: usize = 20;
const SCAN_TIMEOUT: Duration = Duration::from_secs(2);
const PACKET_DELAY: Duration = Duration::from_millis(200);

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let args = Args::parse();
    setup_logging(&args)?;

    let conf = load_conf()?;

    let timeout_duration = Duration::from_secs(args.timeout);

    let runner = run_command(conf, args);
    if !timeout_duration.is_zero() {
        timeout(timeout_duration, runner)
            .await
            .context("Operation timed out")??;
    } else {
        runner.await?;
    }

    Ok(())
}

async fn run_command(mut conf: Config, args: Args) -> Result<(), anyhow::Error> {
    let manager = Manager::new().await?;

    let adapters = manager.adapters().await?;
    let adapter = adapters
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("No adapters found"))?;

    let mut desk = choose_desk(adapter.clone(), args.desk.as_deref()).await?;

    desk.connect().await?;

    match args.command {
        Commands::Sit { save } => {
            if save.is_some() {
                desk.command(Command::SaveSit).await?;

                // let the packet actually send
                sleep(PACKET_DELAY).await;

                let height = desk.height().await?;
                conf.sit_height = height;
                save_conf(&conf)?;

                println!("Saved sitting height @ {:.1}", format_height(height));
            } else {
                desk.command(Command::GotoSit).await?;

                // let the packet actually send
                sleep(PACKET_DELAY).await;
            }
        }
        Commands::Stand { save } => {
            if save.is_some() {
                desk.command(Command::SaveStand).await?;

                // let the packet actually send
                sleep(PACKET_DELAY).await;

                let height = desk.height().await?;
                conf.stand_height = height;
                save_conf(&conf)?;

                println!("Saved standing height @ {:.1}", format_height(height));
            } else {
                desk.command(Command::GotoStand).await?;

                // let the packet actually send
                sleep(PACKET_DELAY).await;
            }
        }
        Commands::ForceSit => {
            force(Command::GotoSit, conf.sit_height, &mut desk).await?;
        }
        Commands::ForceStand => {
            force(Command::GotoStand, conf.stand_height, &mut desk).await?;
        }
        Commands::Query => {
            let height = desk.height().await?;
            let rssi = desk.rssi().await?;
            println!("height: {} rssi: {rssi:?}", format_height(height),);
        }
        Commands::Toggle => {
            let height = desk.height().await?;

            let mid = conf.stand_height + conf.sit_height / 2.;

            if height > mid {
                desk.command(Command::GotoSit).await?;
            } else {
                desk.command(Command::GotoStand).await?;
            }

            // let the packet actually send
            sleep(PACKET_DELAY).await;
        }
        Commands::ForceToggle => {
            let height = desk.height().await?;

            let mid = conf.stand_height + conf.sit_height / 2.;

            if height > mid {
                force(Command::GotoSit, conf.sit_height, &mut desk).await?;
            } else {
                force(Command::GotoStand, conf.stand_height, &mut desk).await?;
            }
        }
        Commands::Listen => {
            let mut heights = desk.height_stream().await?;
            loop {
                if let Some(height) = heights.next().await {
                    trace!("Height {:.1}", format_height(height));
                } else {
                    return Err(anyhow!("Height stream disconnected"));
                }
            }
        }
    }

    Ok(())
}

async fn force(command: Command, goal: Length, desk: &mut UpliftDesk) -> Result<(), anyhow::Error> {
    const HEIGHT_STREAM_TIMEOUT: Duration = Duration::from_secs(1);
    let height_tolerance: Length = Length::new::<inch>(1.);

    let mut attempts = 0;
    let mut previous_height = desk.height().await?;

    let mut heights = desk.height_stream().await?;
    while attempts < FORCE_ATTEMPTS {
        attempts += 1;
        trace!("Running forced attempt {attempts}");
        desk.command(command).await?;

        sleep(PACKET_DELAY).await;

        'query_height: loop {
            if let Ok(Some(next_height)) = timeout(HEIGHT_STREAM_TIMEOUT, heights.next()).await {
                trace!(
                    "Height moved from: {:.1} -> {:.1}",
                    format_height(previous_height),
                    format_height(next_height)
                );

                println!("{}", previous_height == next_height);

                if previous_height == next_height {
                    // we've stopped moving so check our height
                    if (goal - next_height).abs() <= height_tolerance {
                        return Ok(());
                    } else {
                        break 'query_height;
                    }
                } else {
                    previous_height = next_height;
                }
            } else {
                break 'query_height;
            }
        }
    }

    desk.command(Command::GotoSit).await?;

    Err(anyhow!(
        "Failed to force the desk to the intended height after {attempts} attempts"
    ))
}

async fn choose_desk(
    adapter: Adapter,
    desk_uuid: Option<&str>,
) -> Result<UpliftDesk, anyhow::Error> {
    let mut scanner = UpliftDeskScanner::new(adapter).await?;
    let deadline = Instant::now() + SCAN_TIMEOUT;

    if let Some(desk) = desk_uuid {
        let desk_id =
            UpliftDeskId::from(Uuid::parse_str(desk).with_context(|| "Invalid UUID for desk")?);

        loop {
            match timeout_at(deadline, scanner.next()).await {
                Ok(Some(desk)) if desk_id == desk.id() => {
                    return Ok(desk);
                }
                Ok(Some(_)) => continue,
                _ => {
                    return Err(anyhow!("Timed out waiting for desk: {desk_id:?}"));
                }
            }
        }
    } else {
        let mut desks = Vec::new();
        while let Ok(Some(desk)) = timeout_at(deadline, scanner.next()).await {
            desks.push(desk)
        }
        desks.dedup_by_key(|d| d.id());

        if desks.len() > 1 {
            println!("{:?}", desks.iter().map(|d| d.id()).collect::<Vec<_>>());
            Ok(desks.into_iter().next().unwrap())
        } else {
            desks
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("No desk found"))
        }
    }
}

fn setup_logging(args: &Args) -> Result<(), anyhow::Error> {
    let mut builder = env_logger::Builder::new();
    builder.parse_filters(&args.log_level);
    builder.filter_module("uplift_lib", log::LevelFilter::Trace);

    if let Some(s) = &args.log_style {
        builder.parse_write_style(s);
    }

    builder.try_init().context("Failed to setup logger")
}

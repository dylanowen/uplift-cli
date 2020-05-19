#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
#[macro_use]
extern crate objc;

use core::fmt;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};

use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use tokio::time;

use crate::bluetooth::BluetoothError;
use crate::bluetooth::UUID;
use crate::desk::Desk;
use std::time::Duration;
use tokio::time::timeout;

mod bluetooth;
mod desk;
mod group;

lazy_static! {
    pub static ref DESK_SERVICE_UUID: UUID = UUID::parse("ff12").unwrap();
    pub static ref DESK_DATA_IN: UUID = UUID::parse("ff01").unwrap();
    pub static ref DESK_DATA_OUT: UUID = UUID::parse("ff02").unwrap();
    pub static ref DESK_NAME: UUID = UUID::parse("ff06").unwrap();
}

#[tokio::main]
async fn main() -> Result<(), UpliftError> {
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
        .subcommand(SubCommand::with_name("listen"))
        .subcommand(SubCommand::with_name("set").arg(Arg::with_name("height").required(true)))
        .subcommand(SubCommand::with_name("sit").arg(Arg::with_name("store")))
        .subcommand(SubCommand::with_name("stand").arg(Arg::with_name("store")))
        .subcommand(SubCommand::with_name("query"))
        .get_matches();

    setup_logging(&matches)?;

    let mut desk = Desk::new().await?;

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

                time::delay_for(Duration::from_millis(100)).await;
            }
        }
        ("set", Some(sub_matches)) => {
            let height = sub_matches
                .value_of("height")
                .unwrap()
                .parse::<isize>()
                .map_err::<UpliftError, _>(|e| format!("Couldn't parse height: {}", e).into())?;

            if timeout(Duration::from_secs(45), desk.set_height(height))
                .await
                .is_err()
            {
                warn!("Ran out of time to move the desk")
            }
        }
        ("sit", Some(sub_matches)) => {
            if sub_matches.value_of("store").is_some() {
                desk.save_sit().await?;
            } else {
                desk.sit().await?;
            }
            time::delay_for(Duration::from_millis(100)).await;
        }
        ("stand", Some(sub_matches)) => {
            if sub_matches.value_of("store").is_some() {
                desk.save_stand().await?;
            } else {
                desk.stand().await?;
            }
            time::delay_for(Duration::from_millis(100)).await;
        }
        ("query", _) => {
            // wait for our height to load
            while desk.height() <= 0 {
                desk.query().await?;
                time::delay_for(Duration::from_millis(100)).await;
            }
            println!("{}", desk.height());
        }
        _ => unreachable!(),
    }

    // todo why does drop cause so many objc seg faults?
    drop(desk);

    Ok(())
}

fn setup_logging(matches: &ArgMatches) -> Result<(), UpliftError> {
    let log_level = matches.value_of("log-level").unwrap();
    let log_style = matches.value_of("log-style");

    let mut builder = env_logger::Builder::new();
    builder.parse_filters(log_level);

    if let Some(s) = log_style {
        builder.parse_write_style(&s);
    }

    builder
        .try_init()
        .map_err(|e| format!("Failed to setup logger: {}", e).into())
}

#[derive(Debug)]
pub struct UpliftError(String);

impl UpliftError {
    fn new<S: Into<String>>(message: S) -> UpliftError {
        UpliftError(message.into())
    }
}

impl Display for UpliftError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Error for UpliftError {}
//
// impl From<BTError> for UpliftError {
//     fn from(e: BTError) -> Self {
//         UpliftError(format!("{}", e))
//     }
// }

impl From<String> for UpliftError {
    fn from(s: String) -> Self {
        UpliftError(s)
    }
}

impl From<&str> for UpliftError {
    fn from(s: &str) -> Self {
        s.to_string().into()
    }
}

impl From<BluetoothError> for UpliftError {
    fn from(error: BluetoothError) -> Self {
        UpliftError(format!("{}", error))
    }
}

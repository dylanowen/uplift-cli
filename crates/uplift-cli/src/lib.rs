use clap::{Parser, Subcommand};
use log::debug;
use serde::{Deserialize, Serialize};
use std::any::type_name;
use std::fmt::{Debug, Formatter};
use uom::fmt::DisplayStyle;
use uom::si::SI;
use uom::si::f32::Length;
use uom::si::fmt::QuantityArguments;
use uom::si::length::{Dimension, inch, pica_computer};

const CONF_NAME: &str = env!("CARGO_PKG_NAME");

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
#[clap(propagate_version = true)]
pub struct Args {
    #[clap(subcommand)]
    pub command: Commands,
    /// Set the timeout in seconds, 0 for infinite
    #[clap(long, default_value_t = 60)]
    pub timeout: u64,
    /// Choose the desk to interact with
    #[clap(long)]
    pub desk: Option<String>,
    /// Set the environment log level
    #[clap(long, env = env_logger::DEFAULT_FILTER_ENV, default_value_t = String::from("info"))]
    pub log_level: String,
    /// Set the environment log style
    #[clap(long, env = env_logger::DEFAULT_WRITE_STYLE_ENV)]
    pub log_style: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Get the estimated desk height in inches
    Query,
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
    /// Sit -> Stand or Stand -> Sit
    Toggle,
    /// Retry the Sit operation 5 times if the desk doesn't complete it
    ForceSit,
    /// Retry the Stand operation 5 times if the desk doesn't complete it
    ForceStand,
    /// Retry the Toggle operation 5 times if the desk doesn't complete it
    ForceToggle,
    /// Listen for height changes
    Listen,
}

#[derive(Subcommand, Debug)]
pub enum SaveCommand {
    Save,
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub sit_height: Length,
    pub stand_height: Length,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            sit_height: Length::new::<inch>(26.),
            stand_height: Length::new::<inch>(42.),
        }
    }
}

impl Debug for Config {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(type_name::<Self>())
            .field("sit_height", &format_height(self.sit_height))
            .field("stand_height", &format_height(self.stand_height))
            .finish()
    }
}

pub fn load_conf() -> Result<Config, anyhow::Error> {
    let conf = confy::load(CONF_NAME, None)?;

    debug!(
        "Loaded config @ {:?}: {conf:#?}",
        confy::get_configuration_file_path(CONF_NAME, None)?
    );

    Ok(conf)
}

pub fn save_conf(conf: &Config) -> Result<(), anyhow::Error> {
    confy::store(CONF_NAME, None, conf)?;

    debug!(
        "Saved config @ {:?}: {conf:#?}",
        confy::get_configuration_file_path(CONF_NAME, None)?
    );

    Ok(())
}

pub fn format_height(length: Length) -> QuantityArguments<Dimension, SI<f32>, f32, inch> {
    length
        .trunc::<pica_computer>()
        .into_format_args(inch, DisplayStyle::Abbreviation)
}

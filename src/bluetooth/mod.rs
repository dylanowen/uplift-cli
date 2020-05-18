use core::fmt;
use std::error::Error;
use std::fmt::{Display, Formatter};

pub use self::uuid::*;
pub use central_manager::*;
pub use characteristic::*;
pub use peripheral::*;

mod central_manager;
mod characteristic;
mod delegate;
mod peripheral;
mod service;
mod uuid;

mod utils;

#[derive(Debug)]
pub struct BluetoothError(String);

// impl BluetoothError {
//     fn new<S: Into<String>>(message: S) -> BluetoothError {
//         BluetoothError(message.into())
//     }
//}

impl Display for BluetoothError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Error for BluetoothError {}

// impl From<BTError> for UpliftError {
//     fn from(e: BTError) -> Self {
//         UpliftError(format!("{}", e))
//     }
// }

impl From<String> for BluetoothError {
    fn from(s: String) -> Self {
        BluetoothError(s)
    }
}

impl From<&str> for BluetoothError {
    fn from(s: &str) -> Self {
        s.to_string().into()
    }
}

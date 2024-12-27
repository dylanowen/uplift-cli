use std::result;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Bluetooth Error: {0}")]
    BluetoothError(btleplug::Error)
}

pub type Result<T> = result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_sync() {
        fn ensure_send_sync<T: Send + Sync>(t: T) {}

        ensure_send_sync(Error::BluetoothError(btleplug::Error::DeviceNotFound))
    }
}
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;

use core::fmt;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::str::FromStr;
use std::thread;
use std::time::Duration;

use btleplug::api::{Central, Characteristic};
use btleplug::api::{Peripheral as ApiPeripheral, UUID};
use btleplug::corebluetooth::adapter::Adapter;
use btleplug::corebluetooth::manager::Manager;
use btleplug::corebluetooth::peripheral::Peripheral;
use btleplug::Error as BTError;
use env_logger::Env;

const QUERY_PACKET: [u8; 6] = [0xf1, 0xf1, 0x07, 0x00, 0x07, 0x7e];
const UP_PACKET: [u8; 6] = [0xf1, 0xf1, 0x01, 0x00, 0x01, 0x7e];
const DOWN_PACKET: [u8; 6] = [0xf1, 0xf1, 0x02, 0x00, 0x02, 0x7e];

lazy_static! {
    static ref DATA_IN_UUID: UUID =
        UUID::from_str("00:00:ff:01:00:00:10:00:80:00:00:80:5F:9B:34:FB").unwrap();
    static ref DATA_OUT_UUID: UUID =
        UUID::from_str("00:00:ff:02:00:00:10:00:80:00:00:80:5F:9B:34:FB").unwrap();
    static ref NAME_UUID: UUID =
        UUID::from_str("00:00:ff:06:00:00:10:00:80:00:00:80:5F:9B:34:FB").unwrap();
}

fn main() -> Result<(), UpliftError> {
    env_logger::init_from_env(Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "debug"));

    //let devices = bt::discover_devices()?;
    let manager = Manager::new()?;

    // get the first bluetooth adapter
    let adapter = manager.adapters()?.into_iter().nth(0).unwrap();

    adapter.start_scan()?;

    let desk = find_desk(&adapter, Duration::from_secs(20))?;

    //adapter.stop_scan();
    info!("Desk Connected!");

    desk.connect()?;

    let (data_in_characteristic, data_out_characteristic, name_characteristic) =
        get_characteristics(&desk)?;

    desk.command(&data_in_characteristic, &QUERY_PACKET)?;
    desk.command(&data_in_characteristic, &UP_PACKET)?;
    desk.command(&data_in_characteristic, &DOWN_PACKET)?;

    println!("{:?}", desk.address());
    println!("{:?}", desk.properties());
    println!("{:?}", desk.is_connected());
    println!("{:?}", desk.discover_characteristics());
    println!("{:?}", desk.characteristics());
    // println!("{:?}", desk.discover_characteristics())

    desk.disconnect()?;

    thread::sleep(Duration::from_secs(100));

    Ok(())
}

// TODO this should search based on services, not the name
fn find_desk(adapter: &Adapter, max_duration: Duration) -> Result<Peripheral, UpliftError> {
    let sleep_time = Duration::from_millis(100);
    let desk_name = Some("Office".to_string());

    let mut duration = Duration::from_secs(0);

    while duration < max_duration {
        duration += sleep_time;
        thread::sleep(sleep_time);

        for peripheral in adapter.peripherals().into_iter() {
            if peripheral.properties().local_name == desk_name {
                return Ok(peripheral);
            }
        }
    }

    Err("Couldn't connect to the desk".into())
}

fn get_characteristics(
    desk: &Peripheral,
) -> Result<(Characteristic, Characteristic, Characteristic), UpliftError> {
    let mut data_in_characteristic = None;
    let mut data_out_characteristic = None;
    let mut name_characteristic = None;

    for characteristic in desk.characteristics().into_iter() {
        if *DATA_IN_UUID == characteristic.uuid {
            data_in_characteristic = Some(characteristic);
        } else if *DATA_OUT_UUID == characteristic.uuid {
            data_out_characteristic = Some(characteristic);
        } else if *NAME_UUID == characteristic.uuid {
            name_characteristic = Some(characteristic);
        }
    }

    Ok((
        data_in_characteristic
            .ok_or_else(|| UpliftError::new("Couldn't get data-in characteristic"))?,
        data_out_characteristic
            .ok_or_else(|| UpliftError::new("Couldn't find data-out characteristic"))?,
        name_characteristic.ok_or_else(|| UpliftError::new("Couldn't find name characteristic"))?,
    ))
}

// fn parse_bluetooth_uuid<S: Into<String>>(s: S) -> Result<Uuid, UpliftError> {
//     let mut uuid = s.into();
//     if uuid.len() == 4 {
//         uuid = format!("0000{}", uuid)
//     }
//     if uuid.len() == 8 {
//         uuid = format!("{}-0000-1000-8000-00805f9b34fb", uuid)
//     }
//
//     return Uuid::parse_str(&uuid).map_err(|e| UpliftError(e.to_string()));
// }

#[derive(Debug)]
struct UpliftError(String);

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

impl From<BTError> for UpliftError {
    fn from(e: BTError) -> Self {
        UpliftError(format!("{}", e))
    }
}

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

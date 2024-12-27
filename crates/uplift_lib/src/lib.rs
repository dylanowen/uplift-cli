mod desk;
mod discovery;
mod error;
mod id;

use std::collections::BTreeSet;
use std::sync::atomic::AtomicIsize;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

pub use crate::desk::*;
pub use crate::discovery::DeskAdapter;
pub use crate::id::*;
use anyhow::{anyhow, Context, Result};
use btleplug::api::CentralEvent::{DeviceConnected, DeviceDiscovered, DeviceUpdated};
use btleplug::api::{
    bleuuid, Central, Characteristic, Manager as _, Peripheral as _, ScanFilter, ValueNotification,
    WriteType,
};
use btleplug::platform::{Manager, Peripheral, PeripheralId};
use futures::{executor, StreamExt};
use tokio::time;
use uuid::Uuid;

// const UP_PACKET: [u8; 6] = [0xf1, 0xf1, 0x01, 0x00, 0x01, 0x7e];
// const DOWN_PACKET: [u8; 6] = [0xf1, 0xf1, 0x02, 0x00, 0x02, 0x7e];
const SAVE_SIT_PACKET: [u8; 6] = [0xf1, 0xf1, 0x03, 0x00, 0x03, 0x7e];
const SAVE_STAND_PACKET: [u8; 6] = [0xf1, 0xf1, 0x04, 0x00, 0x04, 0x7e];
const SIT_PACKET: [u8; 6] = [0xf1, 0xf1, 0x05, 0x00, 0x05, 0x7e];
const STAND_PACKET: [u8; 6] = [0xf1, 0xf1, 0x06, 0x00, 0x06, 0x7e];
// const STOP_PACKET: [u8; 6] = [0xf1, 0xf1, 0x02, 0x00, 0x2b, 0x7e];
const QUERY_PACKET: [u8; 6] = [0xf1, 0xf1, 0x07, 0x00, 0x07, 0x7e];

pub const DESK_SERVICE_UUID: Uuid = bleuuid::uuid_from_u16(0xff12);

const DESK_DATA_IN_UUID: Uuid = bleuuid::uuid_from_u16(0xff01);
const DESK_DATA_OUT_UUID: Uuid = bleuuid::uuid_from_u16(0xff02);
const DESK_NAME_UUID: Uuid = bleuuid::uuid_from_u16(0xff06);

pub struct ConnectedUpliftDesk {
    height: Arc<AtomicIsize>,
    raw_height: Arc<(AtomicU8, AtomicU8)>,
    data_in_characteristic: Characteristic,
    peripheral: Peripheral,
    _manager: Manager,
}

impl ConnectedUpliftDesk {
    pub async fn new() -> Result<ConnectedUpliftDesk> {
        log::debug!("Connecting to Bluetooth Manager");
        let manager = Manager::new().await?;

        let adapters = manager.adapters().await?;
        let adapter = adapters
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("Couldn't find an adapter"))?;

        // Grab the first desk
        let peripheral = adapter
            .scan_for_desks()
            .await
            .recv()
            .await
            .expect("Scanner unexpectedly stopped")?;
        // let peripheral = adapter.get_desk_peripheral(Uuid::parse_str("0ab28845-04db-6dd1-ddf7-58a1b26ccf31").unwrap()).await?;

        log::debug!("{} - Attempting to connect", peripheral.id());

        peripheral
            .connect()
            .await
            .context(format!("{} - Connection failed", peripheral.id()))?;

        log::debug!("{} - Connected to peripheral", peripheral.id());

        // start discovering characteristics on our peripheral
        peripheral
            .discover_services()
            .await
            .with_context(|| format!("{} - Discovering Services", peripheral.id()))?;

        let (data_in_characteristic, data_out_characteristic, _name_characteristic) =
            get_characteristics(peripheral.characteristics())?;

        let height = Arc::new(AtomicIsize::new(-1));
        let raw_height = Arc::new((AtomicU8::new(0), AtomicU8::new(0)));

        // subscribe to events (height) on our peripheral
        {
            let updated_height = height.clone();
            let updated_raw_height = raw_height.clone();

            let mut height_receiver = peripheral.notifications().await?;
            peripheral
                .subscribe(&data_out_characteristic)
                .await
                .with_context(|| format!("{} - Subscribing to desk updates", peripheral.id()))?;

            let id = peripheral.id();
            tokio::spawn(async move {
                while let Some(ValueNotification { value, .. }) = height_receiver.next().await {
                    let last_height = updated_height.load(Ordering::Relaxed);
                    let (low, high) = get_raw_height(&value);
                    let height = estimate_height((low, high), last_height);

                    log::trace!(
                        "{:?} - Updated Height: ({:x},{:x}) -> {:x}",
                        id,
                        low,
                        high,
                        height
                    );
                    updated_height.store(height, Ordering::Relaxed);
                    updated_raw_height.0.store(low, Ordering::Relaxed);
                    updated_raw_height.1.store(high, Ordering::Relaxed);
                }
            });
        }

        let desk = ConnectedUpliftDesk {
            height,
            raw_height,
            data_in_characteristic,
            peripheral,
            _manager: manager,
        };

        // we need to do an initial query to actually write anything, so just get that out of the way
        desk.write(&desk.data_in_characteristic, &QUERY_PACKET)
            .await?;

        Ok(desk)
    }

    pub fn height(&self) -> isize {
        self.height.load(Ordering::Relaxed)
    }

    pub fn raw_height(&self) -> (u8, u8) {
        (
            self.raw_height.0.load(Ordering::Relaxed),
            self.raw_height.1.load(Ordering::Relaxed),
        )
    }

    pub async fn save_sit(&self) -> Result<(), anyhow::Error> {
        log::debug!("{} - Save sit", self.peripheral.id());

        self.write(&self.data_in_characteristic, &SAVE_SIT_PACKET)
            .await
            .with_context(|| format!("{} - Saving Sit", self.peripheral.id()))
    }

    pub async fn save_stand(&self) -> Result<(), anyhow::Error> {
        log::debug!("{} - Save stand", self.peripheral.id());

        self.write(&self.data_in_characteristic, &SAVE_STAND_PACKET)
            .await
            .with_context(|| format!("{} - Saving Stand", self.peripheral.id()))
    }

    pub async fn sit(&self) -> Result<(), anyhow::Error> {
        log::debug!("{} - Sit", self.peripheral.id());

        self.write(&self.data_in_characteristic, &SIT_PACKET)
            .await
            .with_context(|| format!("{} - Sitting", self.peripheral.id()))
    }

    pub async fn stand(&self) -> Result<(), anyhow::Error> {
        log::debug!("{} - Stand", self.peripheral.id());

        self.write(&self.data_in_characteristic, &STAND_PACKET)
            .await
            .with_context(|| format!("{} - Standing", self.peripheral.id()))
    }

    pub async fn query_height(&self) -> Result<isize, anyhow::Error> {
        // since we're querying, clear our height so we can check if it's updated
        self.height.store(-1, Ordering::Relaxed);
        self.write(&self.data_in_characteristic, &QUERY_PACKET)
            .await
            .with_context(|| format!("{} - Querying", self.peripheral.id()))?;

        // wait for our height to update (is there a better way than polling?)
        while self.height.load(Ordering::Relaxed) <= 0 {
            time::sleep(Duration::from_millis(100)).await;
        }

        Ok(self.height.load(Ordering::Relaxed))
    }

    async fn write(
        &self,
        characteristic: &Characteristic,
        data: &[u8],
    ) -> Result<(), anyhow::Error> {
        self.peripheral
            .write(characteristic, data, WriteType::WithoutResponse)
            .await
            .with_context(|| format!("{} - Failed to write data", self.peripheral.id()))
    }
}

fn get_raw_height(data: &[u8]) -> (u8, u8) {
    (data[5], data[7])
}

// 25.2"
pub const MIN_PHYSICAL_HEIGHT: isize = 252;
// 25.2" + 0xff
pub const MAX_PHYSICAL_HEIGHT: isize = MIN_PHYSICAL_HEIGHT + 0xff;
pub const MID_PHYSICAL_HEIGHT: isize = (MIN_PHYSICAL_HEIGHT + MAX_PHYSICAL_HEIGHT) / 2;
// 26.0" based on a 5'6" person
pub const AVG_SITTING_HEIGHT: isize = 260;
// 40.5" based on a 5'6" person
pub const AVG_STANDING_HEIGHT: isize = 405;
pub const AVG_MID_HEIGHT: isize = (AVG_SITTING_HEIGHT + AVG_STANDING_HEIGHT) / 2;

/// The height ranges from 0x00 to 0xff. 0x01 roughly seems to be 0.1"
fn estimate_height((low, high): (u8, u8), last_height: isize) -> isize {
    // TODO https://github.com/justintout/uplift-reconnect/blob/master/lib/ble.dart#L167

    let low = low as isize;
    let high = high as isize;

    let raw_height = if low >= 0xfd {
        // anything outside of this range seems to be "special"
        if last_height < MID_PHYSICAL_HEIGHT {
            high
        } else {
            low
        }
    } else {
        low
    };

    MIN_PHYSICAL_HEIGHT + raw_height
}

impl Drop for ConnectedUpliftDesk {
    fn drop(&mut self) {
        executor::block_on(self.peripheral.disconnect()).unwrap();
    }
}

fn get_characteristics(
    characteristics: BTreeSet<Characteristic>,
) -> anyhow::Result<(Characteristic, Characteristic, Characteristic)> {
    let mut data_in_characteristic = None;
    let mut data_out_characteristic = None;
    let mut name_characteristic = None;

    for characteristic in characteristics.into_iter() {
        if DESK_DATA_IN_UUID == characteristic.uuid {
            data_in_characteristic = Some(characteristic);
        } else if DESK_DATA_OUT_UUID == characteristic.uuid {
            data_out_characteristic = Some(characteristic);
        } else if DESK_NAME_UUID == characteristic.uuid {
            name_characteristic = Some(characteristic);
        }
    }

    Ok((
        data_in_characteristic.ok_or_else(|| anyhow!("Couldn't get data-in characteristic"))?,
        data_out_characteristic.ok_or_else(|| anyhow!("Couldn't find data-out characteristic"))?,
        name_characteristic.ok_or_else(|| anyhow!("Couldn't find name characteristic"))?,
    ))
}

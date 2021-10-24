use btleplug::api::{
    bleuuid, Central, Characteristic, Manager as _, Peripheral as _, ScanFilter, ValueNotification,
    WriteType,
};
use btleplug::platform::{Manager, Peripheral};

use std::collections::BTreeSet;
use std::sync::atomic::AtomicIsize;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
// use btleplug::api::{Central, Manager, Peripheral};

use anyhow::{anyhow, Context};
use btleplug::api::CentralEvent::{DeviceConnected, DeviceDiscovered, DeviceUpdated};
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

const MAX_MANAGER_RETRIES: usize = 5;
const MAX_CONNECTION_ATTEMPTS: usize = 5;

pub struct Desk {
    // rssi: i64,
    height: Arc<AtomicIsize>,
    raw_height: Arc<(AtomicU8, AtomicU8)>,
    data_in_characteristic: Characteristic,
    peripheral: Peripheral,
    _manager: Manager,
}

impl Desk {
    pub async fn new() -> Result<Desk, anyhow::Error> {
        // let manager = Manager::new().await?;
        let (manager, peripheral) = connect().await?;

        log::debug!("Connected to peripheral: {:?}", peripheral.id());

        // start discovering characteristics on our peripheral so we can `
        peripheral
            .discover_services()
            .await
            .context("Discovering Services")?;

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
                .context("Subscribing to desk updates")?;

            tokio::spawn(async move {
                while let Some(ValueNotification { value, .. }) = height_receiver.next().await {
                    let last_height = updated_height.load(Ordering::Relaxed);
                    let (low, high) = get_raw_height(&value);
                    let height = estimate_height((low, high), last_height);

                    log::trace!("Updated Height: ({:x},{:x}) -> {:x}", low, high, height);
                    updated_height.store(height, Ordering::Relaxed);
                    updated_raw_height.0.store(low, Ordering::Relaxed);
                    updated_raw_height.1.store(high, Ordering::Relaxed);
                }
            });
        }

        let desk = Desk {
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

    // pub fn rssi(&self) -> i64 {
    //     self.rssi
    // }
    //
    pub fn height(&self) -> isize {
        self.height.load(Ordering::Relaxed)
    }

    pub fn raw_height(&self) -> (u8, u8) {
        (
            self.raw_height.0.load(Ordering::Relaxed),
            self.raw_height.1.load(Ordering::Relaxed),
        )
    }

    // pub async fn move_up(&self) -> Result<(), anyhow::Error> {
    //     // debug!("Move up @ height {}", self.height());
    //
    //     self.write(&self.data_in_characteristic, &UP_PACKET).await?;
    //
    //     Ok(())
    // }
    //
    // pub async fn move_down(&self) -> Result<(), anyhow::Error> {
    //     // debug!("Move down @ height {}", self.height());
    //
    //     self.write(&self.data_in_characteristic, &DOWN_PACKET).await
    // }

    pub async fn save_sit(&self) -> Result<(), anyhow::Error> {
        log::debug!("Save sit");

        self.write(&self.data_in_characteristic, &SAVE_SIT_PACKET)
            .await
            .context("Saving Sit")
    }

    pub async fn save_stand(&self) -> Result<(), anyhow::Error> {
        log::debug!("Save stand");

        self.write(&self.data_in_characteristic, &SAVE_STAND_PACKET)
            .await
            .context("Saving Stand")
    }

    pub async fn sit(&self) -> Result<(), anyhow::Error> {
        log::debug!("Sit");

        self.write(&self.data_in_characteristic, &SIT_PACKET)
            .await
            .context("Sitting")
    }

    pub async fn stand(&self) -> Result<(), anyhow::Error> {
        log::debug!("Stand");

        self.write(&self.data_in_characteristic, &STAND_PACKET)
            .await
            .context("Standing")
    }

    pub async fn query_height(&self) -> Result<isize, anyhow::Error> {
        // since we're querying, clear our height so we can check if it's updated
        self.height.store(0, Ordering::Relaxed);
        self.write(&self.data_in_characteristic, &QUERY_PACKET)
            .await
            .context("Querying")?;

        // wait for our height to update (is there a better way than polling?)
        while self.height.load(Ordering::Relaxed) <= 0 {
            time::sleep(Duration::from_millis(100)).await;
        }

        Ok(self.height.load(Ordering::Relaxed))
    }

    // fn read(&self, characteristic: &Characteristic) -> Result<(), UpliftError> {
    //     self.peripheral.on_notification()
    //
    //     self.peripheral.read(characteristic)?;
    //     Ok(())
    // }

    async fn write(
        &self,
        characteristic: &Characteristic,
        data: &[u8],
    ) -> Result<(), anyhow::Error> {
        self.peripheral
            .write(characteristic, data, WriteType::WithoutResponse)
            .await
            .with_context(|| format!("Failed to write data to {:?}", self.peripheral.id()))
    }
}

fn get_raw_height(data: &[u8]) -> (u8, u8) {
    (data[5], data[7])
}

fn estimate_height((low, high): (u8, u8), last_height: isize) -> isize {
    let mut low = low as isize;
    let mut high = high as isize;

    if low > high {
        if last_height < 0xff {
            // we're probably low so go below 0
            low -= 0xff;
        } else {
            // we're probably high so go above 0xff
            high += 0xff;
        }
    }

    low + high
}

impl Drop for Desk {
    fn drop(&mut self) {
        executor::block_on(self.peripheral.disconnect()).unwrap();
    }
}

// async fn connect() -> Result<(Manager, Peripheral), anyhow::Error> {
//     let mut connection_attempt = 0;
//
//     let mut result = Err(anyhow!("Initializing Error"));
//     while result.is_err() && connection_attempt < MAX_CONNECTION_ATTEMPTS {
//         connection_attempt += 1;
//         let manager = Manager::new().await?;
//
//         let adapters = manager.adapters().await?;
//         let central = adapters
//             .into_iter()
//             .next()
//             .ok_or_else(|| anyhow!("Couldn't find an adapter"))?;
//
//         log::debug!("Using adapter: {:?}", central.adapter_info().await?);
//
//         // scan for our desk service
//         central
//             .start_scan(ScanFilter {
//                 services: vec![DESK_SERVICE_UUID],
//             })
//             .await?;
//
//         // try to find services for 10 seconds
//         for _ in 0..100 {
//             time::sleep(Duration::from_millis(100)).await;
//
//             if let Some(peripheral) = central.peripherals().await?.into_iter().next() {
//                 log::debug!("Found a desk: {:?}", peripheral.id());
//
//                 result = peripheral
//                     .connect()
//                     .await
//                     .map(|_| (manager, peripheral))
//                     .context("Failed to connect to peripheral");
//
//                 if let Err(error) = &result {
//                     log::warn!(
//                         "Connection attempt {} failed: {:?}",
//                         connection_attempt,
//                         error
//                     );
//                 }
//
//                 break;
//             }
//         }
//
//         central.stop_scan().await?;
//     }
//
//     result
// }

async fn connect() -> Result<(Manager, Peripheral), anyhow::Error> {
    let mut manager_attempts = 0;

    let mut result = Err(anyhow!("Initializing Error"));
    while result.is_err() && manager_attempts < MAX_MANAGER_RETRIES {
        manager_attempts += 1;

        log::debug!("Connecting to Bluetooth Manager");
        let manager = Manager::new().await?;

        let adapters = manager.adapters().await?;
        let central = adapters
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("Couldn't find an adapter"))?;

        log::debug!("Using adapter: {:?}", central.adapter_info().await?);

        let mut events = central.events().await?;

        // scan for our desk service
        central
            .start_scan(ScanFilter {
                services: vec![DESK_SERVICE_UUID],
            })
            .await?;

        let mut connection_attempt = 0;
        while connection_attempt <= MAX_CONNECTION_ATTEMPTS {
            match events.next().await {
                Some(DeviceDiscovered(id))
                | Some(DeviceUpdated(id))
                | Some(DeviceConnected(id)) => {
                    connection_attempt += 1;

                    let peripheral = central.peripheral(&id).await?;

                    log::debug!("Discovered peripheral {:?}", peripheral.id());

                    match peripheral.connect().await {
                        Ok(_) => {
                            result = Ok((manager, peripheral));
                            break;
                        }
                        Err(error) => log::warn!(
                            "Connection attempt {} failed: {:?}",
                            connection_attempt,
                            error
                        ),
                    }
                }
                Some(event) => log::trace!("{:?}", event),
                None => {
                    result = Err(anyhow!("Our adapter stopped looking for peripherals"));
                    break;
                }
            }
        }

        central.stop_scan().await?;
    }

    result
}

fn get_characteristics(
    characteristics: BTreeSet<Characteristic>,
) -> Result<(Characteristic, Characteristic, Characteristic), anyhow::Error> {
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

#[cfg(test)]
mod test {
    use super::*;
}

use std::collections::HashMap;
use std::sync::atomic::AtomicIsize;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering;

use futures::StreamExt;
use tokio::time;

use crate::bluetooth::{Advertisement, CentralManager, CentralManagerEvent, Delegated, Peripheral};
use crate::bluetooth::{Characteristic, UUID};
use crate::UpliftError;
use core::cmp;
use std::sync::Arc;
use std::time::Duration;

const QUERY_PACKET: [u8; 6] = [0xf1, 0xf1, 0x07, 0x00, 0x07, 0x7e];
const UP_PACKET: [u8; 6] = [0xf1, 0xf1, 0x01, 0x00, 0x01, 0x7e];
const DOWN_PACKET: [u8; 6] = [0xf1, 0xf1, 0x02, 0x00, 0x02, 0x7e];

lazy_static! {
    pub static ref DESK_SERVICE_UUID: UUID = UUID::parse("ff12").unwrap();
    pub static ref DESK_DATA_IN: UUID = UUID::parse("ff01").unwrap();
    pub static ref DESK_DATA_OUT: UUID = UUID::parse("ff02").unwrap();
    pub static ref DESK_NAME: UUID = UUID::parse("ff06").unwrap();
}

pub struct Desk {
    data_in_characteristic: Characteristic,
    data_out_characteristic: Characteristic,
    _name_characteristic: Characteristic,
    manager: CentralManager,
    peripheral: Peripheral<Delegated>,
    raw_height: Arc<(AtomicU8, AtomicU8)>,
    height: Arc<AtomicIsize>,
}

impl Desk {
    pub async fn new() -> Result<Desk, UpliftError> {
        let (manager, mut manager_receiver) = CentralManager::new();
        manager.start_scan(vec![DESK_SERVICE_UUID.clone()]);

        let mut peripheral;
        loop {
            match manager_receiver.next().await {
                Some(CentralManagerEvent::PeripheralDiscovered(p, adv, rssi))
                    if adv.contains(&Advertisement::Connectable(true)) =>
                {
                    debug!("Discovered peripheral {}, {}rssi", p, rssi);
                    peripheral = p;
                    break;
                }
                _ => (), // noop
            }
        }

        manager.stop_scan();
        manager.connect(&peripheral);

        loop {
            if let Some(CentralManagerEvent::PeripheralConnected(p)) = manager_receiver.next().await
            {
                debug!("Connected to peripheral {}", p);
                peripheral = p;
                break;
            }
        }

        let (mut peripheral, mut characteristic_receiver) = peripheral.with_delegate();

        let mut service_uuids = HashMap::new();
        service_uuids.insert(
            DESK_SERVICE_UUID.clone(),
            vec![
                DESK_DATA_IN.clone(),
                DESK_DATA_OUT.clone(),
                DESK_NAME.clone(),
            ],
        );
        let service = peripheral
            .discover_services(service_uuids)
            .await
            .pop()
            .expect("We should have found a service");

        let (data_in_characteristic, data_out_characteristic, name_characteristic) =
            get_characteristics(service.characteristics())?;

        let height = Arc::new(AtomicIsize::new(-1));
        let raw_height = Arc::new((AtomicU8::new(0), AtomicU8::new(0)));
        let updated_height = height.clone();
        let updated_raw = raw_height.clone();
        tokio::spawn(async move {
            while let Some((_, data)) = characteristic_receiver.next().await {
                let last_height = updated_height.load(Ordering::Relaxed);
                let (low, high) = get_raw_height(&data);
                let height = estimate_height((low, high), last_height);

                trace!("Updated Height: ({:x},{:x}) -> {:x}", low, high, height);
                updated_height.store(height, Ordering::Relaxed);
                updated_raw.0.store(low, Ordering::Relaxed);
                updated_raw.1.store(high, Ordering::Relaxed);
            }
        });

        let mut desk = Desk {
            data_in_characteristic,
            data_out_characteristic,
            _name_characteristic: name_characteristic,
            manager,
            peripheral,
            height,
            raw_height,
        };

        desk.peripheral.subscribe(&desk.data_out_characteristic);

        // just connected so send an initial query
        desk.query().await?;

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

    pub async fn set_height(&mut self, height: isize) -> Result<(), UpliftError> {
        // let mut last_height = self.height();
        // wait till we know a height to start our move
        // while last_height < 0 {
        //     last_height = self.height();
        //     time::delay_for(Duration::from_millis(100)).await;
        // }

        loop {
            let last_height = self.height();
            let distance = (height - last_height).abs();
            let delay = move_delay(distance);
            debug!(
                "last-height: {} height: {} distance: {} delay: {:?}",
                last_height,
                height,
                height - last_height,
                delay
            );

            match height.cmp(&last_height) {
                cmp::Ordering::Equal => break,
                cmp::Ordering::Greater => self.move_up().await?,
                cmp::Ordering::Less => self.move_down().await?,
            }

            time::delay_for(delay).await;
        }

        Ok(())
    }

    pub async fn move_up(&mut self) -> Result<(), UpliftError> {
        debug!("Move up @ height {}", self.height());

        Self::write(
            &mut self.peripheral,
            &self.data_in_characteristic,
            &UP_PACKET,
        )
        .await?;

        // self.query().await?;
        // println!("{:?}", self.data_out_characteristic);

        Ok(())
    }

    pub async fn move_down(&mut self) -> Result<(), UpliftError> {
        debug!("Move down @ height {}", self.height());

        Self::write(
            &mut self.peripheral,
            &self.data_in_characteristic,
            &DOWN_PACKET,
        )
        .await
    }

    async fn query(&mut self) -> Result<(), UpliftError> {
        Self::write(
            &mut self.peripheral,
            &self.data_in_characteristic,
            &QUERY_PACKET,
        )
        .await
    }

    // fn read(&self, characteristic: &Characteristic) -> Result<(), UpliftError> {
    //     self.peripheral.on_notification()
    //
    //     self.peripheral.read(characteristic)?;
    //     Ok(())
    // }

    async fn write(
        peripheral: &mut Peripheral<Delegated>,
        characteristic: &Characteristic,
        data: &[u8],
    ) -> Result<(), UpliftError> {
        peripheral.write(characteristic, data).await;

        Ok(())
    }
}

/// Calculate how much extra delay we should add to our delay function when moving our desk, based on how close we are
fn move_delay(distance: isize) -> Duration {
    const SCALING_FACTOR: isize = 250;
    const BASE_DELAY: isize = 500;
    let millis = SCALING_FACTOR - distance;

    Duration::from_millis((millis.max(0) + BASE_DELAY) as u64)
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
        self.manager.disconnect(&self.peripheral);
    }
}

fn get_characteristics(
    characteristics: Vec<Characteristic>,
) -> Result<(Characteristic, Characteristic, Characteristic), UpliftError> {
    let mut data_in_characteristic = None;
    let mut data_out_characteristic = None;
    let mut name_characteristic = None;

    for characteristic in characteristics.into_iter() {
        if let Some(uuid) = characteristic.uuid() {
            if *DESK_DATA_IN == uuid {
                data_in_characteristic = Some(characteristic);
            } else if *DESK_DATA_OUT == uuid {
                data_out_characteristic = Some(characteristic);
            } else if *DESK_NAME == uuid {
                name_characteristic = Some(characteristic);
            }
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

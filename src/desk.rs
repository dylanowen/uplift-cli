use core::cmp;
use std::collections::HashMap;
use std::sync::atomic::AtomicIsize;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use tokio::time;

use crate::bluetooth::{
    Advertisement, CentralManager, CentralManagerEvent, Delegated, Peripheral, State,
};
use crate::bluetooth::{Characteristic, Uuid};
use crate::UpliftError;

const UP_PACKET: [u8; 6] = [0xf1, 0xf1, 0x01, 0x00, 0x01, 0x7e];
const DOWN_PACKET: [u8; 6] = [0xf1, 0xf1, 0x02, 0x00, 0x02, 0x7e];
const SAVE_SIT_PACKET: [u8; 6] = [0xf1, 0xf1, 0x03, 0x00, 0x03, 0x7e];
const SAVE_STAND_PACKET: [u8; 6] = [0xf1, 0xf1, 0x04, 0x00, 0x04, 0x7e];
const SIT_PACKET: [u8; 6] = [0xf1, 0xf1, 0x05, 0x00, 0x05, 0x7e];
const STAND_PACKET: [u8; 6] = [0xf1, 0xf1, 0x06, 0x00, 0x06, 0x7e];
// const STOP_PACKET: [u8; 6] = [0xf1, 0xf1, 0x02, 0x00, 0x2b, 0x7e];
const QUERY_PACKET: [u8; 6] = [0xf1, 0xf1, 0x07, 0x00, 0x07, 0x7e];

lazy_static! {
    pub static ref DESK_SERVICE_UUID: Uuid = Uuid::parse("ff12").unwrap();
    pub static ref DESK_DATA_IN: Uuid = Uuid::parse("ff01").unwrap();
    pub static ref DESK_DATA_OUT: Uuid = Uuid::parse("ff02").unwrap();
    pub static ref DESK_NAME: Uuid = Uuid::parse("ff06").unwrap();
}

pub struct Desk {
    rssi: i64,
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

        // make sure we're powered on before starting our scan
        loop {
            match manager_receiver.next().await {
                Some(CentralManagerEvent::StateUpdated(State::PoweredOn)) => {
                    debug!("Peripheral Powered On");
                    break;
                }
                event => debug!("{:?}", event), // noop
            }
        }

        manager.start_scan(vec![DESK_SERVICE_UUID.clone()]);

        let mut peripheral;
        let rssi;
        loop {
            match manager_receiver.next().await {
                Some(CentralManagerEvent::PeripheralDiscovered(p, adv, found_rssi))
                    if adv.contains(&Advertisement::Connectable(true)) =>
                {
                    debug!("Discovered peripheral {}, {}rssi", p, found_rssi);
                    peripheral = p;
                    rssi = found_rssi;

                    break;
                }
                event => debug!("{:?}", event), // noop
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
            rssi,
            data_in_characteristic,
            data_out_characteristic,
            _name_characteristic: name_characteristic,
            manager,
            peripheral,
            height,
            raw_height,
        };

        desk.peripheral.subscribe(&desk.data_out_characteristic);

        // just connected so ask for our height
        desk.query().await?;

        Ok(desk)
    }

    pub fn rssi(&self) -> i64 {
        self.rssi
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

    /// This function doesn't work that well, we spend too much back and forth trying to get the
    /// desk just right
    pub async fn set_height(&mut self, goal_height: isize) -> Result<(), UpliftError> {
        const DELAY: u64 = 675;

        let mut last_height = self.height();
        // wait till we know a height to start our move
        while last_height < 0 {
            last_height = self.height();
            time::delay_for(Duration::from_millis(100)).await;
        }

        let mut last_checked = Instant::now();
        let mut last_height = self.height();
        loop {
            let height = self.height();
            let distance_to_go = (goal_height - height).abs();
            let distance_moved = (height - last_height).abs();
            let now = Instant::now();
            let velocity =
                distance_moved as f64 / now.duration_since(last_checked).as_millis() as f64;

            last_checked = now;
            last_height = height;

            let will_travel = velocity * DELAY as f64;
            let will_overshoot = distance_to_go as f64 - will_travel < 0.0;

            trace!(
                "goal: {}\nheight: {}\nto_go: {}\nmoved: {}\nvelocity: {}\nwill_travel: {}\nwill_overshoot: {}",
                goal_height, last_height, distance_to_go, distance_moved, velocity, will_travel, will_overshoot
            );

            if !will_overshoot {
                match goal_height.cmp(&height) {
                    cmp::Ordering::Equal => break,
                    cmp::Ordering::Greater => self.move_up().await?,
                    cmp::Ordering::Less => self.move_down().await?,
                }
            } else {
                break;
            }

            time::delay_for(Duration::from_millis(DELAY)).await;
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

    pub async fn save_sit(&mut self) -> Result<(), UpliftError> {
        debug!("Save sit");

        Self::write(
            &mut self.peripheral,
            &self.data_in_characteristic,
            &SAVE_SIT_PACKET,
        )
        .await
    }

    pub async fn save_stand(&mut self) -> Result<(), UpliftError> {
        debug!("Save stand");

        Self::write(
            &mut self.peripheral,
            &self.data_in_characteristic,
            &SAVE_STAND_PACKET,
        )
        .await
    }

    pub async fn sit(&mut self) -> Result<(), UpliftError> {
        debug!("Sit");

        Self::write(
            &mut self.peripheral,
            &self.data_in_characteristic,
            &SIT_PACKET,
        )
        .await
    }

    pub async fn stand(&mut self) -> Result<(), UpliftError> {
        debug!("Stand");

        Self::write(
            &mut self.peripheral,
            &self.data_in_characteristic,
            &STAND_PACKET,
        )
        .await
    }

    pub async fn query(&mut self) -> Result<(), UpliftError> {
        // since we're querying, clear our height so we can check if it's updated
        self.height.store(0, Ordering::Relaxed);
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

#[cfg(test)]
mod test {
    use super::*;

    /// create and redrop our desk continuously, looking for invalid usages of objc
    #[tokio::test]
    async fn dropping() {
        for _ in 0..10 {
            let desk = Desk::new().await.unwrap();
            drop(desk);
        }
    }
}

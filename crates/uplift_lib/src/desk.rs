use btleplug::api::{
    bleuuid, Central, Characteristic, Peripheral as _, PeripheralProperties, ValueNotification,
    WriteType,
};
use btleplug::platform::{Adapter, Peripheral, PeripheralId};
use btleplug::{Error, Result};
use std::collections::BTreeSet;

use crate::id::UpliftDeskId;
use crate::{
    estimate_height, get_raw_height, DESK_DATA_IN_UUID, DESK_DATA_OUT_UUID, DESK_NAME_UUID,
    DESK_SERVICE_UUID, MID_PHYSICAL_HEIGHT, MIN_PHYSICAL_HEIGHT, QUERY_PACKET,
};
use anyhow::anyhow;
use futures::StreamExt;
use std::ops::Deref;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tokio::select;
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::sync::{broadcast, mpsc, RwLock};
use tokio::time::sleep;
use uuid::Uuid;

pub struct UpliftDesk {
    properties: PeripheralProperties,
    peripheral: Peripheral,
    data_in_characteristic: Characteristic,
    data_out_characteristic: Characteristic,
    name_characteristic: Characteristic,
    height_stream: Arc<RwLock<Option<Sender<UpliftDeskHeight>>>>,
}

impl UpliftDesk {
    pub async fn new<I>(id: I, adapter: &Adapter) -> Result<UpliftDesk>
    where
        I: Into<PeripheralId>,
    {
        let peripheral = adapter.peripheral(&id.into()).await?;

        peripheral.connect().await?;
        peripheral.discover_services().await?;

        let (data_in_characteristic, data_out_characteristic, name_characteristic) =
            get_characteristics(peripheral.characteristics())?;

        println!("new desk connecting");

        match peripheral.properties().await? {
            Some(properties) if properties.services.contains(&DESK_SERVICE_UUID) => {
                Ok(UpliftDesk {
                    properties,
                    peripheral,
                    data_in_characteristic,
                    data_out_characteristic,
                    name_characteristic,
                    height_stream: Arc::new(RwLock::new(None)),
                })
            }
            _ => Err(btleplug::Error::DeviceNotFound),
        }
    }

    pub async fn name(&self) -> Result<Option<String>> {
        Ok(self
            .peripheral
            .properties()
            .await?
            .and_then(|p| p.local_name))
    }

    pub async fn get_height(&self) -> Result<UpliftDeskHeight> {
        let mut rx = self.stream_height().await?;
        rx.recv().await.map_err(|e| Error::Other(Box::new(e)))
    }

    pub async fn stream_height(&self) -> Result<Receiver<UpliftDeskHeight>> {
        let height_stream_read = self.height_stream.read().await;
        let rx = if let Some(height_stream) = height_stream_read.as_ref() {
            println!("read subscribe");
            height_stream.subscribe()
        } else {
            drop(height_stream_read);
            let mut height_stream_write = self.height_stream.write().await;
            if let Some(height_stream) = height_stream_write.as_ref() {
                println!("write subscribe");
                height_stream.subscribe()
            } else {
                let (tx, rx) = broadcast::channel(10);

                let peripheral = self.peripheral.clone();
                let data_in_characteristic = self.data_in_characteristic.clone();
                let data_out_characteristic = self.data_out_characteristic.clone();

                let mut height_receiver = peripheral.notifications().await?;
                peripheral.subscribe(&data_out_characteristic).await?;

                tokio::spawn({
                    let tx = tx.clone();
                    let height_stream = self.height_stream.clone();

                    println!("spawning stream");

                    async move {
                        let mut received_message = false;
                        loop {
                            select! {
                                event = height_receiver.next() => {
                                    match event {
                                        Some(ValueNotification { value, .. }) => {
                                            received_message = true;
                                            let height = UpliftDeskHeight::new(&value);
                                            println!("height: {height:?}");

                                            if let Err(_) = tx.send(height) {
                                                // no more receivers
                                                break;
                                            }
                                        }
                                        None => break,
                                    }
                                }
                                _ = sleep(Duration::from_secs(1)) => {
                                    if tx.receiver_count() <= 0 {
                                        break;
                                    } else if !received_message {
                                        write(&QUERY_PACKET,&data_in_characteristic,&peripheral).await;
                                    }
                                }
                            }
                        }

                        if let Err(e) = peripheral.unsubscribe(&data_out_characteristic).await {
                            log::warn!("Error unsubscribing from Data Out Characteristic: {e:?}")
                        }

                        *height_stream.write().await = None
                    }
                });

                *height_stream_write = Some(tx);

                rx
            }
        };

        self.query_desk().await?;
        Ok(rx)
    }

    pub async fn test(&self) -> Result<()> {
        self.peripheral.discover_services().await?;

        for service in self.peripheral.services() {
            println!("service: {service:?}")
        }

        Ok(())
    }

    pub async fn disconnect(self) -> UpliftDeskId {
        // the Drop implementation of self will disconnect this
        UpliftDeskId::new(self.peripheral.id())
    }

    async fn query_desk(&self) -> Result<()> {
        write(
            &QUERY_PACKET,
            &self.data_in_characteristic,
            &self.peripheral,
        )
        .await
    }

    // pub async fn new_old<I>(id: I, adapter: &Adapter) -> Result<Option<UpliftDesk>> where I: Into<PeripheralId> {
    //     let id = id.into();
    //
    //     let peripheral = adapter
    //         .peripheral(&id)
    //         .await
    //         .context(format!("{id} - Couldn't connect to Desk"))?;
    //
    //     let properties = peripheral.properties().await.context(format!(
    //         "{id} - Couldn't get properties",
    //     ))?;
    //
    //     match properties  {
    //         Some(properties) if properties.services.contains(&DESK_SERVICE_UUID) => {
    //             Ok(Some(UpliftDesk{ peripheral }))
    //         }
    //         _ => {
    //             Ok(None)
    //         }
    //     }
    // }
    //
    // pub async fn scan_for_desks(adapter: &Adapter) -> Receiver<Result<UpliftDesk>> {
    //     let (tx, rx) = mpsc::channel(10);
    //
    //     let adapter = adapter.clone();
    //     tokio::spawn(async move {
    //         async fn inner(
    //             adapter: &Adapter,
    //             tx: &Sender<Result<UpliftDesk>>,
    //         ) -> Result<()> {
    //             let mut events = adapter.events().await?;
    //
    //             // scan for our desk service
    //             adapter
    //                 .start_scan(ScanFilter {
    //                     services: vec![DESK_SERVICE_UUID],
    //                 })
    //                 .await?;
    //
    //             loop {
    //                 select! {
    //                 event = events.next() => {
    //                     match event {
    //                         Some(DeviceDiscovered(id) | DeviceUpdated(id) | DeviceConnected(id)) => {
    //                              match UpliftDesk::new_old(id, adapter).await {
    //                                 Ok(Some(desk)) => {
    //                                     if let Err(error) = tx.send(Ok(desk)).await {
    //                                         break Err(error.into())
    //                                     }
    //                                 }
    //                                 Ok(None) => (),
    //                                 Err(error) => log::warn!("{error}")
    //                             }
    //                         }
    //                         Some(event ) => log::trace!("Unhandled Event: {:?}", event),
    //                         None => {
    //                             break Err(anyhow!("Our adapter stopped looking for peripherals"))
    //                         }
    //                     }
    //                 }
    //                 _ = tx.closed() => {
    //                     break Ok(());
    //                 }
    //                 }
    //             }
    //         }
    //
    //         log::trace!("Started Scanning");
    //
    //         let result = inner(&adapter, &tx).await;
    //         if let Err(error) = adapter.stop_scan().await {
    //             log::error!("Failed to stop scanning: {error:?}");
    //         } else {
    //             log::trace!("Stopped Scanning")
    //         }
    //
    //         if let Err(error) = result {
    //             if let Err(error) = tx.send(Err(error)).await {
    //                 // log::warn!("Error when sending final error: {:?}", error.0)
    //             }
    //         }
    //     });
    //
    //     rx
    // }
}

#[inline]
async fn write(
    data: &[u8],
    characteristic: &Characteristic,
    peripheral: &Peripheral,
) -> Result<()> {
    peripheral
        .write(&characteristic, data, WriteType::WithoutResponse)
        .await
}

impl Deref for UpliftDesk {
    type Target = Peripheral;

    fn deref(&self) -> &Self::Target {
        &self.peripheral
    }
}

impl Drop for UpliftDesk {
    fn drop(&mut self) {
        let peripheral = self.peripheral.clone();
        // tokio::task::spawn(async move {
        //     let _ = peripheral.disconnect().await;
        // });
    }
}

#[derive(Clone, Debug)]
pub struct UpliftDeskHeight {
    low: u8,
    high: u8,
}

impl UpliftDeskHeight {
    fn new(data: &[u8]) -> Self {
        Self {
            low: data[5],
            high: data[7],
        }
    }

    pub fn physical_height(&self) -> usize {
        let low = self.low as usize;
        let high = self.high as usize;

        // let raw_height = if low >= 0xfd {
        //     // anything outside of this range seems to be "special"
        //     if last_height < MID_PHYSICAL_HEIGHT {
        //         high
        //     } else {
        //         low
        //     }
        // } else {
        //     low
        // };
        let raw_height = low;

        raw_height
    }
}

fn get_characteristics(
    characteristics: BTreeSet<Characteristic>,
) -> Result<(Characteristic, Characteristic, Characteristic)> {
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
        data_in_characteristic.ok_or_else(|| Error::NoSuchCharacteristic)?,
        data_out_characteristic.ok_or_else(|| Error::NoSuchCharacteristic)?,
        name_characteristic.ok_or_else(|| Error::NoSuchCharacteristic)?,
    ))
}

#[cfg(test)]
mod tests {
    pub use super::*;
    use btleplug::api::Manager as _;
    use btleplug::platform::Manager;
    #[tokio::test]
    async fn test() {
        let manager = Manager::new().await.unwrap();

        let adapters = manager.adapters().await.unwrap();
        let adapter = adapters.into_iter().next().unwrap();

        let mut rx = UpliftDeskId::scan(&adapter).await;
        let id = rx.recv().await.unwrap().unwrap();

        let desk = id.connect(&adapter).await.unwrap();

        println!(
            "height: {}",
            desk.get_height().await.unwrap().physical_height()
        )
    }
}

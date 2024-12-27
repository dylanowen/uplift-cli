use std::collections::BTreeSet;
use std::pin::Pin;
use btleplug::api::{bleuuid, BDAddr, Central, Characteristic, Descriptor, Peripheral as _, PeripheralProperties, ScanFilter, Service, ValueNotification, WriteType};
use btleplug::platform::{Peripheral, Adapter, Manager, PeripheralId};
use tokio::select;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender};
use uuid::Uuid;

use anyhow::{anyhow, Context, Result};
use btleplug::api;
use btleplug::api::CentralEvent::{DeviceConnected, DeviceDiscovered, DeviceUpdated};
use futures::{Stream, StreamExt};

const DESK_SERVICE_UUID: Uuid = bleuuid::uuid_from_u16(0xff12);

pub trait DeskAdapter {
    async fn scan_for_desks(&self) -> Receiver<Result<Peripheral>>;

    async fn get_desk_peripheral<I>(&self, id: I) -> Result<Option<Peripheral>> where I: Into<PeripheralId>;
}

impl DeskAdapter for Adapter {
    async fn scan_for_desks(&self) -> Receiver<Result<Peripheral>> {
        let (tx, rx) = mpsc::channel(10);

        let adapter = self.clone();
        tokio::spawn(async move {
            async fn inner(adapter: &Adapter, tx: &Sender<Result<Peripheral>>) -> Result<()> {
                let mut events = adapter.events().await?;

                // scan for our desk service
                adapter
                    .start_scan(ScanFilter {
                        services: vec![DESK_SERVICE_UUID],
                    })
                    .await?;

                loop {
                    select! {
                    event = events.next() => {
                        match event {
                            Some(DeviceDiscovered(id) | DeviceUpdated(id) | DeviceConnected(id)) => {
                                 match adapter.get_desk_peripheral(id).await {
                                    Ok(Some(peripheral)) => {
                                        if let Err(error) = tx.send(Ok(peripheral)).await {
                                            break Err(error.into())
                                        }
                                    }
                                    Ok(None) => (),
                                    Err(error) => log::warn!("{error}")
                                }
                            }
                            Some(event ) => log::trace!("Unhandled Event: {:?}", event),
                            None => {
                                break Err(anyhow!("Our adapter stopped looking for peripherals"))
                            }
                        }
                    }
                    _ = tx.closed() => {
                        break Ok(());
                    }
                    }
                }
            }

            log::trace!("Started Scanning");

            let result = inner(&adapter, &tx).await;
            if let Err(error) =  adapter.stop_scan().await {
                log::error!("Failed to stop scanning: {error:?}");
            }
            else {
                log::trace!("Stopped Scanning")
            }

            if let Err(error) = result {
                if let Err(error) = tx.send(Err(error)).await {
                    log::warn!("Error when sending final error: {:?}", error.0)
                }
            }
        });

        rx
    }

    async fn get_desk_peripheral<I>(&self, id: I) -> Result<Option<Peripheral>> where I: Into<PeripheralId> {
        let id = id.into();

        let peripheral = self
            .peripheral(&id)
            .await
            .context(format!("{id} - Couldn't connect to Desk"))?;

        let properties = peripheral.properties().await.context(format!(
            "{id} - Couldn't get properties",
        ))?;

        match properties  {
            Some(properties) if properties.services.contains(&DESK_SERVICE_UUID) => {
                Ok(Some(peripheral))
            }
            _ => {
                Ok(None)
            }
        }
    }
}


mod subscription;

use crate::api::{Command, EnhancedPeripheral as _, HeightUnit};
use crate::desk::subscription::DeskSubscription;
use crate::{DESK_UUIDS, DeskUuids, UpliftDeskId};
use btleplug::api::{Characteristic, Peripheral, WriteType};
use btleplug::{Error, Result, platform};
use either::Either;
use std::collections::BTreeSet;
use std::fmt::{Debug, Formatter};
use std::mem;
use tokio::sync::oneshot;
use tokio_stream::Stream;
use uom::si::f32::Length;

#[allow(clippy::large_enum_variant)]
pub enum UpliftDesk<P: Peripheral + 'static = platform::Peripheral> {
    Connected {
        peripheral: P,
        data_in_characteristic: Characteristic,
        data_out_characteristic: Characteristic,
        name_characteristic: Characteristic,
        subscription: Either<oneshot::Receiver<()>, DeskSubscription<P>>,
        cancel_subscription_tx: oneshot::Sender<()>,
        disconnect_on_drop: bool,
    },
    Disconnected {
        peripheral: P,
    },
}

impl<P: Peripheral> UpliftDesk<P> {
    pub fn new(peripheral: P) -> Self {
        Self::Disconnected { peripheral }
    }

    pub async fn connect(&mut self) -> Result<()> {
        if !matches!(self, UpliftDesk::Connected { peripheral, .. } if peripheral.is_connected().await?)
        {
            let peripheral = self.peripheral().clone();

            log::debug!("{{{}}} - Connecting to desk", peripheral.id());

            peripheral.connect().await?;
            peripheral.discover_services().await?;

            let (service, desk_uuids) = peripheral.services().iter().find_map(|service| {
                for desk in DESK_UUIDS {
                    if desk.service_uuid == service.uuid {
                        return Some((service.clone(), desk));
                    }
                }
                None
            }).ok_or_else(|| {
                log::warn!("{{{}}} - Attempted to connect to desk that isn't advertising the necessary Service or Characteristics", peripheral.id());
                Error::DeviceNotFound
            })?;

            let (data_in_characteristic, data_out_characteristic, name_characteristic) =
                get_characteristics(desk_uuids, service.characteristics)?;

            let (cancel_subscription_tx, cancel_subscription_rx) = oneshot::channel();

            // we're about to drop our non-connected self, we don't want to drop our connection as we're establishing it
            if let UpliftDesk::Connected {
                disconnect_on_drop, ..
            } = self
            {
                *disconnect_on_drop = true;
            }

            *self = Self::Connected {
                peripheral,
                disconnect_on_drop: false,
                data_in_characteristic,
                data_out_characteristic,
                name_characteristic,
                subscription: Either::Left(cancel_subscription_rx),
                cancel_subscription_tx,
            };
        }

        Ok(())
    }

    pub fn id(&self) -> UpliftDeskId {
        UpliftDeskId(self.peripheral().id())
    }

    pub async fn rssi(&self) -> Result<Option<i16>> {
        match self {
            UpliftDesk::Connected { peripheral, .. } => {
                Ok(peripheral.properties().await?.and_then(|p| p.rssi))
            }
            UpliftDesk::Disconnected { .. } => Err(Error::NotConnected),
        }
    }

    pub async fn name(&self) -> Result<Option<String>> {
        match self {
            UpliftDesk::Connected { peripheral, .. } => {
                Ok(peripheral.properties().await?.and_then(|p| p.local_name))
            }
            UpliftDesk::Disconnected { .. } => Err(Error::NotConnected),
        }
    }

    pub async fn command(&self, command: Command) -> Result<()> {
        match self {
            UpliftDesk::Connected {
                peripheral,
                data_in_characteristic,
                ..
            } if peripheral.is_connected().await? => {
                peripheral
                    .command(command, data_in_characteristic, WriteType::WithoutResponse)
                    .await
            }
            _ => Err(Error::NotConnected),
        }
    }

    pub async fn height(&mut self) -> Result<Length> {
        self.get_subscription().await?.get_height().await
    }

    pub async fn height_unit(&mut self) -> Result<HeightUnit> {
        Ok(self.get_subscription().await?.get_height_unit())
    }

    pub async fn height_stream(&mut self) -> Result<impl Stream<Item = Length> + use<P>> {
        self.get_subscription().await?.height_stream().await
    }

    /// It would be nice to return our desk here but btleplug drops any knowledge of this peripheral on disconnect
    pub async fn disconnect(&mut self) -> Result<()> {
        if let UpliftDesk::Connected {
            peripheral,
            cancel_subscription_tx,
            disconnect_on_drop: dont_disconnect,
            ..
        } = self
        {
            disconnect(
                peripheral,
                mem::replace(cancel_subscription_tx, oneshot::channel().0),
            )
            .await?;
            // we've already disconnected, we don't need a background job when we drop
            *dont_disconnect = true;
        }
        let peripheral = self.peripheral().clone();
        *self = UpliftDesk::Disconnected { peripheral };

        Ok(())
    }

    async fn get_subscription(&mut self) -> Result<&DeskSubscription<P>> {
        match self {
            UpliftDesk::Connected {
                peripheral,
                data_in_characteristic,
                data_out_characteristic,
                subscription,
                ..
            } if peripheral.is_connected().await? => {
                // subscription.as_ref()

                if let Either::Left(cancel_rx) = subscription.as_mut() {
                    let cancel_rx = mem::replace(cancel_rx, oneshot::channel().1);

                    *subscription = Either::Right(
                        DeskSubscription::new(
                            peripheral.id(),
                            data_in_characteristic.clone(),
                            data_out_characteristic.clone(),
                            peripheral.clone(),
                            cancel_rx,
                        )
                        .await?,
                    )
                }

                let subscription = subscription.as_ref().right().unwrap();
                if !subscription.is_live() {
                    // our subscription exists but the main loop isn't running
                    Err(Error::NotConnected)?
                }

                Ok(subscription)
            }
            _ => Err(Error::NotConnected),
        }
    }

    fn peripheral(&self) -> &P {
        match self {
            UpliftDesk::Connected { peripheral, .. } => peripheral,
            UpliftDesk::Disconnected { peripheral } => peripheral,
        }
    }
}

impl<P: Peripheral> Debug for UpliftDesk<P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpliftDesk")
            .field("id", &self.id())
            .finish()
    }
}

impl<P: Peripheral> Drop for UpliftDesk<P> {
    fn drop(&mut self) {
        if let UpliftDesk::Connected {
            peripheral,
            cancel_subscription_tx,
            disconnect_on_drop: false,
            ..
        } = self
        {
            let peripheral = peripheral.clone();
            let cancel = mem::replace(cancel_subscription_tx, oneshot::channel().0);
            tokio::spawn(async move {
                if let Err(e) = disconnect(&peripheral, cancel).await {
                    log::warn!("{{{}}} - Error while dropping desk: {e:?}", peripheral.id());
                }
            });
        }
    }
}

async fn disconnect<P: Peripheral>(
    peripheral: &P,
    cancel_subscription_tx: oneshot::Sender<()>,
) -> Result<()> {
    log::debug!("{{{}}} - Disconnecting from Desk", peripheral.id());
    if cancel_subscription_tx.send(()).is_err() {
        log::error!(
            "{{{}}} - We have already cancelled our subscription",
            peripheral.id()
        );
    }
    peripheral.disconnect().await
}

fn get_characteristics(
    desk_characteristic: DeskUuids,
    characteristics: BTreeSet<Characteristic>,
) -> Result<(Characteristic, Characteristic, Characteristic)> {
    let mut data_in_characteristic = None;
    let mut data_out_characteristic = None;
    let mut name_characteristic = None;

    for characteristic in characteristics.into_iter() {
        if desk_characteristic.data_in == characteristic.uuid {
            data_in_characteristic = Some(characteristic);
        } else if desk_characteristic.data_out == characteristic.uuid {
            data_out_characteristic = Some(characteristic);
        } else if desk_characteristic.name_uuid == characteristic.uuid {
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
    use super::*;
    use crate::mock_btle::*;
    use btleplug::api::{CharPropFlags, Characteristic, Service};
    use tokio::runtime::Runtime;

    fn valid_characteristics() -> BTreeSet<Characteristic> {
        [
            DESK_UUIDS[0].data_in,
            DESK_UUIDS[0].data_out,
            DESK_UUIDS[0].name_uuid,
        ]
        .into_iter()
        .map(|uuid| Characteristic {
            uuid,
            service_uuid: DESK_UUIDS[0].service_uuid,
            properties: CharPropFlags::empty(),
            descriptors: BTreeSet::new(),
        })
        .collect()
    }

    #[test]
    fn test_new_desk() {
        let runtime = Runtime::new().unwrap();

        runtime.block_on(async {
            let mut peripheral_a = MockPeripheral::new();

            let mut disconnect_peripheral = MockPeripheral::new();
            disconnect_peripheral
                .expect_disconnect()
                .returning(|| Ok(()));

            let mut peripheral_b = MockPeripheral::new();
            peripheral_b.expect_connect().returning(|| Ok(()));
            peripheral_b.expect_discover_services().returning(|| Ok(()));
            peripheral_b.expect_services().returning(|| {
                [Service {
                    uuid: DESK_UUIDS[0].service_uuid,
                    primary: true,
                    characteristics: valid_characteristics(),
                }]
                .into_iter()
                .collect()
            });
            peripheral_b
                .expect_clone()
                .return_once(move || disconnect_peripheral);
            peripheral_a
                .expect_clone()
                .return_once(move || peripheral_b);

            let mut desk = UpliftDesk::new(peripheral_a);
            desk.connect().await.unwrap();
            drop(desk);
        });

        // This will log panics from tokio threads, but it won't fail our test :( we need
        // https://docs.rs/tokio/latest/tokio/runtime/struct.Builder.html#method.unhandled_panic
        drop(runtime);
    }

    #[tokio::test]
    async fn test_invalid_peripheral() {
        let mut peripheral = MockPeripheral::new();

        let mut drop_peripheral = MockPeripheral::new();
        drop_peripheral.expect_connect().returning(|| Ok(()));
        drop_peripheral
            .expect_discover_services()
            .returning(|| Ok(()));
        drop_peripheral.expect_services().returning(BTreeSet::new);

        peripheral
            .expect_clone()
            .return_once(move || drop_peripheral);

        assert!(matches!(
            UpliftDesk::new(peripheral).connect().await.err().unwrap(),
            Error::DeviceNotFound
        ));
    }

    // fn mock_central<F>(setup_peripheral: F) -> MockCentral
    // where
    //     F: FnOnce(&mut MockPeripheral),
    // {
    //     let mut central = MockCentral::new();
    //     let mut peripheral = MockPeripheral::new();
    //
    //     peripheral.expect_connect().returning(|| Ok(()));
    //     peripheral.expect_discover_services().returning(|| Ok(()));
    //     peripheral.expect_properties().returning(|| {
    //         Ok(Some(PeripheralProperties {
    //             services: vec![DESK_UUIDS[0].service_uuid],
    //             ..Default::default()
    //         }))
    //     });
    //     peripheral.expect_services().returning(move || {
    //         let mut services = BTreeSet::new();
    //         services.insert(Service {
    //             uuid: DESK_UUIDS[0].service_uuid,
    //             primary: true,
    //             characteristics: valid_characteristics(),
    //         });
    //
    //         services
    //     });
    //
    //     let mut drop_peripheral = MockPeripheral::new();
    //     drop_peripheral
    //         .expect_disconnect()
    //         .once()
    //         .returning(|| Ok(()));
    //     peripheral
    //         .expect_clone()
    //         .return_once(move || drop_peripheral);
    //
    //     setup_peripheral(&mut peripheral);
    //
    //     central.expect_peripheral().return_once(|_| Ok(peripheral));
    //
    //     central
    // }
    //
    // fn valid_characteristics() -> BTreeSet<Characteristic> {
    //     let mut characteristics = BTreeSet::new();
    //
    //     for uuid in [
    //         DESK_UUIDS[0].data_in,
    //         DESK_UUIDS[0].data_out,
    //         DESK_UUIDS[0].name_uuid,
    //     ] {
    //         characteristics.insert(Characteristic {
    //             uuid,
    //             service_uuid: DESK_UUIDS[0].service_uuid,
    //             properties: CharPropFlags::empty(),
    //             descriptors: BTreeSet::new(),
    //         });
    //     }
    //
    //     characteristics
    // }
}

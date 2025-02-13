use crate::api::{
    Command, EnhancedPeripheral as _, HeightUnit, Message, DESK_DATA_IN_UUID, DESK_DATA_OUT_UUID,
    DESK_NAME_UUID, DESK_SERVICE_UUID,
};
use crate::UpliftDeskId;
use btleplug::api::{
    Central, Characteristic, Peripheral, PeripheralProperties, ValueNotification, WriteType,
};
use btleplug::platform::PeripheralId;
use btleplug::{platform, Error, Result};
use either::Either;
use std::collections::BTreeSet;
use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::mem;
use std::ops::Deref;
use std::time::Duration;
use tokio::sync::{oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::interval;
use tokio::{select, time};
use tokio_stream::wrappers::WatchStream;
use tokio_stream::{Stream, StreamExt};
use uom::si::f32::Length;
use uom::si::length::{centimeter, inch};

pub struct UpliftDesk<P: Peripheral + 'static = platform::Peripheral> {
    properties: PeripheralProperties,
    peripheral: P,
    data_in_characteristic: Characteristic,
    data_out_characteristic: Characteristic,
    name_characteristic: Characteristic,
    subscription: Either<oneshot::Receiver<()>, DeskSubscription<P>>,
    cancel_subscription_tx: oneshot::Sender<()>,
}

impl<P: Peripheral> UpliftDesk<P> {
    pub async fn new<I, C>(id: I, central: &C) -> Result<Self>
    where
        I: Into<PeripheralId>,
        C: Central<Peripheral = P>,
    {
        let id = id.into();
        log::debug!("{{{id}}} - Connecting to desk");

        let peripheral = central.peripheral(&id).await?;

        peripheral.connect().await?;
        peripheral.discover_services().await?;

        let properties = peripheral.properties().await?.ok_or_else(|| {
            log::warn!("{{{id}}} - No properties found for our service");
            Error::DeviceNotFound
        })?;
        let service = peripheral.services().iter().find(|s| s.uuid == DESK_SERVICE_UUID).cloned().ok_or_else(|| {
            log::warn!("{{{id}}} - Attempted to connect to desk that isn't advertising the necessary Service or Characteristics");
            Error::DeviceNotFound
        })?;

        let (data_in_characteristic, data_out_characteristic, name_characteristic) =
            get_characteristics(service.characteristics)?;

        let (cancel_subscription_tx, cancel_subscription_rx) = oneshot::channel();

        Ok(Self {
            properties,
            peripheral,
            data_in_characteristic,
            data_out_characteristic,
            name_characteristic,
            subscription: Either::Left(cancel_subscription_rx),
            cancel_subscription_tx,
        })
    }

    pub fn id(&self) -> UpliftDeskId {
        UpliftDeskId(self.peripheral.id())
    }

    pub fn rssi(&self) -> Option<i16> {
        self.properties.rssi
    }

    pub async fn name(&self) -> Result<Option<String>> {
        Ok(self
            .peripheral
            .properties()
            .await?
            .and_then(|p| p.local_name))
    }

    pub async fn command(&self, command: Command) -> Result<()> {
        self.peripheral
            .command(
                command,
                &self.data_in_characteristic,
                WriteType::WithoutResponse,
            )
            .await
    }

    pub async fn height(&mut self) -> Result<Length> {
        self.get_subscription().await?.get_height().await
    }

    pub async fn height_unit(&mut self) -> Result<HeightUnit> {
        Ok(self.get_subscription().await?.get_height_unit())
    }

    pub async fn height_stream(&mut self) -> Result<impl Stream<Item = Length>> {
        self.get_subscription().await?.height_stream().await
    }

    pub fn disconnect(self) -> UpliftDeskId {
        // Drop takes care of disconnecting
        self.id()
    }

    async fn get_subscription(&mut self) -> Result<&DeskSubscription<P>> {
        if !self.peripheral.is_connected().await? {
            Err(Error::NotConnected)?
        } else {
            if self.subscription.is_left() {
                let cancel_rx =
                    mem::replace(&mut self.subscription, Either::Left(oneshot::channel().1))
                        .left()
                        .unwrap();

                self.subscription = Either::Right(
                    DeskSubscription::new(
                        self.peripheral.id(),
                        self.data_in_characteristic.clone(),
                        self.data_out_characteristic.clone(),
                        self.peripheral.clone(),
                        cancel_rx,
                    )
                    .await,
                );
            }

            let subscription = self.subscription.as_ref().right().unwrap();
            if !subscription.is_live() {
                // our subscription exists but the main loop isn't running
                Err(Error::NotConnected)?
            }

            Ok(subscription)
        }
    }
}

impl<P: Peripheral> Debug for UpliftDesk<P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpliftDesk")
            .field("id", &self.id())
            .field("active_subscription", &self.subscription.is_right())
            .finish()
    }
}

impl<P: Peripheral> Drop for UpliftDesk<P> {
    fn drop(&mut self) {
        let cancel = mem::replace(&mut self.cancel_subscription_tx, oneshot::channel().0);
        let _ = cancel.send(());

        let peripheral = self.peripheral.clone();
        tokio::spawn(async move {
            if let Err(e) = peripheral.disconnect().await {
                log::warn!("{{{}}} - Error while dropping desk: {e:?}", peripheral.id());
            }
        });
    }
}

struct DeskSubscription<P: Peripheral> {
    height_rx: watch::Receiver<Option<u16>>,
    height_unit_rx: watch::Receiver<HeightUnit>,
    data_in_characteristic: Characteristic,
    peripheral: P,
    subscription_live: JoinHandle<Result<()>>,
}

/// The app starts up by sending
///
/// t:400  [Command::FetchHighLowLimit]
/// t:500  [Command::FetchHighLowLimit]
/// t:600  [Command::FetchHeightValue]
/// t:700  [Command::FetchHeightRange]
/// t:?    F1F11F0100207E Advanced Command: 0x1F/31 data: 00
/// t:?    F1F1FE00FE7E Simple command: 0xFE/-2
/// t:1100 F1F1FE00FE7E
impl<P: Peripheral + 'static> DeskSubscription<P> {
    async fn new(
        id: PeripheralId,
        data_in_characteristic: Characteristic,
        data_out_characteristic: Characteristic,
        peripheral: P,
        cancel_rx: oneshot::Receiver<()>,
    ) -> Self {
        let (height_tx, height_rx) = watch::channel(None);
        // The app seems to default to inches
        let (height_unit_tx, height_unit_rx) = watch::channel(HeightUnit::Inch);

        let receiver = peripheral.notifications().await.unwrap();
        peripheral
            .subscribe(&data_out_characteristic)
            .await
            .unwrap();

        // spawn the initial commands
        tokio::spawn({
            let peripheral = peripheral.clone();
            let data_in_characteristic = data_in_characteristic.clone();
            async move {
                // The app specifically sleeps for 100ms between commands
                let mut interval = time::interval(Duration::from_millis(100));
                for cmd in [
                    Command::FetchHighLowLimit,
                    Command::FetchHighLowLimit,
                    Command::FetchHeightValue,
                ] {
                    interval.tick().await;
                    if let Err(e) = peripheral
                        .command(cmd, &data_in_characteristic, WriteType::WithoutResponse)
                        .await
                    {
                        log::warn!("Failed to write `{cmd:?}`: {e:?}");
                        break;
                    }
                }
            }
        });

        let subscription_live = tokio::spawn({
            let peripheral = peripheral.clone();
            async move {
                let result = subscription_main_loop(
                    receiver,
                    height_tx,
                    height_unit_tx,
                    cancel_rx,
                    data_out_characteristic,
                    peripheral,
                )
                .await;

                if let Err(e) = &result {
                    log::error!("{{{id}}} - Error in our subscription task: {e:?}")
                }

                result
            }
        });

        Self {
            height_rx,
            height_unit_rx,
            data_in_characteristic,
            peripheral,
            subscription_live,
        }
    }

    fn get_height(&self) -> impl Future<Output = Result<Length>> + 'static {
        let unit_watch = self.height_unit_rx.clone();
        let height_future = self.wait_for_some_height();
        async move {
            Ok(desk_height_to_length(
                height_future.await?,
                unit_watch.borrow().clone(),
            ))
        }
    }

    fn get_height_unit(&self) -> HeightUnit {
        // I don't know how to ask the desk for this info
        self.height_unit_rx.borrow().clone()
    }

    async fn height_stream(&self) -> Result<impl Stream<Item = Length>> {
        let unit_watch = self.height_unit_rx.clone();
        self.wait_for_some_height().await?;
        Ok(
            WatchStream::new(self.height_rx.clone()).filter_map(move |maybe_height| {
                maybe_height
                    .map(|height| desk_height_to_length(height, unit_watch.borrow().clone()))
            }),
        )
    }

    fn wait_for_some_height(&self) -> impl Future<Output = Result<u16>> + Send + 'static {
        let peripheral = self.peripheral.clone();
        let data_in_characteristic = self.data_in_characteristic.clone();
        wait_for_some(self.height_rx.clone(), move || async move {
            peripheral
                .command(
                    Command::FetchHeightValue,
                    &data_in_characteristic,
                    WriteType::WithoutResponse,
                )
                .await
        })
    }

    fn is_live(&self) -> bool {
        !self.subscription_live.is_finished()
    }
}

async fn subscription_main_loop<R, P>(
    mut receiver: R,
    height_tx: watch::Sender<Option<u16>>,
    height_unit_tx: watch::Sender<HeightUnit>,
    mut cancel_rx: oneshot::Receiver<()>,
    data_out_characteristic: Characteristic,
    peripheral: P,
) -> Result<()>
where
    R: Stream<Item = ValueNotification> + Unpin,
    P: Peripheral,
{
    let id = peripheral.id();
    log::trace!("{{{id}}} - Spawning Subscription");
    let mut partial_message = vec![];
    loop {
        select! {
            event = receiver.next() => {
                match event {
                    Some(ValueNotification { value: raw_message, .. }) => {
                        log::trace!("{{{id}}} - Received Data {}{raw_message:02x?}", if partial_message.len() > 0 { format!("previous({partial_message:02x?}) ") } else { String::new() });
                        let complete_message = [partial_message, raw_message].concat();
                        match Message::parse(&complete_message) {
                            Ok((remainder, messages)) => {
                                partial_message = remainder.to_vec();
                                for message in messages {
                                    log::debug!("{{{id}}} - Received Message {message:?}");
                                    match message {
                                        Message::Height(height) => {
                                            height_tx.send(Some(height)).map_err(|_| Error::RuntimeError("Couldn't send latest value".to_string()))?;
                                        }
                                        Message::PhysicalLimits{ .. } => {}
                                        Message::HeightUnit(height_unit) => {
                                            height_unit_tx.send(height_unit).map_err(|_| Error::RuntimeError("Couldn't send latest value".to_string()))?;
                                        }
                                        Message::Unknown{ .. } => {}
                                    }
                                }
                            }
                            Err(e) => {
                                log::trace!("{{{id}}} - Couldn't parse message {e:?} @ {complete_message:02x?}");
                                partial_message = vec![];
                            }
                        }
                    }
                    None => break,
                }
            }
            _ = &mut cancel_rx => {
                break
            }
        }
    }

    if let Err(e) = peripheral.unsubscribe(&data_out_characteristic).await {
        log::warn!(
            "{{{id}}} - Error Unsubscribing from {{{}}}: {e:?}",
            data_out_characteristic.uuid
        );
    }

    log::trace!("{{{id}}} - Exiting Subscription");

    Ok(())
}

async fn wait_for_some<T, F, Fut, FutO>(mut rx: watch::Receiver<Option<T>>, on_none: F) -> Result<T>
where
    T: Clone,
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<FutO>>,
{
    let maybe_result = rx.borrow_and_update().deref().clone();
    match maybe_result {
        Some(result) => Ok(result.clone()),
        None => {
            on_none().await?;

            Ok(rx
                .wait_for(Option::is_some)
                .await
                .map_err(|_| Error::NotConnected)?
                .clone()
                .unwrap())
        }
    }
}

fn desk_height_to_length(raw_height: u16, height_unit: HeightUnit) -> Length {
    let height = match height_unit {
        HeightUnit::Cm => Length::new::<centimeter>(raw_height as f32),
        HeightUnit::Inch => Length::new::<inch>(raw_height as f32),
    };

    // The desk seems to have its values 10x greater than the unit
    height / 10.
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
    use super::*;
    use crate::mock_btle::*;
    use btleplug::api::{CharPropFlags, Service};
    use tokio::runtime::Runtime;
    use uuid::Uuid;

    #[test]
    fn test_new_desk() {
        let runtime = Runtime::new().unwrap();

        runtime.block_on(async {
            let central = mock_peripheral(|_| ());

            let id: PeripheralId = Uuid::default().into();
            let desk = UpliftDesk::new(id, &central).await.unwrap();
            drop(desk);
        });

        // This will log panics from tokio threads ,but it won't fail our test :( we need
        // https://docs.rs/tokio/latest/tokio/runtime/struct.Builder.html#method.unhandled_panic
        drop(runtime);
    }

    #[tokio::test]
    async fn test_invalid_peripheral() {
        let mut central = MockCentral::new();
        let mut peripheral = MockPeripheral::new();

        peripheral.expect_connect().returning(|| Ok(()));
        peripheral.expect_discover_services().returning(|| Ok(()));
        peripheral.expect_properties().returning(|| {
            Ok(Some(PeripheralProperties {
                services: vec![DESK_SERVICE_UUID],
                ..Default::default()
            }))
        });
        peripheral.expect_services().returning(BTreeSet::new);

        let mut drop_peripheral = MockPeripheral::new();
        drop_peripheral
            .expect_disconnect()
            .once()
            .returning(|| Ok(()));
        peripheral
            .expect_clone()
            .return_once(move || drop_peripheral);

        central.expect_peripheral().return_once(|_| Ok(peripheral));

        let id: PeripheralId = Uuid::default().into();
        assert!(matches!(
            UpliftDesk::new(id, &central).await.err().unwrap(),
            Error::DeviceNotFound
        ));
    }

    fn mock_peripheral<F>(setup_peripheral: F) -> MockCentral
    where
        F: FnOnce(&mut MockPeripheral),
    {
        let mut central = MockCentral::new();
        let mut peripheral = MockPeripheral::new();

        peripheral.expect_connect().returning(|| Ok(()));
        peripheral.expect_discover_services().returning(|| Ok(()));
        peripheral.expect_properties().returning(|| {
            Ok(Some(PeripheralProperties {
                services: vec![DESK_SERVICE_UUID],
                ..Default::default()
            }))
        });
        peripheral.expect_services().returning(move || {
            let mut services = BTreeSet::new();
            services.insert(Service {
                uuid: DESK_SERVICE_UUID,
                primary: true,
                characteristics: valid_characteristics(),
            });

            services
        });

        let mut drop_peripheral = MockPeripheral::new();
        drop_peripheral
            .expect_disconnect()
            .once()
            .returning(|| Ok(()));
        peripheral
            .expect_clone()
            .return_once(move || drop_peripheral);

        setup_peripheral(&mut peripheral);

        central.expect_peripheral().return_once(|_| Ok(peripheral));

        central
    }

    fn valid_characteristics() -> BTreeSet<Characteristic> {
        let mut characteristics = BTreeSet::new();

        for uuid in [DESK_DATA_IN_UUID, DESK_DATA_OUT_UUID, DESK_NAME_UUID] {
            characteristics.insert(Characteristic {
                uuid,
                service_uuid: DESK_SERVICE_UUID,
                properties: CharPropFlags::empty(),
                descriptors: BTreeSet::new(),
            });
        }

        characteristics
    }
}

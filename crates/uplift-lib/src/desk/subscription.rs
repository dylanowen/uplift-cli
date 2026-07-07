use crate::api::{Command, EnhancedPeripheral as _, HeightUnit, Message};
use btleplug::Error;
use btleplug::api::{Characteristic, Peripheral, ValueNotification, WriteType};
use btleplug::platform::PeripheralId;
use std::future::Future;
use std::ops::Deref;
use std::time::Duration;
use tokio::sync::{oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio::{select, time};
use tokio_stream::wrappers::WatchStream;
use tokio_stream::{Stream, StreamExt};
use uom::si::f32::Length;
use uom::si::length::{centimeter, inch};

pub struct DeskSubscription<P: Peripheral> {
    height_rx: watch::Receiver<Option<u16>>,
    height_unit_rx: watch::Receiver<HeightUnit>,
    data_in_characteristic: Characteristic,
    peripheral: P,
    subscription_live: JoinHandle<btleplug::Result<()>>,
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
    pub(super) async fn new(
        id: PeripheralId,
        data_in_characteristic: Characteristic,
        data_out_characteristic: Characteristic,
        peripheral: P,
        cancel_rx: oneshot::Receiver<()>,
    ) -> btleplug::Result<Self> {
        let (height_tx, height_rx) = watch::channel(None);
        // The app seems to default to inches
        let (height_unit_tx, height_unit_rx) = watch::channel(HeightUnit::Inch);

        let receiver = peripheral.notifications().await?;
        peripheral.subscribe(&data_out_characteristic).await?;

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

        Ok(Self {
            height_rx,
            height_unit_rx,
            data_in_characteristic,
            peripheral,
            subscription_live,
        })
    }

    pub(super) fn get_height(&self) -> impl Future<Output = btleplug::Result<Length>> + 'static {
        let unit_watch = self.height_unit_rx.clone();
        let height_future = self.wait_for_some_height();
        async move {
            Ok(desk_height_to_length(
                height_future.await?,
                *unit_watch.borrow(),
            ))
        }
    }

    pub(super) fn get_height_unit(&self) -> HeightUnit {
        // I don't know how to ask the desk for this info
        *self.height_unit_rx.borrow()
    }

    pub(super) async fn height_stream(
        &self,
    ) -> btleplug::Result<impl Stream<Item = Length> + use<P>> {
        let unit_watch = self.height_unit_rx.clone();
        self.wait_for_some_height().await?;
        Ok(
            WatchStream::new(self.height_rx.clone()).filter_map(move |maybe_height| {
                maybe_height.map(|height| desk_height_to_length(height, *unit_watch.borrow()))
            }),
        )
    }

    fn wait_for_some_height(&self) -> impl Future<Output = btleplug::Result<u16>> + Send + 'static {
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

    pub(super) fn is_live(&self) -> bool {
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
) -> btleplug::Result<()>
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
                        log::trace!("{{{id}}} - Received Data {}{raw_message:02x?}", if !partial_message.is_empty() { format!("previous({partial_message:02x?}) ") } else { String::new() });
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
                                        Message::LimitedStatus => {}
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
                log::trace!("{{{id}}} - Cancelling Background Subscription");
                break
            }
        }
    }
    // Drop here to signal ASAP that our subscription is closing
    drop(height_tx);
    drop(height_unit_tx);

    // We MUST time out here as btleplug may never call us back
    if let Err(e) = timeout(
        Duration::from_secs(10),
        peripheral.unsubscribe(&data_out_characteristic),
    )
    .await
    .map_err(|_| "Timed Out".to_string())
    .and_then(|r| r.map_err(|e| format!("{e:?}")))
    {
        log::debug!(
            "{{{id}}} - Error Unsubscribing from {{{}}}: {e}",
            data_out_characteristic.uuid
        );
    }

    log::trace!("{{{id}}} - Exiting Subscription");

    Ok(())
}

async fn wait_for_some<T, F, Fut, FutO>(
    mut rx: watch::Receiver<Option<T>>,
    on_none: F,
) -> btleplug::Result<T>
where
    T: Clone,
    F: FnOnce() -> Fut,
    Fut: Future<Output = btleplug::Result<FutO>>,
{
    let maybe_result = rx.borrow_and_update().deref().clone();
    match maybe_result {
        Some(result) => Ok(result),
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

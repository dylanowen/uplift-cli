use crate::{DESK_UUIDS, UpliftDesk};
use btleplug::api::CentralEvent::{DeviceConnected, DeviceDiscovered, DeviceUpdated};
use btleplug::api::{Central, CentralEvent, Peripheral, ScanFilter};
use futures::{Stream, ready};
use log::debug;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

#[must_use = "streams do nothing unless polled"]
pub struct UpliftDeskScanner<C>
where
    // Workaround for the macro https://github.com/taiki-e/pin-project-lite/issues/2#issuecomment-745190950
    C: Central,
    C: Unpin,
    C: 'static,
{
    events: Pin<Box<dyn Stream<Item = CentralEvent> + Send>>,
    pending: Option<Pin<Box<dyn Future<Output = C::Peripheral>>>>,
    central: C,
}

impl<C> Drop for UpliftDeskScanner<C>
where
    C: Central,
    C: Unpin,
    C: 'static,
{
    fn drop(&mut self) {
        let central = self.central.clone();
        tokio::spawn(async move {
            if let Err(error) = central.stop_scan().await {
                log::error!("Failed to stop scanning: {error:?}");
            } else {
                log::trace!("Stopped Scanning")
            }
        });
    }
}

impl<C> UpliftDeskScanner<C>
where
    C: Central + Unpin,
{
    pub async fn new(central: C) -> btleplug::Result<Self> {
        let events = central.events().await?;

        central
            .start_scan(ScanFilter {
                services: DESK_UUIDS.iter().map(|d| d.service_uuid).collect(),
            })
            .await?;

        log::trace!("Started Scanning");

        Ok(Self {
            events,
            pending: None,
            central,
        })
    }
}

impl<C> Stream for UpliftDeskScanner<C>
where
    C: Central + Unpin,
{
    type Item = UpliftDesk<C::Peripheral>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let next_desk = loop {
            if let Some(ref mut p) = self.pending {
                // We have a peripheral in progress, poll that until it's done
                let peripheral = ready!(p.as_mut().poll(cx));
                self.pending = None;

                debug!("Discovered peripheral {:?}", peripheral.id());

                break Some(UpliftDesk::new(peripheral));
            } else {
                match ready!(self.events.as_mut().poll_next(cx)) {
                    Some(DeviceDiscovered(id) | DeviceUpdated(id) | DeviceConnected(id)) => {
                        let central = self.central.clone();
                        self.pending = Some(Box::pin(async move {
                            central.peripheral(&id).await.unwrap_or_else(|_| {
                                panic!("{{{id}}} - Couldn't find just-reported Peripheral")
                            })
                        }));
                    }
                    Some(event) => log::trace!("Unhandled Event: {:?}", event),
                    None => break None,
                }
            }
        };

        Poll::Ready(next_desk)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        // We don't know the lower bound
        (0, self.events.size_hint().1)
    }
}

// impl<St, Fut, F, T> FusedStream for FilterMap<St, Fut, F>
// where
//     St: Stream + FusedStream,
//     F: FnMut1<St::Item, Output = Fut>,
//     Fut: Future<Output = Option<T>>,
// {
//     fn is_terminated(&self) -> bool {
//         self.pending.is_none() && self.stream.is_terminated()
//     }
// }

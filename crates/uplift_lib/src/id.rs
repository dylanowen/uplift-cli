use crate::UpliftDesk;
use btleplug::api::CentralEvent::{DeviceConnected, DeviceDiscovered, DeviceUpdated};
use btleplug::api::{Central, CentralEvent, ScanFilter};
use btleplug::platform::PeripheralId;
use btleplug::Result;
use futures::{ready, Stream};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::task::{Context, Poll};
use uuid::Uuid;

use crate::api::DESK_SERVICE_UUID;

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Eq, Hash, Ord, PartialEq, PartialOrd, Clone, Debug)]
pub struct UpliftDeskId(pub(crate) PeripheralId);

impl UpliftDeskId {
    pub fn new<I>(id: I) -> Self
    where
        I: Into<PeripheralId>,
    {
        Self(id.into())
    }

    pub async fn scan<C>(central: &C) -> Result<UpliftDeskIdStream<C>>
    where
        C: Central + Unpin + 'static,
    {
        UpliftDeskIdStream::new(central).await
    }

    pub async fn connect<C>(&self, adapter: &C) -> Result<UpliftDesk<C::Peripheral>>
    where
        C: Central,
        <C as Central>::Peripheral: 'static,
    {
        UpliftDesk::new(self.0.clone(), adapter).await
    }
}

impl From<Uuid> for UpliftDeskId {
    fn from(value: Uuid) -> Self {
        UpliftDeskId(value.into())
    }
}

impl From<UpliftDeskId> for PeripheralId {
    fn from(val: UpliftDeskId) -> Self {
        val.0
    }
}

pub struct UpliftDeskIdStream<C>
where
    C: Central + Unpin + 'static,
{
    events: Pin<Box<dyn Stream<Item = CentralEvent> + Send>>,
    central: C,
}

impl<C> UpliftDeskIdStream<C>
where
    C: Central + Unpin,
{
    async fn new(central: &C) -> Result<Self> {
        let central = central.clone();
        let events = central.events().await?;

        central
            .start_scan(ScanFilter {
                services: vec![DESK_SERVICE_UUID],
            })
            .await?;

        log::trace!("Started Scanning");

        Ok(Self { events, central })
    }
}

impl<C> Stream for UpliftDeskIdStream<C>
where
    C: Central + Unpin,
{
    type Item = UpliftDeskId;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let next_id = loop {
            match ready!(self.events.as_mut().poll_next(cx)) {
                Some(DeviceDiscovered(id) | DeviceUpdated(id) | DeviceConnected(id)) => {
                    break Some(UpliftDeskId::new(id))
                }
                Some(event) => log::trace!("Unhandled Event: {:?}", event),
                None => break None,
            }
        };

        Poll::Ready(next_id)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        // We don't know the lower bound
        (0, self.events.size_hint().1)
    }
}

impl<C> Drop for UpliftDeskIdStream<C>
where
    C: Central + Unpin + 'static,
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

#[cfg(feature = "serde")]
mod serde_feature {
    use super::*;

    impl From<Vec<u8>> for UpliftDeskId {
        fn from(value: Vec<u8>) -> Self {
            rmp_serde::from_slice(&value).expect("Failed to deserialize desk id")
        }
    }

    // impl TryFrom<Vec<u8>> for UpliftDeskId {
    //     type Error = rmp_serde::decode::Error;
    //
    //     fn try_from(value: Vec<u8>) -> result::Result<Self, Self::Error> {
    //         rmp_serde::from_slice(&value)
    //     }
    // }
}

#[cfg(feature = "sqlx")]
mod sqlx_feature {
    use super::*;
    use sqlx::encode::IsNull;
    use sqlx::error::BoxDynError;
    use sqlx::{Database, Decode, Encode, Type};
    use std::result;

    impl<DB: Database> Type<DB> for UpliftDeskId
    where
        [u8]: Type<DB>,
    {
        fn type_info() -> DB::TypeInfo {
            <&[u8] as Type<DB>>::type_info()
        }

        fn compatible(ty: &DB::TypeInfo) -> bool {
            <&[u8] as Type<DB>>::compatible(ty)
        }
    }

    impl<'r, DB: Database> Decode<'r, DB> for UpliftDeskId
    where
        // make sure our DB supports binary
        Vec<u8>: Decode<'r, DB>,
    {
        fn decode(
            value: <DB as Database>::ValueRef<'r>,
        ) -> result::Result<UpliftDeskId, BoxDynError> {
            let raw_value = <Vec<u8> as Decode<DB>>::decode(value)?;

            Ok(rmp_serde::from_slice(&raw_value)?)
        }
    }

    // impl<'q, DB: Database> Encode<'q, DB> for UpliftDeskId
    // where
    //     // make sure our DB supports binary
    //     &'q [u8]: Encode<'q, DB>,
    // {
    //     fn encode_by_ref(
    //         &self,
    //         buf: &mut <DB as Database>::ArgumentBuffer<'q>,
    //     ) -> result::Result<IsNull, BoxDynError> {
    //         rmp_serde::to_vec(self)?.encode(buf)
    //     }
    // }

    impl<'q, DB: Database> Encode<'q, DB> for UpliftDeskId
    where
        // make sure our DB supports binary
        Vec<u8>: Encode<'q, DB>,
    {
        fn encode_by_ref(
            &self,
            buf: &mut <DB as Database>::ArgumentBuffer<'q>,
        ) -> result::Result<IsNull, BoxDynError> {
            rmp_serde::to_vec(self)?.encode(buf)
        }
    }

    // impl<'r, DB: Database> Decode<'r, DB> for UpliftDeskId
    // where
    //     // make sure our DB supports binary
    //     &'r [u8]: Decode<'r, DB>,
    // {
    //     fn decode(
    //         value: <DB as Database>::ValueRef<'r>,
    //     ) -> result::Result<UpliftDeskId, Box<dyn Error + 'static + Send + Sync>> {
    //         let raw_value = <&[u8] as Decode<DB>>::decode(value)?;
    //
    //         Ok(rmp_serde::from_slice(raw_value)?)
    //     }
    // }
}

#[cfg(test)]
mod tests {

    // #[tokio::test]
    // async fn test() {
    //     let manager = Manager::new().await.unwrap();
    //
    //     let adapters = manager.adapters().await.unwrap();
    //     let adapter = adapters.into_iter().next().unwrap();
    //
    //     let mut rx = UpliftDeskId::scan(&adapter).await;
    //
    //     let mut i = 10;
    //     while let Some(result) = rx.recv().await {
    //         println!("{result:?}");
    //         i -= 1;
    //
    //         if i <= 0 {
    //             break;
    //         }
    //     }
    // }
}

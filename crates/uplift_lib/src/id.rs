use crate::{UpliftDesk, DESK_SERVICE_UUID};
use anyhow::anyhow;
use btleplug::api::CentralEvent::{DeviceConnected, DeviceDiscovered, DeviceUpdated};
use btleplug::api::{bleuuid, Central, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Peripheral, PeripheralId};
use btleplug::Result;
use futures::StreamExt;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::{mem, result};
use tokio::select;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender};
use uuid::Uuid;

#[cfg(feature = "sqlx")]
pub use sqlx_feature::*;

#[cfg(feature = "serde")]
pub use serde_feture::*;

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Eq, Hash, Ord, PartialEq, PartialOrd, Clone, Debug)]
pub struct UpliftDeskId(PeripheralId);

impl UpliftDeskId {
    pub(crate) fn new<I>(id: I) -> Self
    where
        I: Into<PeripheralId>,
    {
        Self(id.into())
    }

    pub async fn scan(adapter: &Adapter) -> Receiver<Result<UpliftDeskId>> {
        let (tx, rx) = mpsc::channel(10);

        let adapter = adapter.clone();
        tokio::spawn(async move {
            async fn inner(adapter: &Adapter, tx: &Sender<Result<UpliftDeskId>>) -> Result<()> {
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
                                if let Err(error) = tx.send(Ok(UpliftDeskId::new(id))).await {
                                    // the receiver has been dropped
                                    break Ok(())
                                }
                            }
                            Some(event ) => log::trace!("Unhandled Event: {:?}", event),
                            None => {
                                // Our adapter stopped looking for peripherals
                                break Err(btleplug::Error::NotConnected)
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
            if let Err(error) = adapter.stop_scan().await {
                log::error!("Failed to stop scanning: {error:?}");
            } else {
                log::trace!("Stopped Scanning")
            }

            if let Err(error) = result {
                if let Err(error) = tx.send(Err(error)).await {
                    log::warn!("Error when sending final error: {error:?}")
                }
            }
        });

        rx
    }

    pub async fn connect(&self, adapter: &Adapter) -> Result<UpliftDesk> {
        UpliftDesk::new(self.0.clone(), adapter).await
    }
}

impl From<Uuid> for UpliftDeskId {
    fn from(value: Uuid) -> Self {
        UpliftDeskId(value.into())
    }
}

#[cfg(feature = "serde")]
mod serde_feture {
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
        ) -> result::Result<UpliftDeskId, Box<dyn Error + 'static + Send + Sync>> {
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
    pub use super::*;
    use btleplug::api::Manager as _;
    use btleplug::platform::Manager;
    #[tokio::test]
    async fn test() {
        let manager = Manager::new().await.unwrap();

        let adapters = manager.adapters().await.unwrap();
        let adapter = adapters.into_iter().next().unwrap();

        let mut rx = UpliftDeskId::scan(&adapter).await;

        let mut i = 10;
        while let Some(result) = rx.recv().await {
            println!("{result:?}");
            i -= 1;

            if i <= 0 {
                break;
            }
        }
    }
}

use btleplug::platform::PeripheralId;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use uuid::Uuid;

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

    pub fn parse(id: &str) -> Result<UpliftDeskId, uuid::Error> {
        Ok(UpliftDeskId::from(Uuid::parse_str(id)?))
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
        fn decode(value: <DB as Database>::ValueRef<'r>) -> Result<UpliftDeskId, BoxDynError> {
            let raw_value = <Vec<u8> as Decode<DB>>::decode(value)?;

            Ok(rmp_serde::from_slice(&raw_value)?)
        }
    }

    impl<'q, DB: Database> Encode<'q, DB> for UpliftDeskId
    where
        // make sure our DB supports binary
        Vec<u8>: Encode<'q, DB>,
    {
        fn encode_by_ref(
            &self,
            buf: &mut <DB as Database>::ArgumentBuffer,
        ) -> Result<IsNull, BoxDynError> {
            rmp_serde::to_vec(self)?.encode(buf)
        }
    }
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

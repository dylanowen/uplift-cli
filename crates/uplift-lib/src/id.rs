use btleplug::platform::PeripheralId;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display, Formatter};

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

    #[cfg(feature = "serde")]
    pub fn parse(id: &str) -> Result<UpliftDeskId, serde_json::Error> {
        serde_json::from_value::<PeripheralId>(serde_json::Value::String(id.to_string()))
            .map(UpliftDeskId)
    }
}

impl From<UpliftDeskId> for PeripheralId {
    fn from(val: UpliftDeskId) -> Self {
        val.0
    }
}

#[cfg(feature = "serde")]
impl Display for UpliftDeskId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if let Ok(serde_json::Value::String(s)) = serde_json::to_value(&self.0) {
            write!(f, "{s}")
        } else {
            Err(std::fmt::Error)
        }
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

#[cfg(all(test, target_os = "macos", feature = "serde"))]
mod tests {
    use super::*;

    const TEST_UUID: &str = "12345678-1234-1234-1234-123456789012";

    #[test]
    fn test_parse_raw_uuid() {
        let id = UpliftDeskId::parse(TEST_UUID).unwrap();
        assert_eq!(id.to_string(), TEST_UUID);
    }

    #[test]
    fn test_parse_round_trip() {
        let id = UpliftDeskId::parse(TEST_UUID).unwrap();
        let id2 = UpliftDeskId::parse(&id.to_string()).unwrap();
        assert_eq!(id, id2);
    }
}

use std::ffi::CString;

use corebluetooth_sys::id;
use corebluetooth_sys::NSString_NSStringExtensionMethods;
use corebluetooth_sys::CBUUID;

use crate::bluetooth::utils::EnhancedNsString;
use crate::bluetooth::BluetoothError;
use core::fmt;
use regex::Regex;
use std::fmt::{Debug, Display, Formatter};

/// CBUUID makes a lot of "claims" about what it can do, most of them are wrong
/// * it CAN'T seem to match across ids that have the bluetooth base appended manually
/// * it CAN't automatically add a bluetooth base on it's own when getting a string representation
///
/// This class attempts to handle those lies in it's documentation
#[derive(Clone, Eq, PartialEq, Hash)]
pub struct Uuid(uuid::Uuid);

const BLUETOOTH_BASE_UUID: &str = "-0000-1000-8000-00805f9b34fb";

impl Uuid {
    pub fn parse<S: Into<String>>(s: S) -> Result<Uuid, BluetoothError> {
        let mut s = s.into();
        // the uuid crate expects all 128 bits, so make sure to postfix our uuid with the bluetooth base
        if s.len() == 4 {
            s = format!("0000{}", s);
        }
        if s.len() == 8 {
            s = format!("{}{}", s, BLUETOOTH_BASE_UUID);
        }

        let inner_uuid = uuid::Uuid::parse_str(&s)
            .map_err(|e| BluetoothError(format!("Couldn't parse uuid({}): {}", s, e)))?;

        Ok(Uuid(inner_uuid))
    }

    pub fn cbuuid(&self) -> id {
        let normalized_uuid = self.to_short_string();
        unsafe {
            let c_string = CString::new(normalized_uuid).unwrap();
            let ns_string =
                <id as NSString_NSStringExtensionMethods>::stringWithUTF8String_(c_string.as_ptr());

            <id as CBUUID>::UUIDWithString_(ns_string) as id
        }
    }

    pub fn to_short_string(&self) -> String {
        lazy_static! {
            static ref BASE_RE: Regex =
                Regex::new(&format!("([a-z0-9]{{8}}){}", BLUETOOTH_BASE_UUID)).unwrap();
        }
        let mut short_uuid = self.0.to_string();
        if let Some(with_base) = BASE_RE.captures(&short_uuid) {
            short_uuid = with_base[1].to_string();
            // drop the first 4 zeroes if we have them
            if short_uuid.find("0000") == Some(0) {
                short_uuid = short_uuid[4..].to_string()
            }
        }

        short_uuid
    }
}

impl Display for Uuid {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_short_string())
    }
}

impl Debug for Uuid {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Uuid> for id {
    fn from(uuid: Uuid) -> Self {
        uuid.cbuuid()
    }
}

impl From<id> for Uuid {
    fn from(cbuuid: id) -> Self {
        unsafe {
            let ns_string = cbuuid.UUIDString() as id;
            let s = ns_string.to_rust();

            Uuid::parse(s).expect("CBUUID should be well formed")
        }
    }
}

impl From<uuid::Uuid> for Uuid {
    fn from(uuid: uuid::Uuid) -> Self {
        Uuid(uuid)
    }
}

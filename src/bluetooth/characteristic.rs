use core::fmt;
use std::fmt::{Debug, Display, Formatter};
use std::hash::{Hash, Hasher};

use objc::rc::StrongPtr;

use corebluetooth_sys::{id, CBAttribute};

use crate::bluetooth::Uuid;

pub struct Characteristic {
    pub(in crate::bluetooth) characteristic: StrongPtr,
}

impl Eq for Characteristic {}

impl PartialEq for Characteristic {
    fn eq(&self, other: &Self) -> bool {
        *self.characteristic == *other.characteristic
    }
}

impl Hash for Characteristic {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.characteristic.hash(state)
    }
}

impl Characteristic {
    pub fn new(characteristic: StrongPtr) -> Self {
        Characteristic { characteristic }
    }

    pub fn uuid(&self) -> Option<Uuid> {
        unsafe {
            let uuid = <id as CBAttribute>::UUID(*self.characteristic) as id;
            if !uuid.is_null() {
                Some(uuid.into())
            } else {
                None
            }
        }
    }
}

impl Display for Characteristic {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Characteristic(")?;
        if let Some(uuid) = self.uuid() {
            write!(f, "{}", uuid)?;
        }
        write!(f, ")")
    }
}

impl Debug for Characteristic {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Characteristic(")?;
        if let Some(uuid) = self.uuid() {
            write!(f, "{}", uuid)?;
        }
        write!(f, "@{:p})", self.characteristic)
    }
}

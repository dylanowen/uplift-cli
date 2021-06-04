use crate::bluetooth::characteristic::Characteristic;
use crate::bluetooth::Uuid;
use core::fmt;
use corebluetooth_sys::{id, CBAttribute, CBService, NSArray};
use objc::rc::StrongPtr;
use std::fmt::{Debug, Display, Formatter};

pub struct Service {
    pub(in crate::bluetooth) service: StrongPtr,
}

impl Service {
    pub fn new(service: StrongPtr) -> Self {
        Service { service }
    }

    pub fn uuid(&self) -> Uuid {
        unsafe { (<id as CBAttribute>::UUID(*self.service) as id).into() }
    }

    pub fn characteristics(&self) -> Vec<Characteristic> {
        unsafe {
            let mut characteristics = vec![];
            let characteristic_ptrs = self.service.characteristics() as id;
            let found_characteristics_count = <id as NSArray<id>>::count(characteristic_ptrs);

            for i in 0..found_characteristics_count {
                let characteristic_ptr =
                    <id as NSArray<id>>::objectAtIndex_(characteristic_ptrs, i) as id;
                let characteristic = Characteristic::new(StrongPtr::retain(characteristic_ptr));

                characteristics.push(characteristic)
            }

            characteristics
        }
    }
}

impl Display for Service {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Service({})", self.uuid())
    }
}

impl Debug for Service {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Service({}@{:p})", self.uuid(), self.service)
    }
}

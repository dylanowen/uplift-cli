use crate::bluetooth::characteristic::Characteristic;
use crate::bluetooth::delegate::ChanneledDelegate;
use crate::bluetooth::service::Service;
use crate::bluetooth::utils::{EnhancedIDArray, EnhancedNsString};
use crate::bluetooth::Advertisement::Connectable;
use crate::bluetooth::UUID;
use crate::group::GroupBy;
use core::{fmt, ptr, slice};
use corebluetooth_sys::{
    id, CBAdvertisementDataIsConnectable, CBAdvertisementDataLocalNameKey,
    CBAdvertisementDataManufacturerDataKey, CBAdvertisementDataOverflowServiceUUIDsKey,
    CBAdvertisementDataServiceDataKey, CBAdvertisementDataServiceUUIDsKey,
    CBAdvertisementDataSolicitedServiceUUIDsKey, CBAdvertisementDataTxPowerLevelKey,
    CBCharacteristic, CBCharacteristicWriteType_CBCharacteristicWriteWithoutResponse, CBPeripheral,
    NSArray, NSData, NSData_NSDataCreation, NSDictionary, NSError, NSNumber,
};
use futures::channel::mpsc::Receiver;
use futures::{Stream, StreamExt};
use objc::declare::ClassDecl;
use objc::rc::StrongPtr;
use objc::runtime::{Class, Object, Protocol, Sel, NO, YES};
use std::collections::HashMap;
use std::ffi::c_void;
use std::fmt::{Display, Formatter};
use std::marker::PhantomData;
use std::sync::Once;

pub struct Delegated;

pub type EventStream<E> = Box<dyn Stream<Item = E> + Unpin + Send>;

pub struct Peripheral<S> {
    pub(in crate::bluetooth) peripheral: StrongPtr,
    _delegate: Option<Delegate>,
    discovered_services: Option<EventStream<()>>,
    discovered_characteristics: Option<EventStream<Service>>,
    // updated_characteristic_value: Option<Box<dyn Stream<Item = (Characteristic, Vec<u8>)> + Unpin>>,
    //receiver: Option<Receiver<PeripheralEvent>>,
    _state: PhantomData<S>,
}

impl Peripheral<()> {
    pub fn new(peripheral: StrongPtr) -> Self {
        Peripheral {
            peripheral,
            _delegate: None,
            discovered_services: None,
            discovered_characteristics: None,
            // updated_characteristic_value: None,
            _state: PhantomData,
        }
    }

    pub fn with_delegate(
        self,
    ) -> (
        Peripheral<Delegated>,
        EventStream<(Characteristic, Vec<u8>)>,
    ) {
        unsafe {
            let (delegate, receiver) = Delegate::new();
            self.peripheral.setDelegate_(*delegate.0 as *mut u64);

            let discovered_services = receiver.group_by(
                |e| match e {
                    PeripheralEvent::DiscoveredServices => true,
                    _ => false,
                },
                |_| (),
            );
            let discovered_characteristics = discovered_services.add_group(
                |e| match e {
                    PeripheralEvent::DiscoveredCharacteristics(_) => true,
                    _ => false,
                },
                |e| match e {
                    PeripheralEvent::DiscoveredCharacteristics(s) => s,
                    _ => unreachable!(),
                },
            );
            let updated_characteristic_value = discovered_services.add_group(
                |e| match e {
                    PeripheralEvent::UpdatedCharacteristicValue(_) => true,
                    _ => false,
                },
                |e| match e {
                    PeripheralEvent::UpdatedCharacteristicValue(c) => {
                        let ns_data = <id as CBCharacteristic>::value(*c.characteristic) as id;

                        let length = ns_data.length();
                        let data = if length == 0 {
                            vec![]
                        } else {
                            let bytes = ns_data.bytes() as *const u8;

                            slice::from_raw_parts(bytes, length as usize).to_vec()
                        };

                        trace!("{:?} read: {:x?}", c, data);

                        (c, data)
                    }
                    _ => unreachable!(),
                },
            );

            (
                Peripheral {
                    peripheral: self.peripheral,
                    _delegate: Some(delegate),
                    discovered_services: Some(Box::new(discovered_services)),
                    discovered_characteristics: Some(Box::new(discovered_characteristics)),
                    // updated_characteristic_value: Some(Box::new(updated_characteristic_value)),
                    _state: PhantomData,
                },
                Box::new(updated_characteristic_value),
            )
        }
    }
}

impl<S> Peripheral<S> {
    pub fn name(&self) -> Option<String> {
        unsafe {
            let ns_string = self.peripheral.name() as id;

            if !ns_string.is_null() {
                Some(ns_string.to_rust())
            } else {
                None
            }
        }
    }
}

impl Peripheral<Delegated> {
    pub async fn discover_services(&mut self, mut uuids: HashMap<UUID, Vec<UUID>>) -> Vec<Service> {
        unsafe {
            let ns_uuids = uuids_for_objc(uuids.keys().cloned().collect());
            self.peripheral.discoverServices_(ns_uuids);
            self.discovered_services().await;

            let service_ptrs = self.peripheral.services() as id;
            let found_services_count = <id as NSArray<id>>::count(service_ptrs);

            for i in 0..found_services_count {
                let service_ptr = <id as NSArray<id>>::objectAtIndex_(service_ptrs, i) as id;
                let service = Service::new(StrongPtr::new(service_ptr));
                let characteristic_uuids =
                    uuids_for_objc(uuids.remove(&service.uuid()).unwrap_or_else(|| vec![]));

                self.peripheral
                    .discoverCharacteristics_forService_(characteristic_uuids, service_ptr);
            }

            let mut services = Vec::with_capacity(found_services_count as usize);
            while services.len() < found_services_count as usize {
                let service = self.discovered_characteristics().await;

                services.push(service);
            }

            services
        }
    }

    pub async fn write(&mut self, characteristic: &Characteristic, data: &[u8]) {
        unsafe {
            trace!("{} writing: {:x?}", characteristic, data);
            let data = <id as NSData_NSDataCreation>::dataWithBytes_length_(
                data.as_ptr() as *const c_void,
                data.len() as u64,
            ) as id;

            self.peripheral.writeValue_forCharacteristic_type_(
                data,
                *characteristic.characteristic,
                CBCharacteristicWriteType_CBCharacteristicWriteWithoutResponse,
            );

            // loop {
            //     match self.receiver.as_mut().unwrap().next().await {
            //         Some(PeripheralEvent::WroteCharacteristicValue(_)) => {
            //             break;
            //         }
            //         unexpected => warn!(
            //             "Found unexpected event while writing to characteristic: {:?}",
            //             unexpected
            //         ),
            //     }
            // }
        }
    }

    // pub fn read(&mut self, characteristic: &Characteristic) {
    //     unsafe {
    //         self.peripheral
    //             .readValueForCharacteristic_(*characteristic.characteristic);
    //
    //         //self.listen(characteristic).await
    //     }
    // }

    // pub async fn listen(&mut self, characteristic: &Characteristic) -> Vec<u8> {
    //     loop {
    //         let found = self.updated_characteristic_values().await;
    //         if found.len() == 1 && found.contains(characteristic) {
    //             break;
    //         } else {
    //             warn!(
    //                 "Found unexpected other characteristic while listening: {:?}",
    //                 found
    //             )
    //         }
    //     }
    //
    //     unsafe {
    //         let ns_data = <id as CBCharacteristic>::value(*characteristic.characteristic) as id;
    //
    //         let length = ns_data.length();
    //         if length == 0 {
    //             info!("data is 0?");
    //             return vec![];
    //         }
    //
    //         let bytes = ns_data.bytes() as *const u8;
    //         let data = slice::from_raw_parts(bytes, length as usize).to_vec();
    //
    //         trace!("{} read: {:x?}", characteristic, data);
    //
    //         data
    //     }
    // }

    // pub fn read_local_data(&mut self, characteristic: &Characteristic) -> Vec<u8> {
    //     unsafe {
    //         let ns_data = <id as CBCharacteristic>::value(*characteristic.characteristic) as id;
    //
    //         let length = ns_data.length();
    //         if length == 0 {
    //             info!("data is 0?");
    //             return vec![];
    //         }
    //
    //         let bytes = ns_data.bytes() as *const u8;
    //         let data = slice::from_raw_parts(bytes, length as usize).to_vec();
    //
    //         trace!("{} read: {:x?}", characteristic, data);
    //
    //         data
    //     }
    // }

    pub fn subscribe(&mut self, characteristic: &Characteristic) {
        unsafe {
            self.peripheral
                .setNotifyValue_forCharacteristic_(YES, *characteristic.characteristic)
        }
    }

    async fn discovered_services(&mut self) {
        self.discovered_services
            .as_mut()
            .unwrap()
            .next()
            .await
            .expect("We should discover some services")
    }

    async fn discovered_characteristics(&mut self) -> Service {
        self.discovered_characteristics
            .as_mut()
            .unwrap()
            .next()
            .await
            .expect("We should discover some characteristics")
    }

    // async fn updated_characteristic_values(&mut self) -> HashSet<Characteristic> {
    //     let mut characteristics: HashSet<Characteristic> = HashSet::new();
    //     loop {
    //         let next_future = self.updated_characteristic_value.as_mut().unwrap().next();
    //         match time::timeout(Duration::from_secs(0), next_future).await {
    //             Ok(Some((characteristic, data))) => {
    //                 characteristics.insert(characteristic);
    //             }
    //             Ok(None) => unreachable!(),
    //             Err(_) => {
    //                 // we couldn't find any values so break out
    //                 break;
    //             }
    //         }
    //     }
    //
    //     // we tried to find all the immediately available values, now just get at least 1
    //     if characteristics.is_empty() {
    //         let characteristic = self
    //             .updated_characteristic_value
    //             .as_mut()
    //             .unwrap()
    //             .next()
    //             .await
    //             .expect("We should discover an updated characteristic");
    //         characteristics.insert(characteristic);
    //     }
    //
    //     characteristics
    // }

    // async fn discover_characteristics(
    //     &mut self,
    //     uuids: Vec<UUID>,
    //     service: &Service,
    // ) -> Vec<Characteristic> {
    //     unsafe {
    //         let ns_uuids;
    //         if !uuids.is_empty() {
    //             ns_uuids = uuids.into_ns_array();
    //         } else {
    //             ns_uuids = ptr::null_mut();
    //         }
    //
    //         self.peripheral
    //             .discoverCharacteristics_forService_(ns_uuids, *service.service);
    //
    //         loop {
    //             match &mut self.receiver.as_mut().unwrap().next().await {
    //                 Some(PeripheralEvent::DiscoveredCharacteristics(_)) => {
    //                     break;
    //                 }
    //                 unexpected => warn!(
    //                     "Found unexpected event while discovering services: {:?}",
    //                     unexpected
    //                 ),
    //             }
    //         }
    //
    //         let service_ptrs = service.services() as id;
    //         let mut services =
    //             Vec::with_capacity(<id as NSArray<id>>::count(service_ptrs) as usize);
    //         for i in 0..<id as NSArray<id>>::count(service_ptrs) {
    //             let service_ptr = <id as NSArray<id>>::objectAtIndex_(service_ptrs, i) as id;
    //             let service = Service::new(StrongPtr::new(service_ptr));
    //
    //             services.push(service);
    //         }
    //     }
    //
    //     //self.peripheral.discoverDescriptorsForCharacteristic_()
    // }
}

fn uuids_for_objc(uuids: Vec<UUID>) -> id {
    if !uuids.is_empty() {
        uuids.into_ns_array()
    } else {
        ptr::null_mut()
    }
}

impl<S> Display for Peripheral<S> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(name) => write!(f, "{}", name),
            None => write!(f, "<no-name>"),
        }
    }
}

#[derive(Debug)]
pub enum PeripheralEvent {
    DiscoveredServices,
    DiscoveredCharacteristics(Service),
    UpdatedCharacteristicValue(Characteristic),
    WroteCharacteristicValue(Characteristic),
}

// TODO is this even allowed?
unsafe impl Send for PeripheralEvent {}

const DELEGATE_CLASS_NAME: &str = "MyPeripheralDelegate";

struct Delegate(StrongPtr);

impl Delegate {
    fn new() -> (Self, Receiver<PeripheralEvent>) {
        unsafe {
            let raw_delegate = Delegate::init();
            let receiver = Delegate::take_receiver(raw_delegate);
            let delegate = StrongPtr::new(raw_delegate);

            (Delegate(delegate), receiver)
        }
    }

    extern "C" fn discovered_services(
        delegate: &mut Object,
        _cmd: Sel,
        _peripheral: id,
        error: id,
    ) {
        unsafe {
            trace_callback("Discovered Services", error);

            Self::send_event(delegate, PeripheralEvent::DiscoveredServices);
        }
    }

    extern "C" fn discovered_characteristics(
        delegate: &mut Object,
        _cmd: Sel,
        _peripheral: id,
        service: id,
        error: id,
    ) {
        unsafe {
            trace_callback("Discovered Characteristics", error);
            let service = Service::new(StrongPtr::retain(service));

            Self::send_event(
                delegate,
                PeripheralEvent::DiscoveredCharacteristics(service),
            );
        }
    }

    extern "C" fn updated_characteristic_value(
        delegate: &mut Object,
        _cmd: Sel,
        _peripheral: id,
        characteristic: id,
        error: id,
    ) {
        unsafe {
            trace_callback("Updated Characteristic Value", error);
            let characteristic = Characteristic::new(StrongPtr::retain(characteristic));

            Self::send_event(
                delegate,
                PeripheralEvent::UpdatedCharacteristicValue(characteristic),
            );
        }
    }

    extern "C" fn wrote_characteristic_value(
        delegate: &mut Object,
        _cmd: Sel,
        _peripheral: id,
        characteristic: id,
        error: id,
    ) {
        unsafe {
            trace_callback("Wrote Characteristic Value", error);
            let characteristic = Characteristic::new(StrongPtr::retain(characteristic));

            Self::send_event(
                delegate,
                PeripheralEvent::WroteCharacteristicValue(characteristic),
            );
        }
    }
    extern "C" fn did_update_characteristic_notification_state(
        _delegate: &mut Object,
        _cmd: Sel,
        _peripheral: id,
        _characteristic: id,
        error: id,
    ) {
        unsafe {
            trace_callback("Updated Characteristic Notification State", error);
            //let characteristic = Characteristic::new(StrongPtr::retain(characteristic));

            // Self::send_event(
            //     delegate,
            //     PeripheralEvent::WroteCharacteristicValue(characteristic),
            // );
        }
    }
}

unsafe fn trace_callback(message: &str, error: id) {
    trace!("{}", message);
    if !error.is_null() {
        warn!(
            "{} Error: {}",
            message,
            (error.localizedDescription() as id).to_rust()
        );
    }
}

impl ChanneledDelegate<PeripheralEvent> for Delegate {
    fn delegate_class() -> &'static Class {
        static REGISTER_DELEGATE_CLASS: Once = Once::new();
        let mut decl =
            ClassDecl::new(DELEGATE_CLASS_NAME, Class::get("NSObject").unwrap()).unwrap();

        REGISTER_DELEGATE_CLASS.call_once(|| {
            decl.add_protocol(Protocol::get("CBPeripheralDelegate").unwrap());

            decl.add_ivar::<*mut c_void>(Self::DELEGATE_SENDER_IVAR);
            decl.add_ivar::<*mut c_void>(Self::DELEGATE_RECEIVER_IVAR);
            unsafe {
                // Initialization
                decl.add_method(
                    sel!(init),
                    Self::init_impl as extern "C" fn(&mut Object, Sel) -> id,
                );

                // Discovering Services
                decl.add_method(
                    sel!(peripheral:didDiscoverServices:),
                    Self::discovered_services as extern "C" fn(&mut Object, Sel, id, id),
                );

                // Discovering Characteristics
                decl.add_method(
                    sel!(peripheral:didDiscoverCharacteristicsForService:error:),
                    Self::discovered_characteristics as extern "C" fn(&mut Object, Sel, id, id, id),
                );

                // Retrieving Characteristic and Descriptor Values
                decl.add_method(
                    sel!(peripheral:didUpdateValueForCharacteristic:error:),
                    Self::updated_characteristic_value
                        as extern "C" fn(&mut Object, Sel, id, id, id),
                );

                // Writing Characteristic and Descriptor Values
                decl.add_method(
                    sel!(peripheral:didWriteValueForCharacteristic:error:),
                    Self::wrote_characteristic_value as extern "C" fn(&mut Object, Sel, id, id, id),
                );

                // Managing Notifications for a Characteristics Value
                decl.add_method(
                    sel!(peripheral:didUpdateNotificationStateForCharacteristic:error:),
                    Self::did_update_characteristic_notification_state
                        as extern "C" fn(&mut Object, Sel, id, id, id),
                );
            }

            decl.register();
        });

        Class::get(DELEGATE_CLASS_NAME).unwrap()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Advertisement {
    LocalNameKey(String),
    TxPowerLevel(i64),
    Connectable(bool),
}

impl Advertisement {
    pub(in crate::bluetooth) unsafe fn parse(
        data: id, /* NSDictionary<NSString *,id> */
    ) -> Vec<Advertisement> {
        let mut results = vec![];

        let mut value;
        value = <id as NSDictionary<id, id>>::objectForKey_(
            data,
            CBAdvertisementDataLocalNameKey as u64,
        ) as id;
        if !value.is_null() {
            let s = value.to_rust();
            results.push(Advertisement::LocalNameKey(s))
        }
        value = <id as NSDictionary<id, id>>::objectForKey_(
            data,
            CBAdvertisementDataManufacturerDataKey as u64,
        ) as id;
        if !value.is_null() {
            trace!("Found manufacture data")
        }
        value = <id as NSDictionary<id, id>>::objectForKey_(
            data,
            CBAdvertisementDataServiceDataKey as u64,
        ) as id;
        if !value.is_null() {
            trace!("Found service data")
        }
        value = <id as NSDictionary<id, id>>::objectForKey_(
            data,
            CBAdvertisementDataServiceUUIDsKey as u64,
        ) as id;
        if !value.is_null() {
            trace!("Found service UUIDs")
        }
        value = <id as NSDictionary<id, id>>::objectForKey_(
            data,
            CBAdvertisementDataOverflowServiceUUIDsKey as u64,
        ) as id;
        if !value.is_null() {
            trace!("Found overflow service UUIDs")
        }
        value = <id as NSDictionary<id, id>>::objectForKey_(
            data,
            CBAdvertisementDataTxPowerLevelKey as u64,
        ) as id;
        if !value.is_null() {
            results.push(Advertisement::TxPowerLevel(value.longValue()));
        }
        value = <id as NSDictionary<id, id>>::objectForKey_(
            data,
            CBAdvertisementDataIsConnectable as u64,
        ) as id;
        if !value.is_null() {
            results.push(Connectable(value.boolValue() != NO));
        }
        value = <id as NSDictionary<id, id>>::objectForKey_(
            data,
            CBAdvertisementDataSolicitedServiceUUIDsKey as u64,
        ) as id;
        if !value.is_null() {
            trace!("Found solicited service UUIDs")
        }

        results
    }
}

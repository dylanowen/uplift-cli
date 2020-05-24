use std::ffi::c_void;
use std::ffi::CString;
use std::ptr;

use futures::channel::mpsc::Receiver;
use objc::rc::StrongPtr;
use objc::runtime::{Class, Object, Protocol, Sel, YES};

use corebluetooth_sys::{
    dispatch_queue_create, id, CBCentralManager, CBCentralManagerScanOptionAllowDuplicatesKey,
    CBManager, CBManagerState, CBManagerState_CBManagerStatePoweredOff,
    CBManagerState_CBManagerStatePoweredOn, CBManagerState_CBManagerStateResetting,
    CBManagerState_CBManagerStateUnauthorized, CBManagerState_CBManagerStateUnsupported,
    NSMutableDictionary, NSMutableDictionary_NSMutableDictionaryCreation, NSNumber,
    NSNumber_NSNumberCreation, DISPATCH_QUEUE_SERIAL,
};

use crate::bluetooth::delegate::ChanneledDelegate;
use crate::bluetooth::utils::EnhancedIDArray;
use crate::bluetooth::uuid::UUID;
use crate::bluetooth::{Advertisement, Peripheral};
use core::fmt;
use objc::declare::ClassDecl;
use std::fmt::{Display, Formatter};
use std::sync::Once;

pub struct CentralManager {
    manager: StrongPtr,
    _delegate: Delegate,
}

impl CentralManager {
    pub fn new() -> (Self, Receiver<CentralManagerEvent>) {
        unsafe {
            let mut manager: id = msg_send![Class::get("CBCentralManager").unwrap(), alloc];
            let (delegate, receiver) = Delegate::new();

            let label = CString::new("CBQueue").unwrap();
            let queue = dispatch_queue_create(label.as_ptr(), DISPATCH_QUEUE_SERIAL);

            manager = manager.initWithDelegate_queue_(*delegate.0 as *mut u64, queue as id);

            let manager = StrongPtr::retain(manager);

            (
                Self {
                    manager,
                    _delegate: delegate,
                },
                receiver,
            )
        }
    }

    pub fn start_scan(&self, service_uuids: Vec<UUID>) {
        unsafe {
            let yes = <id as NSNumber_NSNumberCreation>::numberWithBool_(YES);

            let services = service_uuids.into_ns_array();
            // let services: id =
            //     <id as NSMutableArray_NSMutableArrayCreation<id>>::arrayWithCapacity_(
            //         service_uuids.len() as u64,
            //     );
            //
            // for uuid in service_uuids.into_iter() {
            //     let cbuuid = uuid.cbuuid();
            //
            //     NSMutableArray::<id>::addObject_(services, cbuuid as u64);
            // }

            let options = <id as NSMutableDictionary_NSMutableDictionaryCreation<id, id>>::dictionaryWithCapacity_(1);
            NSMutableDictionary::<id, id>::setObject_forKey_(
                options,
                yes as u64,
                CBCentralManagerScanOptionAllowDuplicatesKey as u64,
            );

            self.manager
                .scanForPeripheralsWithServices_options_(services, options);
        }
    }

    // pub fn is_scanning(&self) -> bool {
    //     unsafe { self.manager.isScanning() != NO }
    // }

    pub fn stop_scan(&self) {
        unsafe {
            self.manager.stopScan();
        }
    }

    pub fn connect<S>(&self, peripheral: &Peripheral<S>) {
        unsafe {
            self.manager
                .connectPeripheral_options_(*peripheral.peripheral, ptr::null_mut())
        }
    }

    pub fn disconnect<S>(&self, peripheral: &Peripheral<S>) {
        unsafe {
            self.manager
                .cancelPeripheralConnection_(*peripheral.peripheral)
        }
    }
}

pub enum CentralManagerEvent {
    PeripheralDiscovered(Peripheral<()>, Vec<Advertisement>, i64),
    PeripheralConnected(Peripheral<()>),
    PeripheralDisconnected(Peripheral<()>),
    PeripheralFailedToConnect(Peripheral<()>),
    StateUpdated(State),
}

#[derive(Debug)]
pub enum State {
    Unknown,
    Resetting,
    Unsupported,
    Unauthorized,
    PoweredOff,
    PoweredOn,
}

impl From<CBManagerState> for State {
    fn from(state: i64) -> Self {
        #[allow(non_upper_case_globals)] // https://github.com/rust-lang/rust/issues/39371
        match state {
            CBManagerState_CBManagerStateResetting => State::Resetting,
            CBManagerState_CBManagerStateUnsupported => State::Unsupported,
            CBManagerState_CBManagerStateUnauthorized => State::Unauthorized,
            CBManagerState_CBManagerStatePoweredOff => State::PoweredOff,
            CBManagerState_CBManagerStatePoweredOn => State::PoweredOn,
            _ => State::Unknown,
        }
    }
}

impl Display for State {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

const DELEGATE_CLASS_NAME: &str = "MyCentralManagerDelegate";

struct Delegate(StrongPtr);

impl Delegate {
    fn new() -> (Self, Receiver<CentralManagerEvent>) {
        unsafe {
            let raw_delegate = Delegate::init();
            let receiver = Delegate::take_receiver(raw_delegate);
            let delegate = StrongPtr::retain(raw_delegate);

            (Delegate(delegate), receiver)
        }
    }

    extern "C" fn peripheral_connected(
        delegate: &mut Object,
        _cmd: Sel,
        _central: id,
        peripheral: id,
    ) {
        unsafe {
            let peripheral = Peripheral::new(StrongPtr::retain(peripheral));

            trace!("Peripheral Connected '{}'", peripheral);

            let event = CentralManagerEvent::PeripheralConnected(peripheral);
            Self::send_event(delegate, event);
        }
    }

    extern "C" fn peripheral_disconnected(
        delegate: &mut Object,
        _cmd: Sel,
        _central: id,
        peripheral: id,
        _error: id,
    ) {
        unsafe {
            let peripheral = Peripheral::new(StrongPtr::retain(peripheral));

            trace!("Peripheral Disconnected '{}'", peripheral);

            let event = CentralManagerEvent::PeripheralDisconnected(peripheral);
            Self::send_event(delegate, event);
        }
    }

    extern "C" fn peripheral_failed_to_connect(
        delegate: &mut Object,
        _cmd: Sel,
        _central: id,
        peripheral: id,
        _error: id,
    ) {
        unsafe {
            let peripheral = Peripheral::new(StrongPtr::retain(peripheral));

            trace!("Peripheral Failed To Connect '{}'", peripheral);

            let event = CentralManagerEvent::PeripheralFailedToConnect(peripheral);
            Self::send_event(delegate, event);
        }
    }

    extern "C" fn state_updated(delegate: &mut Object, _cmd: Sel, manager: id) {
        unsafe {
            let state = manager.state().into();

            trace!("State Updated '{}'", state);

            let event = CentralManagerEvent::StateUpdated(state);
            Self::send_event(delegate, event);
        }
    }

    // extern "C" fn will_restore_state(_delegate: &mut Object, _cmd: Sel, _central: id, _dict: id) {
    //     trace!("centralmanager_willrestorestate");
    // }

    extern "C" fn peripheral_discovered(
        delegate: &mut Object,
        _cmd: Sel,
        _central: id,
        peripheral: id,
        advertisements: id,
        rssi: id,
    ) {
        unsafe {
            let peripheral = Peripheral::new(StrongPtr::retain(peripheral));
            let advertisements = Advertisement::parse(advertisements);
            let rssi = rssi.longValue();

            trace!(
                "Peripheral Discovered '{}' @ {}: {:?}",
                peripheral,
                rssi,
                advertisements
            );

            let event = CentralManagerEvent::PeripheralDiscovered(peripheral, advertisements, rssi);
            Self::send_event(delegate, event)
        }
    }
}

impl Drop for Delegate {
    fn drop(&mut self) {
        unsafe {
            Delegate::drop_channels(*self.0);
        }
    }
}

impl ChanneledDelegate<CentralManagerEvent> for Delegate {
    fn delegate_class() -> &'static Class {
        static REGISTER_DELEGATE_CLASS: Once = Once::new();

        REGISTER_DELEGATE_CLASS.call_once(|| {
            let mut decl =
                ClassDecl::new(DELEGATE_CLASS_NAME, Class::get("NSObject").unwrap()).unwrap();

            decl.add_protocol(Protocol::get("CBCentralManagerDelegate").unwrap());

            decl.add_ivar::<*mut c_void>(Self::DELEGATE_SENDER_IVAR);
            decl.add_ivar::<*mut c_void>(Self::DELEGATE_RECEIVER_IVAR);
            decl.add_ivar::<bool>(Self::DROPPED_IVAR);
            unsafe {
                // Initialization
                decl.add_method(
                    sel!(init),
                    Self::init_impl as extern "C" fn(&mut Object, Sel) -> id,
                );

                // Peripheral Events
                decl.add_method(
                    sel!(centralManager:didConnectPeripheral:),
                    Self::peripheral_connected as extern "C" fn(&mut Object, Sel, id, id),
                );
                decl.add_method(
                    sel!(centralManager:didDisconnectPeripheral:error:),
                    Self::peripheral_disconnected as extern "C" fn(&mut Object, Sel, id, id, id),
                );
                decl.add_method(
                    sel!(centralManager:didFailToConnectPeripheral:error:),
                    Self::peripheral_failed_to_connect
                        as extern "C" fn(&mut Object, Sel, id, id, id),
                );

                // Discovering Peripherals Events
                decl.add_method(
                    sel!(centralManager:didDiscoverPeripheral:advertisementData:RSSI:),
                    Self::peripheral_discovered as extern "C" fn(&mut Object, Sel, id, id, id, id),
                );

                // CentralManager State Events
                decl.add_method(
                    sel!(centralManagerDidUpdateState:),
                    Self::state_updated as extern "C" fn(&mut Object, Sel, id),
                );
                // TODO we don't really need state restoration, so ignore this for now
                // decl.add_method(
                //     sel!(centralManager:willRestoreState:),
                //     Self::will_restore_state as extern "C" fn(&mut Object, Sel, id, id),
                // );
            }

            decl.register();
        });

        Class::get(DELEGATE_CLASS_NAME).unwrap()
    }
}

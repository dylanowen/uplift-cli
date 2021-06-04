use std::ffi::c_void;

use futures::channel::mpsc::channel;
use futures::channel::mpsc::Receiver;
use futures::channel::mpsc::Sender;
use futures::sink::SinkExt;
use objc::runtime::{Class, Object, Sel};

use corebluetooth_sys::id;

pub trait ChanneledDelegate<Event> {
    const DELEGATE_SENDER_IVAR: &'static str = "_sender";
    const DELEGATE_RECEIVER_IVAR: &'static str = "_receiver";
    const DROPPED_IVAR: &'static str = "_dropped";

    fn init() -> id {
        unsafe {
            let mut delegate: id = msg_send![Self::delegate_class(), alloc];
            delegate = msg_send![delegate, init];
            delegate
        }
    }

    /// This can never be called multiple times, we're explicitly taking the value out of the ObjC Object
    unsafe fn take_receiver(delegate: id) -> Receiver<Event> {
        let boxed = Box::from_raw(
            *(&*delegate).get_ivar::<*mut c_void>(Self::DELEGATE_RECEIVER_IVAR)
                as *mut Receiver<Event>,
        );

        *boxed
    }

    unsafe fn send_event(delegate: id, event: Event) {
        if !Self::dropped(delegate) {
            let sender = *(&*delegate).get_ivar::<*mut c_void>(Self::DELEGATE_SENDER_IVAR)
                as *mut Sender<Event>;

            if let Err(e) = futures::executor::block_on((*sender).send(event)) {
                error!("Couldn't send delegate event: {}", e)
            }
        }
    }

    /// Check to see if we've already dropped this delegate
    unsafe fn dropped(delegate: id) -> bool {
        *(&*delegate).get_ivar::<bool>(Self::DROPPED_IVAR)
    }

    unsafe fn drop_channels(delegate: id) {
        if !Self::dropped(delegate) {
            let _ = Box::from_raw(
                *(&*delegate).get_ivar::<*mut c_void>(Self::DELEGATE_SENDER_IVAR)
                    as *mut Sender<Event>,
            );

            (&mut *delegate).set_ivar::<bool>(Self::DROPPED_IVAR, true);
        }
    }

    fn delegate_class() -> &'static Class;

    extern "C" fn init_impl(delegate: &mut Object, _cmd: Sel) -> id {
        let (sender, receiver) = channel::<Event>(256);

        let sendbox = Box::new(sender);
        let recvbox = Box::new(receiver);
        unsafe {
            delegate.set_ivar::<*mut c_void>(
                Self::DELEGATE_SENDER_IVAR,
                Box::into_raw(sendbox) as *mut c_void,
            );
            delegate.set_ivar::<*mut c_void>(
                Self::DELEGATE_RECEIVER_IVAR,
                Box::into_raw(recvbox) as *mut c_void,
            );
            delegate.set_ivar::<bool>(Self::DROPPED_IVAR, false);
        }
        delegate
    }
}

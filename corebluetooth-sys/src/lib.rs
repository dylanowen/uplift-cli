#![cfg(any(target_os = "macos", target_os = "ios"))]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

#[macro_use]
extern crate objc;

use core::ptr;
use std::os::raw::c_char;

use objc::runtime::Object;

pub use bindings::*;

#[allow(clippy::all)]
mod bindings {
    include!(concat!(env!("OUT_DIR"), "/corebluetooth.rs"));
}

// BUG: the generated code for dispatch_queue_create returns a *mut id. Which
// seems correct based on https://developer.apple.com/documentation/dispatch/dispatch_queue_t?language=objc
// but at runtime this throws a NPE
pub const DISPATCH_QUEUE_SERIAL: id = ptr::null::<Object>() as id;

#[link(name = "AppKit", kind = "framework")]
#[link(name = "Foundation", kind = "framework")]
#[link(name = "CoreBluetooth", kind = "framework")]
extern "C" {
    pub fn dispatch_queue_create(label: *const c_char, attr: id) -> id;
}

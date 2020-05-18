use std::ffi::CStr;

use corebluetooth_sys::{
    id, NSMutableArray, NSMutableArray_NSMutableArrayCreation, NSString_NSStringExtensionMethods,
};
use objc::runtime::Object;

pub trait EnhancedNsString {
    fn to_rust(&self) -> String;
}

impl EnhancedNsString for id {
    fn to_rust(&self) -> String {
        unsafe {
            let c_string = NSString_NSStringExtensionMethods::UTF8String(*self);
            String::from(CStr::from_ptr(c_string).to_str().unwrap())
        }
    }
}

pub trait EnhancedIDArray {
    fn into_ns_array(self) -> id;
}

impl<I: Into<id>> EnhancedIDArray for Vec<I> {
    fn into_ns_array(self) -> *mut Object {
        unsafe {
            let ns_array: id =
                <id as NSMutableArray_NSMutableArrayCreation<id>>::arrayWithCapacity_(
                    self.len() as u64
                );

            for element in self.into_iter() {
                let ptr: id = element.into();

                NSMutableArray::<id>::addObject_(ns_array, ptr as u64);
            }

            ns_array
        }
    }
}

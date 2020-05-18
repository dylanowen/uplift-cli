use bindgen;
use std::env;
use std::path::PathBuf;

fn build(sdk_path: Option<&str>, target: &str) {
    // See https://github.com/rust-lang/rust-bindgen/issues/1211
    // Technically according to the llvm mailing list, the argument to clang here should be
    // -arch arm64 but it looks cleaner to just change the target.
    let target = if target == "aarch64-apple-ios" {
        "arm64-apple-ios"
    } else {
        target
    };

    let mut headers: Vec<&str> = vec![];
    println!("cargo:rustc-link-lib=framework=CoreBluetooth");
    headers.push("CoreBluetooth/CoreBluetooth.h");

    println!("cargo:rerun-if-env-changed=BINDGEN_EXTRA_CLANG_ARGS");

    let meta_header: Vec<_> = headers
        .iter()
        .map(|h| format!("#include <{}>\n", h))
        .collect();

    // Begin building the bindgen params.
    let builder = bindgen::Builder::default()
        .clang_args(&["-x", "objective-c"])
        .clang_args(&[&format!("--target={}", target)])
        //.objc_extern_crate(true)
        // .size_t_is_usize(true)
        .block_extern_crate(true)
        .rustfmt_bindings(true)
        .clang_args(&["-isysroot", sdk_path.unwrap()])
        .trust_clang_mangling(false)
        .derive_default(true)
        .header_contents("CoreBluetooth.h", &meta_header.concat())
        .whitelist_recursively(true)
        .whitelist_var("CBAdvertisementDataLocalNameKey")
        .whitelist_var("CBAdvertisementDataTxPowerLevelKey")
        .whitelist_var("CBAdvertisementDataServiceUUIDsKey")
        .whitelist_var("CBAdvertisementDataServiceDataKey")
        .whitelist_var("CBAdvertisementDataManufacturerDataKey")
        .whitelist_var("CBAdvertisementDataOverflowServiceUUIDsKey")
        .whitelist_var("CBAdvertisementDataIsConnectable")
        .whitelist_var("CBAdvertisementDataSolicitedServiceUUIDsKey")
        .whitelist_var("CBCentralManagerOptionShowPowerAlertKey")
        .whitelist_var("CBCentralManagerOptionRestoreIdentifierKey")
        .whitelist_var("CBCentralManagerScanOptionAllowDuplicatesKey")
        .whitelist_var("CBCentralManagerScanOptionSolicitedServiceUUIDsKey")
        .whitelist_var("CBConnectPeripheralOptionNotifyOnConnectionKey")
        .whitelist_var("CBConnectPeripheralOptionNotifyOnDisconnectionKey")
        .whitelist_var("CBConnectPeripheralOptionNotifyOnNotificationKey")
        .whitelist_var("CBConnectPeripheralOptionStartDelayKey")
        .whitelist_var("CBConnectPeripheralOptionEnableTransportBridgingKey")
        .whitelist_var("CBConnectPeripheralOptionRequiresANCS")
        .whitelist_var("CBCentralManagerRestoredStatePeripheralsKey")
        .whitelist_var("CBCentralManagerRestoredStateScanServicesKey")
        .whitelist_var("CBCentralManagerRestoredStateScanOptionsKey")
        .whitelist_var("DISPATCH_QUEUE_SERIAL")
        .whitelist_type("DispatchQueue")
        .whitelist_type("NSData")
        .whitelist_type("NSData_NSDataCreation")
        .whitelist_type("NSString")
        .whitelist_type("NSString_NSStringExtensionMethods")
        .whitelist_type("NSMutableArray")
        .whitelist_type("NSMutableArray_NSMutableArrayCreation")
        .whitelist_type("*NSNumber*")
        .whitelist_type("NSNumber_NSNumberCreation")
        .whitelist_type("NSMutableDictionary")
        .whitelist_type("NSMutableDictionary_NSMutableDictionaryCreation")
        .whitelist_type("NSValue")
        .whitelist_type("NSMutableArray")
        .whitelist_type("CBUUID")
        .whitelist_type("CBManager")
        .whitelist_type("CBCentralManager")
        .whitelist_type("CBPeripheral")
        .whitelist_type("CBAttribute")
        .whitelist_type("NSError")
        .blacklist_function("dispatch_queue_create")
        .blacklist_type("dispatch_queue_t");

    let bindings = builder.generate().expect("unable to generate bindings");

    // Get the cargo out directory.
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("env variable OUT_DIR not found"));

    // Write them to the crate root.
    println!("{:?}", out_dir);
    bindings
        .write_to_file(out_dir.join("corebluetooth.rs"))
        .expect("could not write bindings");
    // bindings
    //     .write_to_file("src/corebluetooth.rs")
    //     .expect("could not write bindings");
}

fn main() {
    let target = std::env::var("TARGET").unwrap();
    // if !target.contains("apple-ios") {
    //     panic!("uikit-sys requires macos or ios target");
    // }

    let directory = sdk_path(&target).ok();
    println!("{:?}", directory);
    build(directory.as_ref().map(String::as_ref), &target);
}

fn sdk_path(target: &str) -> Result<String, std::io::Error> {
    use std::process::Command;

    let sdk = if target.contains("apple-darwin") {
        "macosx"
    } else if target == "x86_64-apple-ios" || target == "i386-apple-ios" {
        "iphonesimulator"
    } else if target == "aarch64-apple-ios"
        || target == "armv7-apple-ios"
        || target == "armv7s-apple-ios"
    {
        "iphoneos"
    } else {
        unreachable!();
    };

    let output = Command::new("xcrun")
        .args(&["--sdk", sdk, "--show-sdk-path"])
        .output()?
        .stdout;
    let prefix_str = std::str::from_utf8(&output).expect("invalid output from `xcrun`");
    Ok(prefix_str.trim_end().to_string())
}

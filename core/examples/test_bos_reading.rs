use nusb::DeviceInfo;
use fsct_core::usb::fsct_bos_finder;
use fsct_core::usb::fsct_bos_finder::get_fsct_vendor_subclass_number_from_device;

fn find_device_with_fsct_vendor_subclass_number() -> Option<DeviceInfo> {
    let devices = nusb::list_devices()
        .map_err(|e| format!("Failed to list devices: {}", e))
        .unwrap();
    for device in devices {
        let result = get_fsct_vendor_subclass_number_from_device(&device).ok();
        if result.is_some() {
            return Some(device);
        }
    }
    None
}


fn main() {
    let device = find_device_with_fsct_vendor_subclass_number();
    if device.is_none() {
        println!("No device with Ferrum Streaming Control Technology interface found");
        return;
    }
    let device = device.unwrap();

    println!("Device with Ferrum Streaming Control Technology capability found: \"{}\" ({:04X}:{:04X})", device.product_string().unwrap_or("Unknown"), device.vendor_id(), device.product_id());

    let fsct_cap = fsct_bos_finder::get_fsct_vendor_subclass_number_from_device(&device);
    match fsct_cap {
        Ok(fsct_cap) => {
            println!("Vendor subclass number of Ferrum Streaming Control Technology interface: 0x{:02X}", fsct_cap);
        }
        Err(e) => {
            println!("Ferrum Streaming Control Technology interface Vendor subclass number not provided in BOS \
            descriptor, e: {e}");
        }
    }
}
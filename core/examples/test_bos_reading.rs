use fsct_core::usb::fsct_bos_finder;

fn main() {
    let device = fsct_bos_finder::find_device_with_fsct_vendor_subclass_number();
    if device.is_none() {
        println!("No device with Ferrum Streaming Control Technology interface found");
        return;
    }
    let device = device.unwrap();

    println!("Device with Ferrum Streaming Control Technology capability found: \"{}\" ({:04X}:{:04X})", device.product_string().unwrap_or("Unknown"), device.vendor_id(), device.product_id());

    let fsct_cap = fsct_bos_finder::get_fsct_vendor_subclass_number_from_device(&device).unwrap();
    match fsct_cap {
        Some(fsct_cap) => {
            println!("Vendor subclass number of Ferrum Streaming Control Technology interface: 0x{:02X}", fsct_cap);
        }
        None => {
            println!("Ferrum Streaming Control Technology interface Vendor subclass number not provided in BOS descriptor");
        }
    }
}
use std::time::Duration;
use nusb::DeviceInfo;
use nusb::transfer::{Control, ControlType, Recipient};

fn find_dfu_interface(device_info: &DeviceInfo) -> Option<nusb::InterfaceInfo> {
    device_info.interfaces().find(|interface| interface.class() == 0xFE).cloned()
}

fn main() {
    let device = nusb::list_devices().unwrap().find(|device| { device.vendor_id() == 0x3336 });
    if device.is_none() {
        println!("No HEM device");
        return;
    }
    let device = device.unwrap();

    let device_firmware_extended = device.open().unwrap().get_string_descriptor(0xFC, 0, Duration::from_secs(5))
                                         .ok();
    if let Some(device_firmware_extended) = device_firmware_extended {
        println!("Device firmware extended: {:?}", device_firmware_extended);
    }

    println!("HEM device found: {:?}", device);

    let dfu_interface = find_dfu_interface(&device);
    match dfu_interface {
        Some(interface_info) => {
            println!("DFU interface found: {:?}", interface_info);
            println!("Trying to open DFU interface...");
            let interface = device.open().unwrap().claim_interface(interface_info.interface_number());
            match interface {
                Ok(interface) => {
                    println!("DFU interface opened successfully");
                    println!("Trying to send detach DFU request...");
                    let result = interface.control_out_blocking(Control {
                        control_type: ControlType::Class,
                        index: interface_info.interface_number() as u16,
                        recipient: Recipient::Interface,
                        request: 0,
                        value: 1000,
                    }, &[], Duration::from_secs(5));
                    match result {
                        Ok(_) => {
                            println!("Detach DFU request sent successfully");
                        }
                        Err(e) => {
                            println!("Failed to send detach DFU request: {}", e);
                        }
                    }
                }
                Err(e) => {
                    println!("Failed to open DFU interface: {}", e);
                }
            }
        }
        None => {
            println!("DFU interface not found");
        }
    }
}
use dac_player_integration::usb::descriptors::*;
use dac_player_integration::usb::fsct_bos_finder::get_fsct_vendor_subclass_number_from_device;
use dac_player_integration::usb::{find_fsct_interface_number, get_fsct_functionality_descriptor_set};
use nusb::DeviceInfo;
use dac_player_integration::usb;

#[tokio::main]
async fn main() -> Result<(), String> {
    let devices = nusb::list_devices()
        .map_err(|e| format!("Failed to list devices: {}", e))
        .unwrap();
    for device in devices {
        let result = get_fsct_vendor_subclass_number_from_device(&device);
        if result.is_err() {
            continue;
        }
        if let Some(fsct_vendor_subclass_number) = result? {
            let err = print_fsct_dump(&device, fsct_vendor_subclass_number).await;
            if err.is_err() {
                eprintln!("Error: {}", err.unwrap_err());
            }
        }
    }
    Ok(())
}

async fn print_fsct_dump(device_info: &DeviceInfo, fsct_vendor_subclass_number: u8) -> Result<(), String> {
    let fsct_interface_number = find_fsct_interface_number(&device_info, fsct_vendor_subclass_number);
    if fsct_interface_number.is_none() {
        println!("Device reports FSCT in BOS descriptor, but no Ferrum Streaming Control Technology interface found");
        return Ok(()); // ignore devices that report FSCT in BOS descriptor but don't have FSCT interface
    }
    let fsct_interface_number = fsct_interface_number.unwrap();
    let descriptor = get_fsct_functionality_descriptor_set(&device_info, fsct_interface_number).await?;
    println!(
        "Device with Ferrum Streaming Control Technology interface found: \"{}\" ({:04X}:{:04X})",
        device_info.product_string().unwrap_or("Unknown"),
        device_info.vendor_id(),
        device_info.product_id()
    );
    println!("FSCT interface number: {}", fsct_interface_number);

    for descriptor in usb::Descriptors(&descriptor) {
        match descriptor.descriptor_type() {
            FSCT_FUNCTIONALITY_DESCRIPTOR_ID => {
                let fsct_descriptor: FsctFunctionalityDescriptor = descriptor.try_into()?;
                println!("{:#?}", fsct_descriptor);
            }
            FSCT_IMAGE_METADATA_DESCRIPTOR_ID => {
                let fsct_descriptor: FsctImageMetadataDescriptor = descriptor.try_into()?;
                println!("{:#?}", fsct_descriptor);
            }
            FSCT_TEXT_METADATA_DESCRIPTOR_ID => {
                let fsct_descriptor: FsctTextMetadataDescriptor = descriptor.try_into()?;
                println!("{:#?}", fsct_descriptor);
            }
            _ => {}
        }
    }

    Ok(())
}

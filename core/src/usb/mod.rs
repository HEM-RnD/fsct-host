use nusb::DeviceInfo;

pub mod descriptors;
pub mod fsct_bos_finder;
pub mod descriptor_utils;
mod fsct_usb_interface;
pub mod fsct_device;
pub mod requests;

const FSCT_SUPPORTED_PROTOCOL_VERSION: u8 = 0x01;

fn check_fsct_interface_protocol(device_info: &DeviceInfo, fsct_interface_number: u8) -> Result<(), String> {
    let protocol = device_info
        .interfaces()
        .find(|i| i.interface_number() == fsct_interface_number)
        .map(|v| v.protocol());

    if protocol == Some(FSCT_SUPPORTED_PROTOCOL_VERSION) {
        Ok(())
    } else {
        Err(String::from("Not supported protocol version of FSCT interface"))
    }
}


pub async fn open_interface(device_info: &DeviceInfo, interface_number: u8) -> Result<nusb::Interface, String>
{
    let device = device_info.open().map_err(
        |e| format!("Failed to open device: {}", e)
    )?;
    let interface = device.claim_interface(interface_number)
                          .map_err(
                              |e| format!("Failed to claim interface {}: {}", interface_number, e)
                          )?;
    Ok(interface)
}

pub async fn create_and_configure_fsct_device(device_info: &DeviceInfo) -> Result<fsct_device::FsctDevice, String> {
    let fsct_vendor_subclass_number = fsct_bos_finder::get_fsct_vendor_subclass_number_from_device(device_info)?
        .ok_or_else(|| String::from("No FSCT BOS capability descriptor"))?;

    let fsct_interface_number = descriptor_utils::find_fsct_interface_number(device_info, fsct_vendor_subclass_number).ok_or_else(
        || String::from("No FSCT interface found")
    )?;
    check_fsct_interface_protocol(device_info, fsct_interface_number)?;
    let interface = open_interface(&device_info, fsct_interface_number).await?;
    let fsct_descriptors = descriptor_utils::get_fsct_functionality_descriptor_set(&interface).await?;
    let fsct_interface = fsct_usb_interface::FsctUsbInterface::new(interface);
    let mut fsct_device = fsct_device::FsctDevice::new(fsct_interface);
    fsct_device.init(&fsct_descriptors).await?;
    Ok(fsct_device)
}

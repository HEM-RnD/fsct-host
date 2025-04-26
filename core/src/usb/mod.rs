use nusb::DeviceInfo;
use crate::usb::errors::{FsctDeviceError, IoErrorOr};

pub mod descriptors;
pub mod fsct_bos_finder;
pub mod descriptor_utils;
mod fsct_usb_interface;
pub mod fsct_device;
pub mod requests;

pub mod errors;

const FSCT_SUPPORTED_PROTOCOL_VERSION: u8 = 0x01;

fn check_fsct_interface_protocol(device_info: &DeviceInfo, fsct_interface_number: u8) -> Result<(), FsctDeviceError> {
    let protocol = device_info
        .interfaces()
        .find(|i| i.interface_number() == fsct_interface_number)
        .map(|v| v.protocol())
        .ok_or(FsctDeviceError::InterfaceNotFound)?;


    if protocol == FSCT_SUPPORTED_PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(FsctDeviceError::ProtocolVersionNotSupported(protocol))
    }
}


pub async fn open_interface(device_info: &DeviceInfo, interface_number: u8) -> Result<nusb::Interface, IoErrorOr<FsctDeviceError>>
{
    let device = device_info.open()?;
    let interface = device.claim_interface(interface_number)?;
    Ok(interface)
}

pub async fn create_and_configure_fsct_device(device_info: &DeviceInfo) -> Result<fsct_device::FsctDevice, IoErrorOr<FsctDeviceError>> {
    let fsct_vendor_subclass_number = fsct_bos_finder::get_fsct_vendor_subclass_number_from_device(device_info)?;

    let fsct_interface_number = descriptor_utils::find_fsct_interface_number(device_info, fsct_vendor_subclass_number)?;
    check_fsct_interface_protocol(device_info, fsct_interface_number)?;
    let interface = open_interface(&device_info, fsct_interface_number).await?;
    let fsct_descriptors = descriptor_utils::get_fsct_functionality_descriptor_set(&interface).await?;
    let fsct_interface = fsct_usb_interface::FsctUsbInterface::new(interface);
    let mut fsct_device = fsct_device::FsctDevice::new(fsct_interface);
    fsct_device.init(&fsct_descriptors).await?;
    Ok(fsct_device)
}

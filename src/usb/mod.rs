pub mod definitions;
pub mod descriptors;
pub mod fsct_bos_finder;
pub mod descriptor_utils;
mod fsct_usb_interface;
pub mod fsct_device;
pub mod requests;

pub async fn create_fsct_device(device_info: &nusb::DeviceInfo) -> Option<fsct_device::FsctDevice> {
    let fsct_vendor_subclass_number = fsct_bos_finder::get_fsct_vendor_subclass_number_from_device(device_info)
        .ok()
        .flatten()?;

    let fsct_interface_number = descriptor_utils::find_fsct_interface_number(device_info, fsct_vendor_subclass_number)?;
    let interface = device_info.open().ok()?.claim_interface(fsct_interface_number).ok()?;
    let fsct_interface = fsct_usb_interface::FsctUsbInterface::new(interface);
    let mut fsct_device = fsct_device::FsctDevice::new(fsct_interface);
    fsct_device.synchronize_time().await.ok()?;
    fsct_device.fsct_interface().set_enable(true).await.ok()?;
    Some(fsct_device)
}

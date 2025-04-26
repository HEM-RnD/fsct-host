use nusb::DeviceInfo;
use std::time::Duration;
use uuid::Uuid;

#[repr(packed)]
#[derive(Debug, Copy, Clone)]
#[allow(non_snake_case)]
struct BosDescriptor {
    bLength: u8,
    bDescriptorType: u8,
    wTotalLength: u16,
    bNumDeviceCaps: u8,
}

use crate::usb::errors::{BosError, IoErrorOr};

#[repr(u8)]
#[derive(Debug, Copy, Clone)]
#[allow(dead_code)]
enum BosCapabilityType {
    WirelessUsb = 1,
    Usb2_0Extension = 2,
    SuperspeedUsb = 3,
    ContainerId = 4,
    Platform = 5,
    PowerDelivery = 6,
    BatteryInfo = 7,
    PdConsumerPort = 8,
    PdProviderPort = 9,
    SuperspeedPlus = 10,
    PrecisionTimeMeasurement = 11,
    WirelessUsbExt = 12,
    Billboard = 13,
    Authentication = 14,
    BillboardExt = 15,
    ConfigurationSummary = 16,
    FWStatus = 17,
}

#[repr(packed)]
#[derive(Debug, Copy, Clone)]
#[allow(non_snake_case)]
#[allow(dead_code)]
struct BosCapabilityDescriptor {
    bLength: u8,
    bDescriptorType: u8,
    bDevCapabilityType: u8,
    bReserved: u8,
}

#[derive(Debug, Clone)]
struct BosCapabilityDescWithData<'a> {
    length: usize,
    capability: BosCapabilityType,
    data: &'a [u8],
}

#[derive(Debug, Clone)]
struct PlatformCapability {
    uuid: Uuid,
    data: Vec<u8>,
}

fn decode_bos_descriptor(data: &[u8]) -> Result<BosDescriptor, BosError> {
    if data.len() < std::mem::size_of::<BosDescriptor>() {
        return Err(BosError::TooShort { name: "BosDescriptor", expected: std::mem::size_of::<BosDescriptor>(), actual: data.len() });
    }
    let descriptor: BosDescriptor =
        unsafe { *std::mem::transmute::<*const u8, &BosDescriptor>(data.as_ptr()) };
    if descriptor.bDescriptorType != 0x0F {
        return Err(BosError::WrongType {
            name: "BosDescriptor",
            expected: 0x0F,
            actual: descriptor.bDescriptorType,
        });
    }
    Ok(descriptor)
}

fn decode_bos_capability(data: &[u8]) -> Result<BosCapabilityDescWithData, BosError> {
    if data.len() < std::mem::size_of::<BosCapabilityDescriptor>() {
        return Err(BosError::TooShort { name: "BosCapabilityDescriptor", expected: std::mem::size_of::<BosCapabilityDescriptor>(), actual: data.len() });
    }
    let capability_desc: BosCapabilityDescriptor =
        unsafe { *std::mem::transmute::<*const u8, &BosCapabilityDescriptor>(data.as_ptr()) };
    if capability_desc.bLength as usize > data.len() {
        return Err(BosError::TooShort { name: "BosCapabilityDescriptor", expected: capability_desc.bLength as usize, actual: data.len() });
    }
    if capability_desc.bDescriptorType != 0x10 {
        return Err(BosError::WrongType { name: "BosCapabilityDescriptor", expected: 0x10, actual: capability_desc.bDescriptorType });
    }
    let data =
        &data[std::mem::size_of::<BosCapabilityDescriptor>()..(capability_desc.bLength as usize)];
    if capability_desc.bDevCapabilityType == 0 || capability_desc.bDevCapabilityType > 17 {
        return Err(BosError::CapabilityTypeMismatch(capability_desc.bDevCapabilityType));
    }
    let capability = unsafe { std::mem::transmute(capability_desc.bDevCapabilityType) };
    Ok(BosCapabilityDescWithData {
        length: capability_desc.bLength as usize,
        capability,
        data,
    })
}

fn decode_bos_descriptor_with_capabilities(
    data: &[u8],
) -> Result<Vec<BosCapabilityDescWithData>, BosError> {
    let descriptor = decode_bos_descriptor(data)?;
    let total_length = descriptor.wTotalLength as usize;
    if data.len() < total_length {
        return Err(BosError::TooShort { name: "BosDescriptor with capabilities", expected: total_length, actual: data.len() });
    }
    let mut capabilities = Vec::new();
    let mut offset = descriptor.bLength as usize;
    for _ in 0..descriptor.bNumDeviceCaps {
        let capability = decode_bos_capability(&data[offset..])?;
        offset += capability.length;
        capabilities.push(capability);
    }
    Ok(capabilities)
}

fn get_platform_capabilities(
    bos_capabilities: Vec<BosCapabilityDescWithData>,
) -> Result<Vec<PlatformCapability>, BosError> {
    let mut capabilities = Vec::new();
    for capability in bos_capabilities {
        match capability.capability {
            BosCapabilityType::Platform => {
                let uuid_bytes = capability.data[0..16]
                    .try_into()
                    .map_err(|_| BosError::TooShort { name: "PlatformCapability UUID", expected: 16, actual: 0 })?;
                let uuid = Uuid::from_bytes_le(uuid_bytes);
                capabilities.push(PlatformCapability {
                    uuid,
                    data: capability.data[16..].to_vec(),
                });
            }
            _ => {}
        }
    }
    Ok(capabilities)
}

#[allow(non_snake_case)]
#[repr(packed)]
#[derive(Debug, Copy, Clone)]
struct FSCTCapabilityDesc {
    capabilityDescriptorVersion: u16,
    vendorSubClassNumber: u8,
}

const FSCT_CAPABILITY_DESCRIPTOR_VERSION: u16 = 0x0100;
const FSCT_UUID: Uuid = Uuid::from_u128(0xc433beeb_8d00_4420_9515_bcb7faf38a41);

#[derive(Debug, Copy, Clone)]
#[allow(dead_code)]
struct FSCTCapability {
    vendor_sub_class_number: u8,
    version: (u8, u8),
}

fn get_fsct_capability(
    platform_capabilities: Vec<PlatformCapability>,
) -> Result<FSCTCapability, BosError> {
    for capability in platform_capabilities {
        if capability.uuid == FSCT_UUID {
            if capability.data.len() < std::mem::size_of::<FSCTCapabilityDesc>() {
                return Err(BosError::TooShort { name: "FSCT capability data", expected: std::mem::size_of::<FSCTCapabilityDesc>(), actual: capability.data.len() });
            }
            let fsct_capability: FSCTCapabilityDesc = unsafe {
                *std::mem::transmute::<*const u8, &FSCTCapabilityDesc>(capability.data.as_ptr())
            };
            if fsct_capability.capabilityDescriptorVersion != FSCT_CAPABILITY_DESCRIPTOR_VERSION {
                let capability_descriptor_version = fsct_capability.capabilityDescriptorVersion;
                return Err(BosError::FsctCapabilityVersionMismatch { expected: FSCT_CAPABILITY_DESCRIPTOR_VERSION, actual: capability_descriptor_version });
            }
            return Ok(FSCTCapability {
                vendor_sub_class_number: fsct_capability.vendorSubClassNumber,
                version: (
                    (fsct_capability.capabilityDescriptorVersion >> 8) as u8,
                    fsct_capability.capabilityDescriptorVersion as u8,
                ),
            });
        }
    }
    Err(BosError::NotFsctCapability)
}

fn get_fsct_vendor_subclass_number(
    platform_capabilities: Vec<PlatformCapability>,
) -> Result<u8, BosError> {
    Ok(get_fsct_capability(platform_capabilities)?.vendor_sub_class_number)
}

pub fn get_fsct_vendor_subclass_number_from_device(
    device: &DeviceInfo,
) -> Result<u8, IoErrorOr<BosError>> {
    if device.usb_version() <= 0x0200 {
        return Err(BosError::NotAvailable(device.usb_version()).into());
    }

    let handle = device.open()?;
    let desc = handle
        .get_descriptor(15, 0, 0, Duration::from_secs(1))?;
    let bos_desc = decode_bos_descriptor_with_capabilities(&desc)?;
    let platform_caps = get_platform_capabilities(bos_desc)?;
    Ok(get_fsct_vendor_subclass_number(platform_caps)?)
}

pub fn find_device_with_fsct_vendor_subclass_number() -> Option<DeviceInfo> {
    let devices = nusb::list_devices()
        .map_err(|e| format!("Failed to list devices: {}", e))
        .unwrap();
    for device in devices {
        let result = get_fsct_vendor_subclass_number_from_device(&device);
        if let Ok(_fsct_vendor_subclass_number) = result {
            return Some(device);
        }
    }
    None
}

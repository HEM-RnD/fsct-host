use std::time::Duration;
use nusb::{DeviceInfo};
use uuid::{Uuid};

#[repr(packed)]
#[derive(Debug, Copy, Clone)]
#[allow(non_snake_case)]
struct BosDescriptor {
    bLength: u8,
    bDescriptorType: u8,
    wTotalLength: u16,
    bNumDeviceCaps: u8,
}

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

fn decode_bos_descriptor(data: &[u8]) -> Result<BosDescriptor, String> {
    if data.len() < std::mem::size_of::<BosDescriptor>() {
        return Err(format!("Data too short to parse BosDescriptor, expected at least {} bytes, got {} bytes", std::mem::size_of::<BosDescriptor>(), data.len()));
    }
    let descriptor: BosDescriptor = unsafe { *std::mem::transmute::<*const u8, &BosDescriptor>(data.as_ptr()) };
    if descriptor.bDescriptorType != 0x0F {
        return Err(format!("Expected BOS descriptor type 0x0F, got 0x{:02X}", descriptor.bDescriptorType));
    }
    Ok(descriptor)
}

fn decode_bos_capability(data: &[u8]) -> Result<BosCapabilityDescWithData, String> {
    if data.len() < std::mem::size_of::<BosCapabilityDescriptor>() {
        return Err(format!("Data too short to parse BosCapabilityDescriptor, expected at least {} bytes, got {} bytes", std::mem::size_of::<BosCapabilityDescriptor>(), data.len()));
    }
    let capability_desc: BosCapabilityDescriptor = unsafe { *std::mem::transmute::<*const u8, &BosCapabilityDescriptor>(data.as_ptr()) };
    if capability_desc.bLength as usize > data.len() {
        return Err(format!("Data too short to parse BosCapabilityDescriptor with length {}, expected at least {} bytes, got {} bytes", capability_desc.bLength, capability_desc.bLength, data.len()));
    }
    if capability_desc.bDescriptorType != 0x10 {
        return Err(format!("Expected BosCapabilityDescriptor type 0x10, got 0x{:02X}", capability_desc.bDescriptorType));
    }
    let data = &data[std::mem::size_of::<BosCapabilityDescriptor>()..(capability_desc.bLength as usize)];
    if capability_desc.bDevCapabilityType == 0 || capability_desc.bDevCapabilityType > 17 {
        return Err(format!("Unknown BOS capability type: {}", capability_desc.bDevCapabilityType));
    }
    let capability = unsafe {std::mem::transmute(capability_desc.bDevCapabilityType)};
    Ok(BosCapabilityDescWithData {
        length: capability_desc.bLength as usize,
        capability,
        data,
    })
}

fn decode_bos_descriptor_with_capabilities(data: &[u8]) -> Result<Vec<BosCapabilityDescWithData>, String> {
    let descriptor = decode_bos_descriptor(data)?;
    let total_length = descriptor.wTotalLength as usize;
    if data.len() < total_length {
        return Err(format!("Data too short to parse BosDescriptor with capabilities, expected at least {} bytes, got {} bytes", total_length, data.len()));
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

fn get_platform_capabilities(bos_capabilities: Vec<BosCapabilityDescWithData>) -> Result<Vec<PlatformCapability>, String>
{
    let mut capabilities = Vec::new();
    for capability in bos_capabilities {
        match capability.capability {
            BosCapabilityType::Platform => {
                let uuid_bytes = capability.data[0..16].try_into().map_err(|_| "Platform capability data too short".to_string())?;
                let uuid = Uuid::from_bytes_le(uuid_bytes);
                capabilities.push(PlatformCapability {
                    uuid,
                    data: capability.data[16..].to_vec(),
                });
            },
            _ => {},
        }
    }
    Ok(capabilities)
}

#[allow(non_snake_case)]
#[repr(packed)]
#[derive(Debug, Copy, Clone)]
struct FSCTCapabilityDesc {
    capabilityDescriptorVersion: u16,
    vendorSubClassNumber: u8
}
const FSCT_CAPABILITY_DESCRIPTOR_VERSION: u16 = 0x0100;
const FSCT_UUID: Uuid = Uuid::from_u128(0xc433beeb_8d00_4420_9515_bcb7faf38a41);

#[derive(Debug, Copy, Clone)]
#[allow(dead_code)]
struct FSCTCapability {
    vendor_sub_class_number: u8,
    version: (u8, u8),
}

fn get_fsct_capability(platform_capabilities: Vec<PlatformCapability>) -> Result<Option<FSCTCapability>, String> {
    for capability in platform_capabilities {
        if capability.uuid == FSCT_UUID {
            if capability.data.len() < std::mem::size_of::<FSCTCapabilityDesc>() {
                return Err("FSCT capability data too short".to_string());
            }
            let fsct_capability: FSCTCapabilityDesc = unsafe { *std::mem::transmute::<*const u8, &FSCTCapabilityDesc>(capability.data.as_ptr()) };
            if fsct_capability.capabilityDescriptorVersion != FSCT_CAPABILITY_DESCRIPTOR_VERSION {
                let capability_descriptor_version = fsct_capability.capabilityDescriptorVersion;
                return Err(format!("Expected FSCT capability descriptor version 0x{:04X}, got 0x{:04X}", FSCT_CAPABILITY_DESCRIPTOR_VERSION, capability_descriptor_version));
            }
            return Ok(Some(FSCTCapability {
                vendor_sub_class_number: fsct_capability.vendorSubClassNumber,
                version: ((fsct_capability.capabilityDescriptorVersion >> 8) as u8, fsct_capability.capabilityDescriptorVersion as u8),
            }));
        }
    }
    Ok(None)
}

fn get_fsct_vendor_subclass_number(platform_capabilities: Vec<PlatformCapability>) -> Result<Option<u8>, String> {
    match get_fsct_capability(platform_capabilities)? {
        Some(fsct_capability) => Ok(Some(fsct_capability.vendor_sub_class_number)),
        None => Ok(None),
    }
}

fn get_fsct_vendor_subclass_number_from_device(device: &DeviceInfo) -> Result<Option<u8>, String> {
    let handle = device.open().map_err(|e| format!("Failed to open device: {}", e))?;
    let desc = handle.get_descriptor(
        15,
        0,
        0,
        Duration::from_secs(1),
    ).map_err(|e| format!("Failed to get descriptor: {}", e))?;
    let bos_desc = decode_bos_descriptor_with_capabilities(&desc)?;
    let platform_caps = get_platform_capabilities(bos_desc)?;
    get_fsct_vendor_subclass_number(platform_caps)
}

fn find_device_with_fsct_vendor_subclass_number() -> Result<Option<DeviceInfo>, String> {
    let devices = nusb::list_devices().map_err(|e| format!("Failed to list devices: {}", e))?;
    for device in devices {
        if let Some(_fsct_vendor_subclass_number) = get_fsct_vendor_subclass_number_from_device(&device)? {
            return Ok(Some(device));
        }
    }
    Ok(None)
}
fn main() {

    let device = find_device_with_fsct_vendor_subclass_number().unwrap();
    if device.is_none() {
        println!("No device with Ferrum Streaming Control Technology interface found");
        return;
    }
    let device = device.unwrap();

    println!("Device with Ferrum Streaming Control Technology capability found: \"{}\" ({:04X}:{:04X})", device.product_string().unwrap_or("Unknown"), device.vendor_id(), device.product_id());

    let fsct_cap = get_fsct_vendor_subclass_number_from_device(&device).unwrap();
    match fsct_cap {
        Some(fsct_cap) => {
            println!("Vendor subclass number of Ferrum Streaming Control Technology interface: 0x{:02X}", fsct_cap);
        },
        None => {
            println!("Ferrum Streaming Control Technology interface Vendor subclass number not provided in BOS descriptor");
        },
    }
}
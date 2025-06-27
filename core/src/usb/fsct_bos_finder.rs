// Copyright 2025 HEM Sp. z o.o.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// This file is part of an implementation of Ferrum Streaming Control Technology™,
// which is subject to additional terms found in the LICENSE-FSCT.md file.

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

use crate::usb::errors::{BosError, IoErrorOrAny};

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
}

#[repr(packed)]
#[derive(Debug, Copy, Clone)]
#[allow(non_snake_case)]
#[allow(dead_code)]
struct PlatformDataPartDescriptor {
    bReserved: u8,
    uuid: [u8; 16],
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
    if (capability_desc.bLength as usize) < std::mem::size_of::<BosCapabilityDescriptor>() {
        return Err(BosError::TooShort { 
            name: "BosCapabilityDescriptor data", 
            expected: std::mem::size_of::<BosCapabilityDescriptor>(), 
            actual: capability_desc.bLength as usize 
        });
    }     
    let data = if (capability_desc.bLength as usize) == std::mem::size_of::<BosCapabilityDescriptor>() { &[] }
    else { &data[std::mem::size_of::<BosCapabilityDescriptor>()..(capability_desc.bLength as usize)] };
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
                let platform_part_size = size_of::<PlatformDataPartDescriptor>();
                if capability.data.len() < size_of::<PlatformDataPartDescriptor>() {
                    return Err(BosError::TooShort { name: "PlatformCapabilityDescriptor - bReserved and UUID part", expected: 17, actual: capability.data.len() });
                };
                let platform_part: PlatformDataPartDescriptor =  
                    unsafe { *std::mem::transmute::<*const u8, &PlatformDataPartDescriptor>(capability.data.as_ptr()) };
                let uuid = Uuid::from_bytes_le(platform_part.uuid);
                capabilities.push(PlatformCapability {
                    uuid,
                    data: capability.data[platform_part_size..].to_vec(),
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
) -> Result<u8, IoErrorOrAny> {
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

#[cfg(test)]
mod tests {
    use super::*;
    
    const FSCT_PLATFORM_CAPABILITY_DATA: [u8; 20] = [
        0x00, // bReserved
        0xEB, 0xBE, 0x33, 0xC4, 0x00, 0x8D, 0x20, 0x44,
        0x95, 0x15, 0xBC, 0xB7, 0xFA, 0xF3, 0x8A, 0x41, // FSCT UUID
        0x00, 0x01, // FSCT desc version
        0x42, // FSCT vendorSubClassNumber
    ];

    fn create_bos_descriptor(total_length: u16, num_caps: u8) -> Vec<u8> {
        vec![
            5, // bLength
            0x0F, // bDescriptorType
            total_length as u8,
            (total_length >> 8) as u8,
            num_caps, // bNumDeviceCaps
        ]
    }

    fn create_capability_descriptor(cap_type: u8, data: &[u8]) -> Vec<u8> {
        let mut desc = vec![
            (3 + data.len()) as u8, // bLength 
            0x10, // bDescriptorType
            cap_type, // bDevCapabilityType
        ];
        desc.extend_from_slice(data);
        desc
    }

    #[test]
    fn test_fsct_capability_present() {
        let mut data = create_bos_descriptor(28, 1);
        data.extend(create_capability_descriptor(
            BosCapabilityType::Platform as u8,
            &FSCT_PLATFORM_CAPABILITY_DATA,
        ));

        let bos_caps = decode_bos_descriptor_with_capabilities(&data).unwrap();
        let platform_caps = get_platform_capabilities(bos_caps).unwrap();
        let vendor_subclass = get_fsct_vendor_subclass_number(platform_caps).unwrap();

        assert_eq!(vendor_subclass, 0x42);
    }

    #[test]
    fn test_fsct_capability_with_others() {
        let mut data = create_bos_descriptor(34, 2);
        data.extend(create_capability_descriptor(
            BosCapabilityType::SuperspeedUsb as u8,
            &[1, 2, 3],
        ));
        data.extend(create_capability_descriptor(
            BosCapabilityType::Platform as u8,
            &FSCT_PLATFORM_CAPABILITY_DATA,
        ));

        let bos_caps = decode_bos_descriptor_with_capabilities(&data).unwrap();
        let platform_caps = get_platform_capabilities(bos_caps).unwrap();
        let vendor_subclass = get_fsct_vendor_subclass_number(platform_caps).unwrap();

        assert_eq!(vendor_subclass, 0x42);
    }

    #[test]
    fn test_fsct_capability_with_short_other_capability() {
        let mut data = create_bos_descriptor(30, 2);
        data.extend(create_capability_descriptor(
            BosCapabilityType::PrecisionTimeMeasurement as u8,
            &[],
        ));
        data.extend(create_capability_descriptor(
            BosCapabilityType::Platform as u8,
            &FSCT_PLATFORM_CAPABILITY_DATA,
        ));

        let bos_caps = decode_bos_descriptor_with_capabilities(&data).unwrap();
        let platform_caps = get_platform_capabilities(bos_caps).unwrap();
        let vendor_subclass = get_fsct_vendor_subclass_number(platform_caps).unwrap();

        assert_eq!(vendor_subclass, 0x42);
    }
    
    #[test]
    fn test_wrong_bos_descriptor() {
        let data = vec![5, 0x0E, 0, 0, 0]; // Wrong descriptor type

        assert!(matches!(
            decode_bos_descriptor(&data),
            Err(BosError::WrongType {
                name: "BosDescriptor",
                expected: 0x0F,
                actual: 0x0E
            })
        ));
    }

    #[test]
    fn test_wrong_capability_descriptor() {
        let mut data = create_bos_descriptor(8, 1);
        data.extend(vec![3, 0x11, 5]); // Wrong descriptor type

        assert!(matches!(
            decode_bos_descriptor_with_capabilities(&data),
            Err(BosError::WrongType {
                name: "BosCapabilityDescriptor",
                expected: 0x10,
                actual: 0x11
            })
        ));
    }

    #[test]
    fn test_wrong_fsct_capability_version() {
        let mut wrong_platform_data = FSCT_PLATFORM_CAPABILITY_DATA.to_vec();
        wrong_platform_data[18] = 0x02; // Wrong version

        let mut data = create_bos_descriptor(28, 1);
        data.extend(create_capability_descriptor(
            BosCapabilityType::Platform as u8,
            &wrong_platform_data,
        ));

        let bos_caps = decode_bos_descriptor_with_capabilities(&data).unwrap();
        let platform_caps = get_platform_capabilities(bos_caps).unwrap();

        assert!(matches!(
            get_fsct_capability(platform_caps),
            Err(BosError::FsctCapabilityVersionMismatch {
                expected: 0x0100,
                actual: 0x0200
            })
        ));
    }

    #[test]
    fn test_wrong_platform_capability_descriptor() {
        let mut data = create_bos_descriptor(22, 1);
        data.extend(create_capability_descriptor(
            BosCapabilityType::Platform as u8,
            &FSCT_PLATFORM_CAPABILITY_DATA[..17], // Missing version data
        ));

        let bos_caps = decode_bos_descriptor_with_capabilities(&data).unwrap();
        let platform_caps = get_platform_capabilities(bos_caps).unwrap();

        assert!(matches!(
            get_fsct_capability(platform_caps),
            Err(BosError::TooShort {
                name: "FSCT capability data",
                expected: 3,
                actual: 0
            })
        ));
    }
}

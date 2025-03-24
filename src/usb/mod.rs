use std::iter;
use log::warn;
use crate::usb::descriptors::FSCT_FUNCTIONALITY_DESCRIPTOR_ID;
use descriptors::*;
use nusb::transfer::{ControlIn, ControlType, Recipient};
use nusb::{DeviceInfo, Interface};
use nusb::descriptors::Descriptor;
use crate::usb::definitions::FsctTextEncoding;

pub mod definitions;
pub mod descriptors;
pub mod fsct_bos_finder;

async fn get_interface_descriptor(interface: &Interface,
                                  descriptor_number: u8,
                                  length: u16) -> Result<Vec<u8>, String>
{
    let interface_number = interface.interface_number();
    let control_in = ControlIn {
        control_type: ControlType::Standard,
        recipient: Recipient::Interface,
        request: 0x06,
        value: (descriptor_number as u16) << 8,
        index: interface.interface_number() as u16,
        length,
    };
    interface
        .control_in(control_in)
        .await
        .into_result()
        .map_err(|e| format!("Failed to get interface descriptor: {}", e))
}

const FSCT_FUNCTIONALITY_DESCRIPTOR_SIZE: usize = size_of::<FsctFunctionalityDescriptor>();

pub async fn get_fsct_functionality_descriptor_set(device_info: &DeviceInfo,
                                                   interface_number: u8) -> Result<Vec<u8>, String>
{
    let device = device_info.open().map_err(
        |e| format!("Failed to open device: {}", e)
    )?;
    let interface = device.claim_interface(interface_number)
                          .map_err(
                              |e| format!("Failed to claim interface {}: {}", interface_number, e)
                          )?;

    let descriptor = get_interface_descriptor(
        &interface,
        FSCT_FUNCTIONALITY_DESCRIPTOR_ID,
        FSCT_FUNCTIONALITY_DESCRIPTOR_SIZE as u16,
    )
        .await?;

    if descriptor.len() < FSCT_FUNCTIONALITY_DESCRIPTOR_SIZE {
        return Err("FSCT functionality descriptor too short".to_string());
    }
    let fsct_functionality_descriptor: FsctFunctionalityDescriptor = unsafe {
        *std::mem::transmute::<*const u8, &FsctFunctionalityDescriptor>(descriptor.as_ptr())
    };
    if fsct_functionality_descriptor.bLength != FSCT_FUNCTIONALITY_DESCRIPTOR_SIZE as u8 {
        return Err("FSCT functionality descriptor too short".to_string());
    }
    if fsct_functionality_descriptor.wTotalLength < FSCT_FUNCTIONALITY_DESCRIPTOR_SIZE as u16 {
        return Err("FSCT functionality descriptor too short".to_string());
    }
    get_interface_descriptor(
        &interface,
        FSCT_FUNCTIONALITY_DESCRIPTOR_ID,
        fsct_functionality_descriptor.wTotalLength,
    )
        .await
}

pub fn find_fsct_interface_number(device: &DeviceInfo,
                                  fsct_vendor_subclass_number: u8) -> Option<u8>
{
    let interfaces = device.interfaces();
    for interface in interfaces {
        if interface.class() == 0xFF && interface.subclass() == fsct_vendor_subclass_number {
            return Some(interface.interface_number());
        }
    }
    None
}


// Copied from nusb::descriptors::Descriptors, because it is not public
/// An iterator over a sequence of USB descriptors.
#[derive(Clone)]
pub struct Descriptors<'a>(pub &'a [u8]);
impl<'a> Descriptors<'a> {
    /// Get the concatenated bytes of the remaining descriptors.
    pub fn as_bytes(&self) -> &'a [u8] {
        self.0
    }

    fn split_first(&self) -> Option<(&'a [u8], &'a [u8])> {
        if self.0.len() < 2 {
            return None;
        }

        if self.0[0] < 2 {
            warn!(
                "descriptor with bLength {} can't point to next descriptor",
                self.0[0]
            );
            return None;
        }

        if self.0[0] as usize > self.0.len() {
            warn!(
                "descriptor with bLength {} exceeds remaining buffer length {}",
                self.0[0],
                self.0.len()
            );
            return None;
        }

        Some(self.0.split_at(self.0[0] as usize))
    }

    fn split_by_type(mut self, descriptor_type: u8, min_len: u8) -> impl Iterator<Item=&'a [u8]> {
        iter::from_fn(move || {
            loop {
                let (_, next) = self.split_first()?;

                if self.0[1] == descriptor_type {
                    if self.0[0] >= min_len {
                        break;
                    } else {
                        warn!("ignoring descriptor of type {} and length {} because the minimum length is {}", self.0[1], self.0[0], min_len);
                    }
                }

                self.0 = next;
            }

            let mut end = self.0[0] as usize;

            while self.0.len() >= end + 2
                && self.0[end] > 2
                && self.0[end + 1] != descriptor_type
                && self.0.len() >= end + self.0[end] as usize
            {
                end += self.0[end] as usize;
            }

            let (r, next) = self.0.split_at(end);
            self.0 = next;
            Some(r)
        })
    }
}
impl<'a> Iterator for Descriptors<'a> {
    type Item = Descriptor<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some((cur, next)) = self.split_first() {
            self.0 = next;
            Descriptor::new(cur)
        } else {
            None
        }
    }
}
impl TryFrom<Descriptor<'_>> for FsctFunctionalityDescriptor {
    type Error = String;
    fn try_from(value: Descriptor<'_>) -> Result<Self, Self::Error> {
        if value.descriptor_type() != FSCT_FUNCTIONALITY_DESCRIPTOR_ID {
            return Err("Not an FSCT functionality descriptor".to_string());
        }
        if value.len() != FSCT_FUNCTIONALITY_DESCRIPTOR_SIZE {
            return Err("FSCT functionality descriptor too short".to_string());
        }
        let fsct_functionality_descriptor: FsctFunctionalityDescriptor = unsafe {
            *std::mem::transmute::<*const u8, &FsctFunctionalityDescriptor>(value.as_ptr())
        };
        Ok(fsct_functionality_descriptor)
    }
}

impl TryFrom<Descriptor<'_>> for FsctImageMetadataDescriptor {
    type Error = String;
    fn try_from(value: Descriptor<'_>) -> Result<Self, Self::Error> {
        if value.descriptor_type() != FSCT_IMAGE_METADATA_DESCRIPTOR_ID {
            return Err("Not an FSCT image metadata descriptor".to_string());
        }
        if value.len() != size_of::<FsctImageMetadataDescriptor>() {
            return Err("FSCT image metadata descriptor too short".to_string());
        }
        let fsct_image_metadata_descriptor: FsctImageMetadataDescriptor = unsafe {
            *std::mem::transmute::<*const u8, &FsctImageMetadataDescriptor>(value.as_ptr())
        };
        Ok(fsct_image_metadata_descriptor)
    }
}


const FSCT_TEXT_METADATA_DESCRIPTOR_HEADER_SIZE: usize = size_of::<FsctTextMetadataDescriptorHeader>();

impl TryFrom<Descriptor<'_>> for FsctTextMetadataDescriptor {
    type Error = String;
    fn try_from(value: Descriptor<'_>) -> Result<Self, Self::Error> {
        if value.descriptor_type() != FSCT_TEXT_METADATA_DESCRIPTOR_ID {
            return Err("Not an FSCT text metadata descriptor".to_string());
        }
        if value.len() < FSCT_TEXT_METADATA_DESCRIPTOR_HEADER_SIZE {
            return Err("FSCT text metadata descriptor too short".to_string());
        }
        let fsct_text_metadata_descriptor_header: &FsctTextMetadataDescriptorHeader = unsafe {
            &std::mem::transmute::<*const u8, &FsctTextMetadataDescriptorHeader>(value.as_ptr())
        };

        let mut fsct_text_metadata_descriptor = FsctTextMetadataDescriptor {
            bLength: fsct_text_metadata_descriptor_header.bLength,
            bDescriptorType: fsct_text_metadata_descriptor_header.bDescriptorType,
            bSystemTextCoding: fsct_text_metadata_descriptor_header.bSystemTextCoding,
            aMetadata: Vec::new(),
        };

        //here metadata is a vector of FsctTextMetadataDescriptorMultiPart
        let mut remaining_data = &value.iter().as_slice()[FSCT_TEXT_METADATA_DESCRIPTOR_HEADER_SIZE..];
        while !remaining_data.is_empty() {
            if remaining_data.len() < size_of::<FsctTextMetadataDescriptorMultiPart>() {
                return Err("FSCT text metadata descriptor too short".to_string());
            }
            let fsct_text_metadata_descriptor_multi_part: &FsctTextMetadataDescriptorMultiPart = unsafe {
                &std::mem::transmute::<*const u8, &FsctTextMetadataDescriptorMultiPart>(remaining_data.as_ptr())
            };
            fsct_text_metadata_descriptor.aMetadata.push(*fsct_text_metadata_descriptor_multi_part);
            remaining_data = &remaining_data[size_of::<FsctTextMetadataDescriptorMultiPart>()..];
        }

        Ok(fsct_text_metadata_descriptor)
    }
}
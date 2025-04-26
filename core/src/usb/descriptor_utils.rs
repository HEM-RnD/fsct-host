use std::mem::size_of;
use nusb::descriptors::Descriptor;
use nusb::{DeviceInfo, Interface};
use log::warn;
use nusb::transfer::{ControlIn, ControlType, Recipient};
use crate::usb::descriptors::{FsctFunctionalityDescriptor, FsctImageMetadataDescriptor, FsctTextMetadataDescriptor, FsctTextMetadataDescriptorHeader, FsctTextMetadataDescriptorMultiPart, FSCT_FUNCTIONALITY_DESCRIPTOR_ID, FSCT_IMAGE_METADATA_DESCRIPTOR_ID, FSCT_TEXT_METADATA_DESCRIPTOR_ID};
use crate::usb::errors::{DescriptorError, IoErrorOrAny};

async fn get_interface_descriptor(interface: &Interface,
                                  descriptor_number: u8,
                                  length: u16) -> Result<Vec<u8>, IoErrorOrAny>
{
    let interface_number = interface.interface_number();
    let control_in = ControlIn {
        control_type: ControlType::Standard,
        recipient: Recipient::Interface,
        request: 0x06,
        value: (descriptor_number as u16) << 8,
        index: interface_number as u16,
        length,
    };
    interface
        .control_in(control_in)
        .await
        .into_result()
        .map_err(|e| IoErrorOrAny::IoError(e.into()))
}

const FSCT_FUNCTIONALITY_DESCRIPTOR_SIZE: usize = size_of::<FsctFunctionalityDescriptor>();

async fn get_fsct_functionality_descriptor_set_raw(interface: &Interface) -> Result<Vec<u8>, IoErrorOrAny>
{
    let descriptor = get_interface_descriptor(
        interface,
        FSCT_FUNCTIONALITY_DESCRIPTOR_ID,
        FSCT_FUNCTIONALITY_DESCRIPTOR_SIZE as u16,
    )
        .await?;

    if descriptor.len() < FSCT_FUNCTIONALITY_DESCRIPTOR_SIZE {
        return Err(DescriptorError::TooShort.into());
    }
    let fsct_functionality_descriptor: FsctFunctionalityDescriptor = unsafe {
        *std::mem::transmute::<*const u8, &FsctFunctionalityDescriptor>(descriptor.as_ptr())
    };
    if fsct_functionality_descriptor.bLength != FSCT_FUNCTIONALITY_DESCRIPTOR_SIZE as u8 {
        return Err(DescriptorError::TooShort.into());
    }
    if fsct_functionality_descriptor.wTotalLength < FSCT_FUNCTIONALITY_DESCRIPTOR_SIZE as u16 {
        return Err(DescriptorError::TooShort.into());
    }
    get_interface_descriptor(
        interface,
        FSCT_FUNCTIONALITY_DESCRIPTOR_ID,
        fsct_functionality_descriptor.wTotalLength,
    )
        .await
}

#[derive(Debug)]
pub enum FsctDescriptorSet {
    Functionality(FsctFunctionalityDescriptor),
    ImageMetadata(FsctImageMetadataDescriptor),
    TextMetadata(FsctTextMetadataDescriptor),
}

pub async fn get_fsct_functionality_descriptor_set(interface: &Interface) -> Result<Vec<FsctDescriptorSet>, IoErrorOrAny>
{
    let raw_descriptor = get_fsct_functionality_descriptor_set_raw(interface).await?;
    let descriptors = Descriptors(&raw_descriptor);
    let mut fsct_descriptors = Vec::new();
    for descriptor in descriptors {
        match descriptor.descriptor_type() {
            FSCT_FUNCTIONALITY_DESCRIPTOR_ID => {
                let fsct_descriptor: FsctFunctionalityDescriptor = descriptor.try_into()
                                                                             .map_err(|_| DescriptorError::NotFsctFunctionalityDescriptor)?;
                fsct_descriptors.push(FsctDescriptorSet::Functionality(fsct_descriptor));
            }
            FSCT_IMAGE_METADATA_DESCRIPTOR_ID => {
                let fsct_descriptor: FsctImageMetadataDescriptor = descriptor.try_into()
                                                                             .map_err(|_| DescriptorError::NotFsctImageMetadataDescriptor)?;
                fsct_descriptors.push(FsctDescriptorSet::ImageMetadata(fsct_descriptor));
            }
            FSCT_TEXT_METADATA_DESCRIPTOR_ID => {
                let fsct_descriptor: FsctTextMetadataDescriptor = descriptor.try_into()
                                                                            .map_err(|_| DescriptorError::NotFsctTextMetadataDescriptor)?;
                fsct_descriptors.push(FsctDescriptorSet::TextMetadata(fsct_descriptor));
            }
            _ => {}
        }
    }
    Ok(fsct_descriptors)
}

pub fn find_fsct_interface_number(device: &DeviceInfo,
                                  fsct_vendor_subclass_number: u8) -> Result<u8, DescriptorError>
{
    let interfaces = device.interfaces();
    for interface in interfaces {
        if interface.class() == 0xFF && interface.subclass() == fsct_vendor_subclass_number {
            return Ok(interface.interface_number());
        }
    }
    Err(DescriptorError::InterfaceNotFound)
}

// Copied from nusb::descriptors::Descriptors, because it is not public
/// An iterator over a sequence of USB descriptors.
#[derive(Clone)]
struct Descriptors<'a>(&'a [u8]);

impl<'a> Descriptors<'a> {
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
    type Error = DescriptorError;
    fn try_from(value: Descriptor<'_>) -> Result<Self, Self::Error> {
        if value.descriptor_type() != FSCT_FUNCTIONALITY_DESCRIPTOR_ID {
            return Err(DescriptorError::NotFsctFunctionalityDescriptor);
        }
        if value.len() != FSCT_FUNCTIONALITY_DESCRIPTOR_SIZE {
            return Err(DescriptorError::TooShort);
        }
        let fsct_functionality_descriptor: FsctFunctionalityDescriptor = unsafe {
            *std::mem::transmute::<*const u8, &FsctFunctionalityDescriptor>(value.as_ptr())
        };
        Ok(fsct_functionality_descriptor)
    }
}

impl TryFrom<Descriptor<'_>> for FsctImageMetadataDescriptor {
    type Error = DescriptorError;
    fn try_from(value: Descriptor<'_>) -> Result<Self, Self::Error> {
        if value.descriptor_type() != FSCT_IMAGE_METADATA_DESCRIPTOR_ID {
            return Err(DescriptorError::NotFsctImageMetadataDescriptor);
        }
        if value.len() != size_of::<FsctImageMetadataDescriptor>() {
            return Err(DescriptorError::TooShort);
        }
        let fsct_image_metadata_descriptor: FsctImageMetadataDescriptor = unsafe {
            *std::mem::transmute::<*const u8, &FsctImageMetadataDescriptor>(value.as_ptr())
        };
        Ok(fsct_image_metadata_descriptor)
    }
}

const FSCT_TEXT_METADATA_DESCRIPTOR_HEADER_SIZE: usize = size_of::<FsctTextMetadataDescriptorHeader>();

impl TryFrom<Descriptor<'_>> for FsctTextMetadataDescriptor {
    type Error = DescriptorError;
    fn try_from(value: Descriptor<'_>) -> Result<Self, Self::Error> {
        if value.descriptor_type() != FSCT_TEXT_METADATA_DESCRIPTOR_ID {
            return Err(DescriptorError::NotFsctTextMetadataDescriptor);
        }
        if value.len() < FSCT_TEXT_METADATA_DESCRIPTOR_HEADER_SIZE {
            return Err(DescriptorError::TooShort);
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
                return Err(DescriptorError::TooShort);
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
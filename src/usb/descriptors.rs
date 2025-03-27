use crate::usb::definitions::{FsctFunctionality, FsctImagePixelFormat, FsctTextEncoding, FsctTextMetadata};

pub const FSCT_FUNCTIONALITY_DESCRIPTOR_ID: u8 = 0x31;
pub const FSCT_TEXT_METADATA_DESCRIPTOR_ID: u8 = 0x32;
pub const FSCT_IMAGE_METADATA_DESCRIPTOR_ID: u8 = 0x33;

#[repr(C, packed)]
#[derive(Debug, Default, Clone, Copy)]
#[allow(non_snake_case)]
pub struct FsctFunctionalityDescriptor {
    pub bLength: u8,
    pub bDescriptorType: u8,
    pub wTotalLength: u16,
    pub bmFunctionality: FsctFunctionality, // Updated type
}

#[repr(C, packed)]
#[derive(Debug, Default, Clone, Copy)]
#[allow(non_snake_case)]
pub struct FsctTextMetadataDescriptorMultiPart {
    pub bMetadata: FsctTextMetadata, // Updated type
    pub wMaxLength: u16,
}

#[repr(C, packed)]
#[derive(Debug, Clone)]
#[allow(non_snake_case)]
pub struct FsctTextMetadataDescriptorHeader {
    pub bLength: u8,
    pub bDescriptorType: u8,
    pub bSystemTextCoding: FsctTextEncoding, // Updated type
}

#[derive(Debug, Clone)]
#[allow(non_snake_case)]
pub struct FsctTextMetadataDescriptor {
    pub bLength: u8,
    pub bDescriptorType: u8,
    pub bSystemTextCoding: FsctTextEncoding,
    pub aMetadata: Vec<FsctTextMetadataDescriptorMultiPart>,
}


#[repr(C, packed)]
#[derive(Debug, Default, Clone, Copy)]
#[allow(non_snake_case)]
pub struct FsctImageMetadataDescriptor {
    pub bLength: u8,
    pub bDescriptorType: u8,
    pub wImageWidth: u16,
    pub wImageHeight: u16,
    pub bPixelFormat: FsctImagePixelFormat, // Updated type
}


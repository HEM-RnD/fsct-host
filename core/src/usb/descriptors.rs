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

use crate::definitions::{FsctFunctionality, FsctImagePixelFormat, FsctTextEncoding, FsctTextMetadata};

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


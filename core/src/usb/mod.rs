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
use crate::usb::errors::{DeviceDiscoveryError};

pub mod descriptors;
pub mod fsct_bos_finder;
pub mod descriptor_utils;
mod fsct_usb_interface;
pub mod fsct_device;
pub mod requests;

pub mod errors;

const FSCT_SUPPORTED_PROTOCOL_VERSION: u8 = 0x01;

fn check_fsct_interface_protocol(device_info: &DeviceInfo, fsct_interface_number: u8) -> Result<(), DeviceDiscoveryError> {
    let protocol = device_info
        .interfaces()
        .find(|i| i.interface_number() == fsct_interface_number)
        .map(|v| v.protocol())
        .ok_or(DeviceDiscoveryError::InterfaceNotFound)?;


    if protocol == FSCT_SUPPORTED_PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(DeviceDiscoveryError::ProtocolVersionNotSupported(protocol))
    }
}


pub async fn open_interface(device_info: &DeviceInfo, interface_number: u8) -> Result<nusb::Interface, DeviceDiscoveryError>
{
    let device = device_info.open()?;
    let interface = device.claim_interface(interface_number)?;
    Ok(interface)
}

pub async fn create_and_configure_fsct_device(device_info: &DeviceInfo) -> Result<fsct_device::FsctDevice, DeviceDiscoveryError> {
    let fsct_vendor_subclass_number = fsct_bos_finder::get_fsct_vendor_subclass_number_from_device(device_info)?;

    let fsct_interface_number = find_fsct_interface_number(device_info, fsct_vendor_subclass_number)?;
    check_fsct_interface_protocol(device_info, fsct_interface_number)?;
    let interface = open_interface(&device_info, fsct_interface_number).await?;
    let fsct_descriptors = descriptor_utils::get_fsct_functionality_descriptor_set(&interface).await?;
    let fsct_interface = fsct_usb_interface::FsctUsbInterface::new(interface);
    let mut fsct_device = fsct_device::FsctDevice::new(fsct_interface);
    fsct_device.init(&fsct_descriptors).await?;
    Ok(fsct_device)
}

pub fn find_fsct_interface_number(device: &DeviceInfo,
                                  fsct_vendor_subclass_number: u8) -> Result<u8, DeviceDiscoveryError>
{
    let interfaces = device.interfaces();
    for interface in interfaces {
        if interface.class() == 0xFF && interface.subclass() == fsct_vendor_subclass_number {
            return Ok(interface.interface_number());
        }
    }
    Err(DeviceDiscoveryError::InterfaceNotFound)
}
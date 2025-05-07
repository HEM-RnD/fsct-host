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

use fsct_core::usb::fsct_bos_finder::get_fsct_vendor_subclass_number_from_device;
use nusb::DeviceInfo;
use fsct_core::usb::descriptor_utils::get_fsct_functionality_descriptor_set;
use fsct_core::usb::{find_fsct_interface_number, open_interface};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let devices = nusb::list_devices()
        .map_err(|e| format!("Failed to list devices: {}", e))
        .unwrap();
    for device in devices {
        if let Ok(fsct_vendor_subclass_number) = get_fsct_vendor_subclass_number_from_device(&device) {
            let err = print_fsct_dump(&device, fsct_vendor_subclass_number).await;
            if err.is_err() {
                eprintln!("Error: {}", err.unwrap_err());
            }
        }
    }
    Ok(())
}

async fn print_fsct_dump(device_info: &DeviceInfo, fsct_vendor_subclass_number: u8) -> Result<(), anyhow::Error> {
    let fsct_interface_number = find_fsct_interface_number(&device_info, fsct_vendor_subclass_number);
    if let Err(e) = fsct_interface_number {
        println!("Device reports FSCT in BOS descriptor, but no Ferrum Streaming Control Technology interface found. \
        Error: {e}");
        return Ok(()); // ignore devices that report FSCT in BOS descriptor but don't have FSCT interface
    }
    let fsct_interface_number = fsct_interface_number.unwrap();
    let interface = open_interface(device_info, fsct_interface_number).await?;
    let descriptor = get_fsct_functionality_descriptor_set(&interface).await?;
    println!(
        "Device with Ferrum Streaming Control Technology interface found: \"{}\" ({:04X}:{:04X})",
        device_info.product_string().unwrap_or("Unknown"),
        device_info.vendor_id(),
        device_info.product_id()
    );
    println!("FSCT interface number: {}", fsct_interface_number);

    println!("FSCT functionality descriptor set: {:#?}", descriptor);

    Ok(())
}

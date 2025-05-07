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

use std::time::Duration;
// use tokio::main;
use fsct_core::usb::create_and_configure_fsct_device;
use nusb::list_devices;
use fsct_core::definitions::TimelineInfo;
use fsct_core::definitions::FsctTextMetadata;
use fsct_core::definitions::FsctStatus;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let devices = list_devices()?;
    for device in devices {
        let fsct_device = create_and_configure_fsct_device(&device).await;
        if let Err(error_string) = fsct_device {
            println!("Device {:04x}:{:04x} omitted: {}", device.vendor_id(), device.product_id(), error_string);
            continue;
        }
        let fsct_device = fsct_device.unwrap();
        println!("Device with Ferrum Streaming Control Technology capability found: \"{}\" ({:04X}:{:04X})",
                 device.product_string().unwrap_or("Unknown"),
                 device.vendor_id(),
                 device.product_id());
        let time_diff = fsct_device.time_diff();
        println!("Time difference: {:?}", time_diff);
        let enable = fsct_device.get_enable().await?;
        println!("Enable: {}", enable);
        if !enable {
            println!("Enabling FSCT...");
            fsct_device.set_enable(true).await?;
            let enable = fsct_device.get_enable().await?;
            println!("Enable: {}", enable);
        } else {
            println!("FSCT is already enabled.");
        }

        fsct_device.set_progress(Some(TimelineInfo {
            update_time: std::time::SystemTime::now() - Duration::from_secs(60),
            position: Duration::from_secs(60),
            duration: Duration::from_secs(186),
            rate: 1.0,
        })).await?;
        println!(
            "Progress set to 60 seconds from the start of the track, 60 seconds ago, which means 120 seconds now.");
        let current_title = "Пісня Сміливих Дівчат";
        let current_artist = "KAZKA";
        fsct_device.set_current_text(FsctTextMetadata::CurrentTitle, Some(current_title)).await?;
        println!("Set current title: \"{}\"", current_title);
        fsct_device.set_current_text(FsctTextMetadata::CurrentAuthor, Some(current_artist)).await?;
        println!("Set current artist: \"{}\"", current_artist);
        fsct_device.set_status(FsctStatus::Playing).await?;

        let sleep = Duration::from_secs(10); // 10 seconds to ensure we are sending poll requests
        tokio::time::sleep(sleep).await;

        fsct_device.set_progress(Some(TimelineInfo {
            update_time: std::time::SystemTime::now(),
            position: Duration::from_secs(120) + sleep,
            duration: Duration::from_secs(186),
            rate: 0.0,
        })).await?;
        fsct_device.set_status(FsctStatus::Paused).await?;
        println!("Progress paused at 130 seconds.");
        tokio::time::sleep(Duration::from_secs(3)).await;
        fsct_device.set_progress(None).await?;
        fsct_device.set_current_text(FsctTextMetadata::CurrentTitle, None).await?;
        fsct_device.set_current_text(FsctTextMetadata::CurrentAuthor, None).await?;
        fsct_device.set_status(FsctStatus::Stopped).await?;
        println!("Metadata cleared.");
        drop(fsct_device);
        println!("Waiting for 10 seconds to check if polling stops working");
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
    Ok(())
}
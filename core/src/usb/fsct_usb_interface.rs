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

use std::mem::size_of;
use anyhow::{Context};
use nusb::Interface;
use nusb::transfer::{ControlIn, ControlOut, ControlType, Recipient};
use crate::definitions::FsctTextMetadata;
use crate::usb::requests;
use crate::definitions::FsctStatus;
use crate::usb::errors::{FsctDeviceError, ToFsctDeviceResult};

pub struct FsctUsbInterface {
    interface: Interface,
}

impl FsctUsbInterface {
    pub fn new(interface: Interface) -> Self {
        Self {
            interface,
        }
    }
    pub async fn get_device_timestamp(&self) -> Result<requests::Timestamp, FsctDeviceError> {
        let control_in = ControlIn {
            control_type: ControlType::Vendor,
            recipient: Recipient::Interface,
            request: requests::FsctRequestCode::Timestamp as u8,
            value: 0x00,
            index: self.interface.interface_number() as u16,
            length: size_of::<requests::Timestamp>() as u16,
        };
        let timestamp_raw = self.interface.control_in(control_in)
                                .await
                                .into_result()
                                .context("Failed to get device timestamp")
                                .map_err_to_fsct_device_control_transfer_error()?;

        if timestamp_raw.len() != size_of::<requests::Timestamp>() {
            return Err(FsctDeviceError::DataSizeMismatch {
                expected: size_of::<requests::Timestamp>(),
                actual: timestamp_raw.len(),
            });
        }
        let timestamp = unsafe { *(timestamp_raw.as_ptr() as *const requests::Timestamp) };
        Ok(timestamp)
    }

    pub async fn get_enable(&self) -> Result<bool, FsctDeviceError> {
        let control_in = ControlIn {
            control_type: ControlType::Vendor,
            recipient: Recipient::Interface,
            request: requests::FsctRequestCode::Enable as u8,
            value: 0x00,
            index: self.interface.interface_number() as u16,
            length: 1,
        };

        let enable_raw = self.interface.control_in(control_in)
                             .await
                             .into_result()
                             .context("Failed to get enable.")
                             .map_err_to_fsct_device_control_transfer_error()?;
        if enable_raw.len() != 1 {
            return Err(FsctDeviceError::DataSizeMismatch {
                expected: 1,
                actual: enable_raw.len(),
            });
        }
        Ok(enable_raw[0] != 0)
    }

    pub async fn set_enable(&self, enable: bool) -> Result<(), FsctDeviceError> {
        let control_out = ControlOut {
            control_type: ControlType::Vendor,
            recipient: Recipient::Interface,
            request: requests::FsctRequestCode::Enable as u8,
            value: if enable { 0x01 } else { 0x00 },
            index: self.interface.interface_number() as u16,
            data: &[],
        };
        self.interface.control_out(control_out)
            .await
            .into_result()
            .context("Failed to set enable")
            .map_err_to_fsct_device_control_transfer_error()?;
        Ok(())
    }

    pub async fn send_track_progress(&self, progress: &requests::TrackProgressRequestData) -> Result<(), FsctDeviceError> {
        let control_out = ControlOut {
            control_type: ControlType::Vendor,
            recipient: Recipient::Interface,
            request: requests::FsctRequestCode::Progress as u8,
            value: 0x00,
            index: self.interface.interface_number() as u16,
            data: unsafe {
                std::slice::from_raw_parts(
                    progress as *const requests::TrackProgressRequestData as *const u8,
                    size_of::<requests::TrackProgressRequestData>(),
                )
            },
        };
        self.interface.control_out(control_out).await.into_result()
            .context("Failed to send track progress")
            .map_err_to_fsct_device_control_transfer_error()?;

        Ok(())
    }

    pub async fn disable_track_progress(&self) -> Result<(), FsctDeviceError> {
        let control_out = ControlOut {
            control_type: ControlType::Vendor,
            recipient: Recipient::Interface,
            request: requests::FsctRequestCode::Progress as u8,
            value: 0x00,
            index: self.interface.interface_number() as u16,
            data: &[],
        };
        self.interface.control_out(control_out).await.into_result()
            .context("Failed to disable track progress")
            .map_err_to_fsct_device_control_transfer_error()?;
        Ok(())
    }

    pub async fn send_current_text(&self, text_id: FsctTextMetadata, text_raw: &[u8]) -> Result<(), FsctDeviceError>
    {
        let control_out = ControlOut {
            control_type: ControlType::Vendor,
            recipient: Recipient::Interface,
            request: requests::FsctRequestCode::CurrentText as u8,
            value: 0x00,
            index: self.interface.interface_number() as u16 | ((text_id as u16) << 8),
            data: text_raw,
        };
        self.interface.control_out(control_out).await.into_result()
            .context("Failed to send current text")
            .map_err_to_fsct_device_control_transfer_error()?;
        Ok(())
    }

    pub async fn disable_current_text(&self, text_id: FsctTextMetadata) -> Result<(), FsctDeviceError>
    {
        let control_out = ControlOut {
            control_type: ControlType::Vendor,
            recipient: Recipient::Interface,
            request: requests::FsctRequestCode::CurrentText as u8,
            value: 0x00,
            index: self.interface.interface_number() as u16 | ((text_id as u16) << 8),
            data: &[],
        };
        self.interface.control_out(control_out).await.into_result()
            .context("Failed to send current text")
            .map_err_to_fsct_device_control_transfer_error()?;
        Ok(())
    }

    pub async fn send_status(&self, status: FsctStatus) -> Result<(), FsctDeviceError> {
        let control_out = ControlOut {
            control_type: ControlType::Vendor,
            recipient: Recipient::Interface,
            request: requests::FsctRequestCode::Status as u8,
            value: status as u16,
            index: self.interface.interface_number() as u16,
            data: &[],
        };
        self.interface.control_out(control_out).await.into_result()
            .context("Failed to send status")
            .map_err_to_fsct_device_control_transfer_error()?;
        Ok(())
    }
}
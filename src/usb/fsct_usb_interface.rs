use nusb::Interface;
use nusb::transfer::{ControlIn, ControlOut, ControlType, Recipient};
use crate::usb::definitions::FsctTextMetadata;
use crate::usb::requests;
use crate::usb::requests::FsctStatus;

pub struct FsctUsbInterface {
    interface: Interface,
}

impl FsctUsbInterface {
    pub fn new(interface: Interface) -> Self {
        Self {
            interface,
        }
    }
    pub async fn get_device_timestamp(&self) -> Result<requests::Timestamp, String> {
        let control_in = ControlIn {
            control_type: ControlType::Vendor,
            recipient: Recipient::Interface,
            request: requests::FsctRequestCode::Timestamp as u8,
            value: 0x00,
            index: self.interface.interface_number() as u16,
            length: size_of::<requests::Timestamp>() as u16,
        };
        let timestamp_raw = self.interface.control_in(control_in).await.into_result().map_err(
            |e| format!("Failed to get device timestamp: {}", e)
        )?;
        if timestamp_raw.len() != size_of::<requests::Timestamp>() {
            return Err(format!("Expected {} bytes, got {}", size_of::<requests::Timestamp>(), timestamp_raw.len()));
        }
        let timestamp = unsafe { *(timestamp_raw.as_ptr() as *const requests::Timestamp) };
        Ok(timestamp)
    }

    pub async fn get_enable(&self) -> Result<bool, String> {
        let control_in = ControlIn {
            control_type: ControlType::Vendor,
            recipient: Recipient::Interface,
            request: requests::FsctRequestCode::Enable as u8,
            value: 0x00,
            index: self.interface.interface_number() as u16,
            length: 1,
        };

        let enable_raw = self.interface.control_in(control_in).await.into_result().map_err(
            |e| format!("Failed to get enable: {}", e)
        )?;
        if enable_raw.len() != 1 {
            return Err(format!("Expected 1 byte, got {}", enable_raw.len()));
        }
        Ok(enable_raw[0] != 0)
    }

    pub async fn set_enable(&self, enable: bool) -> Result<(), String> {
        let control_out = ControlOut {
            control_type: ControlType::Vendor,
            recipient: Recipient::Interface,
            request: requests::FsctRequestCode::Enable as u8,
            value: if enable { 0x01 } else { 0x00 },
            index: self.interface.interface_number() as u16,
            data: &[],
        };
        self.interface.control_out(control_out).await.into_result().map_err(
            |e| format!("Failed to set enable: {}", e)
        )?;
        Ok(())
    }

    pub async fn send_track_progress(&self, progress: &requests::TrackProgressRequestData) -> Result<(), String> {
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
        self.interface.control_out(control_out).await.into_result().map_err(
            |e| format!("Failed to send track progress: {}", e)
        )?;
        Ok(())
    }

    pub async fn disable_track_progress(&self) -> Result<(), String> {
        let control_out = ControlOut {
            control_type: ControlType::Vendor,
            recipient: Recipient::Interface,
            request: requests::FsctRequestCode::Progress as u8,
            value: 0x00,
            index: self.interface.interface_number() as u16,
            data: &[],
        };
        self.interface.control_out(control_out).await.into_result().map_err(
            |e| format!("Failed to disable track progress: {}", e)
        )?;
        Ok(())
    }

    pub async fn send_current_text(&self, text_id: FsctTextMetadata, text_raw: &[u8]) -> Result<(), String>
    {
        let control_out = ControlOut {
            control_type: ControlType::Vendor,
            recipient: Recipient::Interface,
            request: requests::FsctRequestCode::CurrentText as u8,
            value: 0x00,
            index: self.interface.interface_number() as u16 | ((text_id as u16) << 8),
            data: text_raw,
        };
        self.interface.control_out(control_out).await.into_result().map_err(
            |e| format!("Failed to send current text: {}", e)
        )?;
        Ok(())
    }

    pub async fn disable_current_text(&self, text_id: FsctTextMetadata) -> Result<(), String>
    {
        let control_out = ControlOut {
            control_type: ControlType::Vendor,
            recipient: Recipient::Interface,
            request: requests::FsctRequestCode::CurrentText as u8,
            value: 0x00,
            index: self.interface.interface_number() as u16 | ((text_id as u16) << 8),
            data: &[],
        };
        self.interface.control_out(control_out).await.into_result().map_err(
            |e| format!("Failed to send current text: {}", e)
        )?;
        Ok(())
    }

    pub async fn send_status(&self, status: FsctStatus) -> Result<(), String> {
        let control_out = ControlOut {
            control_type: ControlType::Vendor,
            recipient: Recipient::Interface,
            request: requests::FsctRequestCode::Status as u8,
            value: status as u16,
            index: self.interface.interface_number() as u16,
            data: &[],
        };
        self.interface.control_out(control_out).await.into_result().map_err(
            |e| format!("Failed to send status: {}", e)
        )?;
        Ok(())
    }

    pub async fn poll(&self) -> Result<(), String> {
        let control_out = ControlOut {
            control_type: ControlType::Vendor,
            recipient: Recipient::Interface,
            request: requests::FsctRequestCode::Poll as u8,
            value: 0,
            index: self.interface.interface_number() as u16,
            data: &[],
        };
        self.interface.control_out(control_out).await.into_result().map_err(
            |e| format!("Failed to poll: {}", e)
        )?;
        Ok(())
    }
}
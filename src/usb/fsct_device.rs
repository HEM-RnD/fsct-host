use crate::platform::TimelineInfo;
use crate::usb::definitions::{FsctTextEncoding, FsctTextMetadata};
use crate::usb::fsct_usb_interface;
use crate::usb::requests::TrackProgressRequestData;

pub struct FsctDevice {
    fsct_interface: fsct_usb_interface::FsctUsbInterface,
    time_diff: std::time::Duration,
    fsct_text_encoding: FsctTextEncoding,
}

impl FsctDevice {
    pub fn new(fsct_interface: fsct_usb_interface::FsctUsbInterface) -> Self {
        Self {
            fsct_interface,
            time_diff: std::time::Duration::from_millis(0),
            fsct_text_encoding: FsctTextEncoding::Unicode16,
        }
    }
    pub fn fsct_interface(&self) -> &fsct_usb_interface::FsctUsbInterface {
        &self.fsct_interface
    }

    pub fn time_diff(&self) -> std::time::Duration {
        self.time_diff
    }
    pub async fn synchronize_time(&mut self) -> Result<(), String> {
        let before = std::time::SystemTime::now();
        let timestamp_in_millis = self.fsct_interface.get_device_timestamp().await?;
        let after = std::time::SystemTime::now();
        let mean_now = ((before.duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() + after.duration_since
        (std::time::UNIX_EPOCH).unwrap().as_millis()) / 2) as i128;
        let time_diff = mean_now - (timestamp_in_millis as i128);
        if time_diff > u64::MAX as i128 {
            return Err("Time difference is too large".to_string());
        }
        if time_diff < 0 {
            return Err("Time difference is negative".to_string());
        }
        self.time_diff = std::time::Duration::from_millis(time_diff as u64);
        Ok(())
    }

    pub async fn get_enable(&self) -> Result<bool, String> {
        self.fsct_interface.get_enable().await
    }
    pub async fn set_enable(&self, enable: bool) -> Result<(), String> {
        self.fsct_interface.set_enable(enable).await
    }

    pub async fn set_progress(&self, progress: Option<TimelineInfo>) -> Result<(), String>
    {
        match progress {
            None => self.fsct_interface.disable_track_progress().await,
            Some(progress) => {
                let timestamp = std::time::SystemTime::now();
                let duration_since_update_time = timestamp.duration_since(progress.update_time).map_err(
                    |e| format!("Failed to get time difference.\
                    It seems that timestamp is later than now. Error: {}", e)
                )?;

                let position = progress.position + (duration_since_update_time.as_secs_f64() * progress.rate as f64);
                let device_timestamp = (timestamp - self.time_diff).duration_since(std::time::UNIX_EPOCH)
                                                                   .unwrap().as_millis() as u64;
                let track_progress_request_data = TrackProgressRequestData {
                    duration: progress.duration as u32,
                    position: position as i32,
                    timestamp: device_timestamp,
                    rate: progress.rate,
                };
                self.fsct_interface.send_track_progress(&track_progress_request_data).await
            }
        }
    }

    fn to_usb_encoded_text(&self, text: &str) -> Vec<u8> {
        match self.fsct_text_encoding {
            FsctTextEncoding::Unicode16 => {
                text.chars().map(|c| {
                    if (c as u32) < (u16::MAX as u32) {
                        c as u16
                    } else {
                        char::REPLACEMENT_CHARACTER as u16
                    }
                }).map(u16::to_ne_bytes).flatten().collect()
            }
            FsctTextEncoding::Utf8 => {
                text.as_bytes().to_vec()
            }
            FsctTextEncoding::Utf16 => {
                text.encode_utf16().map(u16::to_ne_bytes).flatten().collect()
            }
            FsctTextEncoding::Unicode32 => {
                text.chars().map(|c| c as u32).map(u32::to_ne_bytes).flatten().collect()
            }
        }
    }

    pub async fn set_current_text(&self, text_id: FsctTextMetadata, text: Option<&str>) -> Result<(), String>
    {
        match text {
            None => self.fsct_interface.disable_current_text(text_id).await,
            Some(text) => {
                let data_text = self.to_usb_encoded_text(text);
                self.fsct_interface.send_current_text(text_id, data_text.as_slice()).await
            }
        }
    }

    pub async fn set_status(&self, status: crate::usb::requests::FsctStatus) -> Result<(), String>
    {
        self.fsct_interface.send_status(status).await
    }
}


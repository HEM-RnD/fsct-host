use std::sync::Arc;
use crate::platform::TimelineInfo;
use crate::usb::definitions::{FsctFunctionality, FsctTextEncoding, FsctTextMetadata};
use crate::usb::descriptor_utils::FsctDescriptorSet;
use crate::usb::fsct_usb_interface;
use crate::usb::requests::TrackProgressRequestData;


#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
struct SupportedMetadata {
    pub metadata: FsctTextMetadata,
    pub max_length: usize,
}

pub struct FsctDevice {
    fsct_interface: Arc<fsct_usb_interface::FsctUsbInterface>,
    time_diff: Option<std::time::Duration>,
    fsct_text_encoding: FsctTextEncoding,
    supported_current_texts: Vec<SupportedMetadata>,
    supported_functionalities: FsctFunctionality,
    poll_task_handle: Option<tokio::task::JoinHandle<()>>,
}

impl FsctDevice {
    pub(super) fn new(fsct_interface: fsct_usb_interface::FsctUsbInterface) -> Self {
        let fsct_device = Self {
            fsct_interface: Arc::new(fsct_interface),
            time_diff: None,
            fsct_text_encoding: FsctTextEncoding::Utf8,
            supported_current_texts: Vec::new(),
            supported_functionalities: FsctFunctionality::empty(),
            poll_task_handle: None,
        };
        fsct_device
    }

    pub(super) async fn init(&mut self, fsct_descriptors: &[FsctDescriptorSet]) -> Result<(), String> {
        self.parse_descriptors(fsct_descriptors);
        if self.supported_functionalities.contains(FsctFunctionality::CurrentPlaybackProgress) {
            self.synchronize_time().await?;
        }
        self.fsct_interface.set_enable(true).await?;
        let fsct_interface = self.fsct_interface.clone();
        self.poll_task_handle = Some(tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                let res = fsct_interface.poll().await;
                if let Err(e) = res {
                    log::error!("Error in FSCT interface: {}", e);
                }
            }
        }));

        Ok(())
    }
    fn parse_descriptors(&mut self, fsct_descriptor_set: &[FsctDescriptorSet]) {
        for descriptor in fsct_descriptor_set {
            match descriptor {
                FsctDescriptorSet::Functionality(functionality_descriptor) => {
                    self.supported_functionalities = functionality_descriptor.bmFunctionality;
                }
                FsctDescriptorSet::TextMetadata(text_metadata_descriptor) => {
                    self.fsct_text_encoding = text_metadata_descriptor.bSystemTextCoding;
                    for metadata_part in &text_metadata_descriptor.aMetadata {
                        self.supported_current_texts.push(SupportedMetadata {
                            metadata: metadata_part.bMetadata,
                            max_length: metadata_part.wMaxLength as usize,
                        });
                    }
                }
                _ => ()
            }
        }
    }

    pub fn time_diff(&self) -> Option<std::time::Duration> {
        self.time_diff
    }

    async fn synchronize_time(&mut self) -> Result<(), String> {
        if !self.supported_functionalities.contains(FsctFunctionality::CurrentPlaybackProgress) {
            return Err("Device does not support current playback progress, so it can't synchronize time".to_string());
        }
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
        self.time_diff = Some(std::time::Duration::from_millis(time_diff as u64));
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
        if !self.supported_functionalities.contains(FsctFunctionality::CurrentPlaybackProgress) {
            return Ok(()); // not supported, omitting
        }
        let time_diff = self.time_diff.ok_or("Time is not synchronized")?;
        match progress {
            None => self.fsct_interface.disable_track_progress().await,
            Some(progress) => {
                let timestamp = std::time::SystemTime::now();
                let duration_since_update_time = timestamp.duration_since(progress.update_time).map_err(
                    |e| format!("Failed to get time difference.\
                    It seems that timestamp is later than now. Error: {}", e)
                )?;

                let position = progress.position + (duration_since_update_time.as_secs_f64() * progress.rate as f64);
                let device_timestamp = (timestamp - time_diff).duration_since(std::time::UNIX_EPOCH)
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


    pub async fn set_current_text(&self, text_id: FsctTextMetadata, text: Option<&str>) -> Result<(), String>
    {
        let supported_metadata =
            self.supported_current_texts.iter().find(|metadata| metadata.metadata == text_id).copied();
        if supported_metadata.is_none() {
            return Ok(());
        }
        let supported_metadata = supported_metadata.unwrap();

        match text {
            None => self.fsct_interface.disable_current_text(text_id).await,
            Some(text) => {
                let data_text = to_usb_encoded_text(self.fsct_text_encoding, text, supported_metadata.max_length);
                self.fsct_interface.send_current_text(text_id, data_text.as_slice()).await
            }
        }
    }

    pub async fn set_status(&self, status: crate::usb::requests::FsctStatus) -> Result<(), String>
    {
        self.fsct_interface.send_status(status).await
    }
}

impl Drop for FsctDevice {
    fn drop(&mut self) {
        if let Some(handle) = self.poll_task_handle.take() {
            log::info!("Stopping FSCT device polling task");
            handle.abort();
        }
    }
}

fn floor_char_boundary_utf8(text: &str, max_length: usize) -> &str {
    let mut new_text_length = text.len().min(max_length);
    while !text.is_char_boundary(new_text_length) {
        new_text_length -= 1;
    }
    &text[..new_text_length]
}

fn to_usb_encoded_text(fsct_text_encoding: FsctTextEncoding, text: &str, max_length_in_bytes: usize) -> Vec<u8> {
    match fsct_text_encoding {
        FsctTextEncoding::Ucs2 => {
            text.chars().map(|c| {
                if (c as u32) < (u16::MAX as u32) {
                    c as u16
                } else {
                    char::REPLACEMENT_CHARACTER as u16
                }
            }).take(max_length_in_bytes / 2).map(u16::to_ne_bytes).flatten().collect()
        }
        FsctTextEncoding::Utf8 => {
            floor_char_boundary_utf8(text, max_length_in_bytes).as_bytes().to_vec()
        }
        FsctTextEncoding::Utf16 => {
            let mut res: Vec<u8> = text.encode_utf16().take(max_length_in_bytes / 2)
                                       .map(u16::to_ne_bytes)
                                       .flatten()
                                       .collect();
            if (res.last().unwrap_or(&0) & 0xFC) == 0xD8 {
                // when last word starts from utf-16 4-word marker, we remove half of the character
                let new_len = res.len() - 2;
                res.resize(new_len, 0);
            }
            res
        }
        FsctTextEncoding::Utf32 => {
            text.chars().map(|c| c as u32).take(max_length_in_bytes / 4).map(u32::to_ne_bytes).flatten().collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fsct_device_to_usb_encoded_utf16_simple_text() {
        let text = "Hello World";
        let encoded_text = to_usb_encoded_text(FsctTextEncoding::Utf16, text, 10);
        assert_eq!(encoded_text, vec![72, 00, 101, 00, 108, 00, 108, 00, 111, 00]);
    }

    #[test]
    fn test_fsct_device_to_usb_encoded_utf16_latin_text() {
        let text = "Dzień dobry, witaj świecie!";
        let encoded_text = to_usb_encoded_text(FsctTextEncoding::Utf16, text, 10);
        let required: Vec<u8> = text.encode_utf16().take(5).map(u16::to_ne_bytes).flatten().collect();
        assert_eq!(encoded_text, required);
    }

    #[test]
    fn test_fsct_device_to_usb_encoded_multichar_utf16_with_last_char_in_the_middle_of_max_length() {
        let text = "abcd\u{10437}";
        let encoded_text = to_usb_encoded_text(FsctTextEncoding::Utf16, text, 10);
        let required: Vec<u8> = text.encode_utf16().take(4).map(u16::to_ne_bytes).flatten().collect(); // we know
        // that last character does not fit
        assert_eq!(encoded_text, required);
    }

    #[test]
    fn test_fsct_device_to_usb_encoded_multichar_utf16_with_last_char_fits_but_it_is_in_the_end() {
        let text = "abcd\u{10437}abc";
        let encoded_text = to_usb_encoded_text(FsctTextEncoding::Utf16, text, 12);
        let required: Vec<u8> = text.encode_utf16().take(6).map(u16::to_ne_bytes).flatten().collect();
        assert_eq!(encoded_text, required);
    }

    #[test]
    fn test_fsct_device_to_usb_encoded_multichar_utf8_with_last_char_in_the_middle_of_max_length() {
        let text = "abcd\u{10437}";
        let encoded_text = to_usb_encoded_text(FsctTextEncoding::Utf8, text, 5);
        let required: Vec<u8> = "abcd".as_bytes().to_vec();
        assert_eq!(encoded_text, required);
    }

    #[test]
    fn test_fsct_device_to_usb_encoded_multichar_utf8_with_last_char_in_the_middle_of_max_length2() {
        let text = "abcd\u{10437}";
        let encoded_text = to_usb_encoded_text(FsctTextEncoding::Utf8, text, 5);
        let required: Vec<u8> = "abcd".as_bytes().to_vec();
        assert_eq!(encoded_text, required);
    }

    #[test]
    fn test_fsct_device_to_usb_encoded_multichar_utf8_with_last_char_in_the_middle_of_max_length3() {
        let text = "abcd\u{10437}";
        let encoded_text = to_usb_encoded_text(FsctTextEncoding::Utf8, text, 7);
        let required: Vec<u8> = "abcd".as_bytes().to_vec();
        assert_eq!(encoded_text, required);
    }

    #[test]
    fn test_fsct_device_to_usb_encoded_multichar_utf8_with_last_char_in_the_end() {
        let text = "abcd\u{10437}";
        let encoded_text = to_usb_encoded_text(FsctTextEncoding::Utf8, text, 8);
        let required: Vec<u8> = text.as_bytes().to_vec();
        assert_eq!(encoded_text, required);
    }

    #[test]
    fn test_fsct_device_to_usb_encoded_multichar_utf8_length0() {
        let text = "";
        let encoded_text = to_usb_encoded_text(FsctTextEncoding::Utf8, text, 5);
        let required: Vec<u8> = "".as_bytes().to_vec();
        assert_eq!(encoded_text, required);
    }

    #[test]
    fn test_fsct_device_to_usb_encoded_multichar_utf8_with_only_char_doesnt_fit() {
        let text = "\u{10437}";
        let encoded_text = to_usb_encoded_text(FsctTextEncoding::Utf8, text, 2);
        let required: Vec<u8> = "".as_bytes().to_vec();
        assert_eq!(encoded_text, required);
    }
}


use bitflags::bitflags;

bitflags! {
    #[derive(Debug, Clone, Copy, Default)]
    pub struct FsctFunctionality: u8 {
        const CurrentPlaybackMetadata = 0x01;
        const CurrentPlaybackProgress = 0x02;
        const CurrentPlaybackStatus = 0x04;
        const PlaybackQueueMetadata = 0x08;
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, Default)]
pub enum FsctTextMetadata {
    #[default]
    CurrentTitle = 0x01,
    CurrentAuthor = 0x02,
    CurrentGenre = 0x03,
    CurrentYear = 0x04,
    CurrentTrack = 0x05,
    CurrentAlbum = 0x06,
    CurrentComment = 0x07,
    CurrentRating = 0x08,
    QueueTitle = 0x31,
    QueueAuthor = 0x32,
    QueueGenre = 0x33,
    QueueYear = 0x34,
    QueueTrack = 0x35,
    QueueAlbum = 0x36,
    QueueComment = 0x37,
    QueueRating = 0x38,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, Default)]
pub enum FsctImagePixelFormat {
    #[default]
    Rgb565 = 0x01,
    Rgb888 = 0x02,
    Bgr565 = 0x03,
    Bgr888 = 0x04,
    Grayscale4 = 0x05,
    Grayscale8 = 0x06,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum FsctTextDirection {
    LeftToRight = 0,
    RightToLeft = 1,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum FsctTextEncoding {
    Utf8 = 0,
    Utf16 = 1,
    Unicode16 = 2,
    Unicode32 = 3,
}
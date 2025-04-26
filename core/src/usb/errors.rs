use std::io;
use thiserror::Error;


#[derive(Error, Debug)]
pub enum IoErrorOr<T>
where
    T: Sized,
{
    #[error("IO error: {0}")]
    IoError(#[from] io::Error),

    #[error("Other error: {0}")]
    Or(T),
}

#[derive(Error, Debug)]
pub enum BosError {
    #[error("BOS descriptor not found")]
    NotFound,

    #[error("Data is too short to parse {name}: expected {expected}, got {actual} bytes")]
    TooShort {
        name: &'static str,
        expected: usize,
        actual: usize,
    },

    #[error("Wrong type of {name}: expected {expected}, got {actual}")]
    WrongType {
        name: &'static str,
        expected: u8,
        actual: u8,
    },

    #[error("BOS capability type mismatch, got {0}")]
    CapabilityTypeMismatch(u8),

    #[error("BOS FSCT capability version mismatch, expected {expected}, got {actual}")]
    FsctCapabilityVersionMismatch {
        expected: u16,
        actual: u16,
    },
}

#[derive(Error, Debug)]
pub enum DescriptorError {
    #[error("Not a FSCT functionality descriptor")]
    NotFsctFunctionalityDescriptor,

    #[error("Not a FSCT image metadata descriptor")]
    NotFsctImageMetadataDescriptor,

    #[error("Not a FSCT text metadata descriptor")]
    NotFsctTextMetadataDescriptor,

    #[error("Descriptor is too short")]
    TooShort,

    #[error("Interface not found")]
    InterfaceNotFound,
}

#[derive(Error, Debug)]
pub enum FsctDeviceError {
    #[error("Failed to get BOS descriptor: {0}")]
    BosError(#[from] BosError),

    #[error("Failed to get FSCT descriptor: {0}")]
    DescriptorError(#[from] DescriptorError),

    #[error("Interface not found")]
    InterfaceNotFound,

    #[error("Protocol version {0} not supported")]
    ProtocolVersionNotSupported(u8),

    #[error("BOS FSCT capability not available")]
    BosFsctCapabilityNotAvailable,

    #[error("Other error: {0}")]
    OtherError(String),
}

impl From<String> for FsctDeviceError {
    fn from(other: String) -> Self {
        FsctDeviceError::OtherError(other)
    }
}

impl From<IoErrorOr<BosError>> for IoErrorOr<FsctDeviceError>
where
{
    fn from(other: IoErrorOr<BosError>) -> Self {
        match other {
            IoErrorOr::IoError(e) => IoErrorOr::IoError(e),
            IoErrorOr::Or(f) => IoErrorOr::Or(f.into()),
        }
    }
}

impl From<IoErrorOr<DescriptorError>> for IoErrorOr<FsctDeviceError>
where
{
    fn from(other: IoErrorOr<DescriptorError>) -> Self {
        match other {
            IoErrorOr::IoError(e) => IoErrorOr::IoError(e),
            IoErrorOr::Or(f) => IoErrorOr::Or(f.into()),
        }
    }
}

impl From<BosError> for IoErrorOr<BosError>
where
{
    fn from(other: BosError) -> Self {
        IoErrorOr::Or(other)
    }
}

impl From<DescriptorError> for IoErrorOr<DescriptorError>
where
{
    fn from(other: DescriptorError) -> Self {
        IoErrorOr::Or(other)
    }
}

impl From<BosError> for IoErrorOr<FsctDeviceError> {
    fn from(error: BosError) -> Self {
        IoErrorOr::Or(FsctDeviceError::from(error))
    }
}

impl From<DescriptorError> for IoErrorOr<FsctDeviceError> {
    fn from(error: DescriptorError) -> Self {
        IoErrorOr::Or(FsctDeviceError::from(error))
    }
}

impl From<FsctDeviceError> for IoErrorOr<FsctDeviceError> {
    fn from(error: FsctDeviceError) -> Self {
        IoErrorOr::Or(error)
    }
}

impl From<String> for IoErrorOr<FsctDeviceError> {
    fn from(error: String) -> Self {
        IoErrorOr::Or(FsctDeviceError::from(error))
    }
}
use std::io;
use anyhow::{anyhow};
use thiserror::Error;


#[derive(Error, Debug)]
pub enum IoErrorOrAny
{
    #[error("IO error -> {0}")]
    IoError(#[from] io::Error),

    #[error(transparent)]
    Or(#[from] anyhow::Error),
}


#[derive(Error, Debug)]
pub enum DeviceDiscoveryError
{
    #[error("IO error -> {0}")]
    IoError(#[from] io::Error),

    #[error("No interface found")]
    InterfaceNotFound,

    #[error("Protocol version {0} not supported")]
    ProtocolVersionNotSupported(u8),

    #[error("Device initialization error -> {0}")]
    DeviceInitializationError(FsctDeviceError),

    #[error(transparent)]
    Or(#[from] anyhow::Error),
}

impl From<FsctDeviceError> for DeviceDiscoveryError {
    fn from(error: FsctDeviceError) -> Self {
        DeviceDiscoveryError::DeviceInitializationError(error.into())
    }
}

impl From<IoErrorOrAny> for DeviceDiscoveryError {
    fn from(error: IoErrorOrAny) -> Self {
        match error {
            IoErrorOrAny::IoError(error) => DeviceDiscoveryError::IoError(error),
            IoErrorOrAny::Or(error) => DeviceDiscoveryError::Or(error),
        }
    }
}

impl From<BosError> for IoErrorOrAny {
    fn from(error: BosError) -> Self {
        IoErrorOrAny::Or(error.into())
    }
}

impl From<DescriptorError> for IoErrorOrAny {
    fn from(error: DescriptorError) -> Self {
        IoErrorOrAny::Or(error.into())
    }
}

impl From<FsctDeviceError> for IoErrorOrAny {
    fn from(error: FsctDeviceError) -> Self {
        IoErrorOrAny::Or(error.into())
    }
}

impl From<String> for IoErrorOrAny {
    fn from(error: String) -> Self {
        IoErrorOrAny::Or(anyhow!(error))
    }
}

#[derive(Error, Debug)]
pub enum BosError {
    #[error("BOS descriptor not available, unsupported usb version {:2x}.{:02x}", .0 >> 8, .0 & 0xff)]
    NotAvailable(u16),

    #[error("Fsct capability not available")]
    NotFsctCapability,

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
}

#[derive(Error, Debug)]
pub enum FsctDeviceError {
    #[error("Time is not synchronized")]
    TimeNotSynchronized,

    #[error("Time difference is too large")]
    TimeDifferenceTooLarge,

    #[error("Time difference is negative")]
    TimeDifferenceNegative,

    #[error("Failed to get time difference. It seems that timestamp is later than now. Error: {0}")]
    TimeDifferenceCalculationError(String),

    #[error("Device does not support current playback progress, so it can't synchronize time")]
    PlaybackProgressNotSupported,

    #[error("USB control transfer failed: {0}")]
    UsbControlTransferError(#[source] anyhow::Error),

    #[error("Expected {expected} bytes, got {actual}")]
    DataSizeMismatch {
        expected: usize,
        actual: usize,
    },
}

pub trait ToFsctDeviceError {
    fn map_to_fsct_device_control_transfer_error(self) -> FsctDeviceError;
}

pub trait ToFsctDeviceResult<T> {
    fn map_err_to_fsct_device_control_transfer_error(self) -> Result<T, FsctDeviceError>;
}

impl<E> ToFsctDeviceError for E
where
    E: Into<anyhow::Error>,
{
    fn map_to_fsct_device_control_transfer_error(self) -> FsctDeviceError {
        FsctDeviceError::UsbControlTransferError(self.into())
    }
}

// impl ToFsctDeviceError for anyhow::Error
// {
//     fn map_to_fsct_device_control_transfer_error(self) -> FsctDeviceError {
//         FsctDeviceError::UsbControlTransferError(self)
//     }
// }

impl<T, E> ToFsctDeviceResult<T> for Result<T, E>
where
    E: ToFsctDeviceError,
{
    fn map_err_to_fsct_device_control_transfer_error(self) -> Result<T, FsctDeviceError> {
        self.map_err(|e| e.map_to_fsct_device_control_transfer_error())
    }
}
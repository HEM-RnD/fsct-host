use std::io;
use anyhow::{anyhow, bail};
use thiserror::Error;


#[derive(Error, Debug)]
pub enum IoErrorOrAny
{
    #[error("IO error -> {0}")]
    IoError(#[from] io::Error),

    #[error(transparent)]
    Or(#[from] anyhow::Error),
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

    #[error("Interface not found")]
    InterfaceNotFound,
}

#[derive(Error, Debug)]
pub enum FsctDeviceError {
    #[error("Interface not found")]
    InterfaceNotFound,

    #[error("Protocol version {0} not supported")]
    ProtocolVersionNotSupported(u8),
}

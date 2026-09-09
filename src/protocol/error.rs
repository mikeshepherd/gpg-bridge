use std::string::FromUtf8Error;

use ssh_agent_lib::ssh_encoding::Error as EncodingError;
use tokio_util::bytes::TryGetError;

#[derive(Debug, PartialEq, Clone)]
pub enum ProtocolError {
    Length,
    InvalidUtf8(String),
    NotEnoughBytes(String),
    EncodingError(String),
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ProtocolError::Length => write!(f, "Invalid length"),
            ProtocolError::InvalidUtf8(details) => write!(f, "Invalid utf8. {}", details),
            ProtocolError::NotEnoughBytes(details) => write!(f, "Not enough bytes. {}", details),
            ProtocolError::EncodingError(details) => write!(f, "Encoding error. {}", details),
        }
    }
}

impl std::error::Error for ProtocolError {}

pub type ProtocolResult<T> = Result<T, ProtocolError>;

impl From<FromUtf8Error> for ProtocolError {
    fn from(value: FromUtf8Error) -> Self {
        Self::InvalidUtf8(value.to_string())
    }
}

impl From<TryGetError> for ProtocolError {
    fn from(value: TryGetError) -> Self {
        Self::NotEnoughBytes(value.to_string())
    }
}

impl From<EncodingError> for ProtocolError {
    fn from(value: EncodingError) -> Self {
        Self::EncodingError(value.to_string())
    }
}

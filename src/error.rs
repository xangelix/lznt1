use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum DecompressionError {
    #[error("Unexpected end of stream")]
    UnexpectedEof,

    #[error("Invalid block header")]
    InvalidHeader,

    #[error("Lookback offset out of bounds")]
    InvalidOffset,

    #[error("Input buffer too short for expected data")]
    InputTooShort,
}

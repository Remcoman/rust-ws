use std::{
    error::Error,
    fmt::{Display, Formatter, Result},
};

#[derive(Debug)]
pub enum WebSocketError {
    InvalidRequestHeader,
    WouldBlock,
    UnknownError,
    InvalidConnectionState,
}
impl Display for WebSocketError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            Self::InvalidRequestHeader => {
                write!(f, "Invalid request header")
            }
            Self::UnknownError => {
                write!(f, "Unknown connection error")
            }
            Self::WouldBlock => {
                write!(f, "Would block")
            }
            Self::InvalidConnectionState => {
                write!(f, "Invalid connection state")
            }
        }
    }
}
impl Error for WebSocketError {}

use std::{
    io::Write,
    net::{TcpStream, ToSocketAddrs},
};

use crate::{
    connection::{MessageHandler, WebSocketConnection},
    error::WebSocketError,
    http::HTTPHeader,
    message::Message,
};

pub struct WebSocketClientOptions<S: ToSocketAddrs> {
    pub addr: S,
}

pub struct WebSocketClient {
    connection: WebSocketConnection,
}

impl WebSocketClient {
    pub fn connect<S: ToSocketAddrs>(
        options: WebSocketClientOptions<S>,
    ) -> Result<Self, WebSocketError> {
        let mut stream =
            TcpStream::connect(options.addr).map_err(|_e| WebSocketError::UnknownError)?;

        let request = HTTPHeader::websocket_request();
        stream
            .write_all(&request.to_bytes())
            .map_err(|_e| WebSocketError::UnknownError)?;

        let response_header =
            HTTPHeader::read(&mut stream).map_err(|_| WebSocketError::InvalidRequestHeader)?;

        if !response_header.is_valid_websocket_response() {
            return Err(WebSocketError::InvalidRequestHeader);
        }

        Ok(Self {
            connection: WebSocketConnection::new(stream),
        })
    }

    pub fn on_message(&self, f: impl Fn(Message) + Send + 'static) -> MessageHandler {
        self.connection.on_message(f)
    }

    pub fn send(&mut self, message: Message) -> Result<(), WebSocketError> {
        self.connection.send(message)
    }

    pub fn iter_messages(&mut self) -> impl Iterator<Item = Message> + '_ {
        self.connection.iter_messages()
    }
}

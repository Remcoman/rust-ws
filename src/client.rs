use std::{
    io::Write,
    net::{SocketAddr, TcpStream},
};

use crate::{
    connection::{MessageIter, WebSocketConnection},
    http::HTTPHeader,
    message::Message,
};

pub struct WebSocketClientOptions {
    pub addr: SocketAddr,
}

pub struct WebSocketClient {
    options: WebSocketClientOptions,
    connection: WebSocketConnection,
}

impl WebSocketClient {
    pub fn connect(options: WebSocketClientOptions) -> Result<Self, Box<dyn std::error::Error>> {
        let mut stream = TcpStream::connect(options.addr)?;

        let request = HTTPHeader::websocket_request();
        stream.write_all(&request.to_bytes())?;

        Ok(Self {
            options,
            connection: WebSocketConnection::new(stream),
        })
    }

    pub fn send(&mut self, message: Message) -> Result<(), std::io::Error> {
        self.connection.send(message)
    }

    pub fn iter_messages(&mut self) -> MessageIter<TcpStream> {
        self.connection.iter_messages()
    }
}

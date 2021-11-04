use std::{
    io::{ErrorKind, Write},
    net::{TcpListener, TcpStream, ToSocketAddrs},
};

use crate::{connection::WebSocketConnection, error::WebSocketError, http::HTTPHeader};

pub struct WebSocketServerOptions<S: ToSocketAddrs> {
    pub addr: S,
}

impl Default for WebSocketServerOptions<&str> {
    fn default() -> Self {
        Self { addr: "0.0.0.0:80" }
    }
}

pub struct WebSocketServer {
    listener: TcpListener,
}

impl WebSocketServer {
    pub fn listen<S: ToSocketAddrs>(
        options: WebSocketServerOptions<S>,
    ) -> Result<Self, std::io::Error> {
        let listener = TcpListener::bind(options.addr)?;

        Ok(WebSocketServer { listener })
    }

    pub fn iter_connections(&self) -> ConnectionIter<'_> {
        ConnectionIter::new(&self.listener)
    }
}

pub type IterItem = Result<WebsocketConnectionPreAccept, WebSocketError>;

pub struct ConnectionIter<'a> {
    listener: &'a TcpListener,
}

impl<'a> ConnectionIter<'a> {
    pub fn new(listener: &'a TcpListener) -> Self {
        ConnectionIter { listener }
    }

    pub fn ok(self) -> impl Iterator<Item = WebsocketConnectionPreAccept> + 'a {
        self.filter_map(Result::ok)
    }

    pub fn auto_accept(self) -> impl Iterator<Item = WebSocketConnection> + 'a {
        self.filter_map(|e| e.and_then(|e| e.accept()).ok())
    }

    fn try_get_next(&self) -> IterItem {
        let (mut stream, _) = self.listener.accept().map_err(|e| match e.kind() {
            ErrorKind::WouldBlock => WebSocketError::WouldBlock,
            _ => WebSocketError::UnknownError,
        })?;

        let request_header =
            HTTPHeader::read(&mut stream).map_err(|_| WebSocketError::InvalidRequestHeader)?;

        if !request_header.is_valid_websocket_request() {
            return Err(WebSocketError::InvalidRequestHeader);
        }

        Ok(WebsocketConnectionPreAccept {
            header: request_header,
            stream,
        })
    }
}

impl Iterator for ConnectionIter<'_> {
    type Item = IterItem;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let conn = match self.try_get_next() {
                Ok(s) => s,
                Err(WebSocketError::WouldBlock) => continue,
                Err(e) => return Some(Err(e)),
            };

            return Some(Ok(conn));
        }
    }
}

pub struct WebsocketConnectionPreAccept {
    stream: TcpStream,
    header: HTTPHeader,
}

impl WebsocketConnectionPreAccept {
    pub fn get_header<R: AsRef<[u8]>>(&self, name: R) -> Option<&[u8]> {
        self.header.get_value(name)
    }

    pub fn accept(mut self) -> Result<WebSocketConnection, WebSocketError> {
        let response_header = self.header.into_websocket_response();
        self.stream
            .write_all(&response_header.to_bytes())
            .map_err(|_| WebSocketError::UnknownError)?;
        Ok(WebSocketConnection::new(self.stream))
    }
}

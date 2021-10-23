use std::{
    fmt::Display,
    io::{ErrorKind, Write},
    net::{TcpListener, TcpStream},
    stream::Stream,
    task::Poll,
};

use crate::{
    connection::WebSocketConnection,
    http::{self, HTTPHeader},
};

pub struct WebSocketServerOptions {
    pub port: u16,
}

pub struct WebSocketServer {
    options: WebSocketServerOptions,
    listener: TcpListener,
}

impl WebSocketServer {
    pub fn listen(options: WebSocketServerOptions) -> Result<Self, Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(format!("0.0.0.0:{}", options.port))?;
        //listener.set_nonblocking(true)?;

        Ok(WebSocketServer { options, listener })
    }

    pub fn iter_connections(&self) -> ConnectionIter<'_> {
        ConnectionIter::new(&self.listener)
    }
}

#[derive(Debug)]
pub enum ConnectionError {
    InvalidRequestHeader,
    WouldBlock,
    UnknownError,
}
impl Display for ConnectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
        }
    }
}
impl std::error::Error for ConnectionError {}

pub type IterItem = Result<WebsocketConnectionPreAccept, ConnectionError>;

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
            ErrorKind::WouldBlock => ConnectionError::WouldBlock,
            _ => ConnectionError::UnknownError,
        })?;

        let request_header = http::HTTPHeader::read(&mut stream)
            .map_err(|_| ConnectionError::InvalidRequestHeader)?;

        if !request_header.is_valid_websocket_request() {
            return Err(ConnectionError::InvalidRequestHeader);
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
                Err(ConnectionError::WouldBlock) => continue,
                Err(e) => return Some(Err(e)),
            };

            return Some(Ok(conn));
        }
    }
}

impl Stream for ConnectionIter<'_> {
    type Item = IterItem;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        match self.try_get_next() {
            Ok(s) => Poll::Ready(Some(Ok(s))),
            Err(ConnectionError::WouldBlock) => Poll::Pending,
            Err(e) => Poll::Ready(Some(Err(e))),
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

    pub fn accept(mut self) -> Result<WebSocketConnection, ConnectionError> {
        let response_header = self.header.into_websocket_response();
        self.stream
            .write(&response_header.to_bytes())
            .map_err(|_| ConnectionError::UnknownError)?;
        Ok(WebSocketConnection::new(self.stream))
    }
}

#![feature(async_stream)]

use async_std::task;
use rust_ws::server::{WebSocketServer, WebSocketServerOptions};

fn main() {
    let s = WebSocketServer::listen(WebSocketServerOptions {
        port: 3001,
        ..Default::default()
    })
    .unwrap();

    task::block_on(async move { while let conn = s.iter_connections().next().await {} });
}

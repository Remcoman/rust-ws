use std::time::Duration;

use rust_ws::{
    client::{WebSocketClient, WebSocketClientOptions},
    message::Message,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = WebSocketClient::connect(WebSocketClientOptions {
        addr: "0.0.0.0:3000",
    })?;

    println!("start");

    let handler = client.on_message(|message| {
        println!("{:?}", message);
    });

    std::thread::sleep(Duration::from_secs(3));
    client
        .send(Message::Text("message from client".to_owned()))
        .unwrap();

    let joiner = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(20));
        handler.stop();
    });

    joiner.join().unwrap();

    println!("done");

    Ok(())
}

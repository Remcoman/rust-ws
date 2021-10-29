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

    let handle = client.on_message(|message| {
        println!("{:?}", message);
    });

    std::thread::sleep(Duration::from_secs(3));
    client.send(Message::Text("hoi".to_owned())).unwrap();

    handle.join().unwrap();

    println!("done");

    Ok(())
}

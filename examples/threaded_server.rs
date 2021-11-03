use std::time::Duration;

use rust_ws::{message::Message, server::WebSocketServer, server::WebSocketServerOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let s = WebSocketServer::listen(WebSocketServerOptions {
        addr: "0.0.0.0:3000",
    })
    .unwrap();

    println!("start");

    // loop through each connection and auto accept
    for conn in s.iter_connections().auto_accept() {
        println!("conn");

        // create a sender which can be used to...send messages
        let mut sender = conn.sender();

        // register a callback for messages
        conn.on_message(move |message| {
            println!("{:?}", message);
            //sender.send(Message::Text("hoi".to_owned())).unwrap();
        });

        //spawn a new thread that after 3 seconds will send a message through the connection
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(6));
            println!("sending message back");
            sender
                .send(Message::Text("message from server".to_owned()))
                .unwrap()
        });

        std::thread::sleep(Duration::from_secs(15));
        conn.close().unwrap();
    }

    println!("done");

    Ok(())
}

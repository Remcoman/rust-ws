use rust_ws::{server::WebSocketServer, server::WebSocketServerOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let s = WebSocketServer::listen(WebSocketServerOptions { port: 3000 }).unwrap();

    println!("start");

    for mut conn in s.iter_connections().auto_accept() {
        let mut sender = conn.on_message(|message| {
            println!("{:?}", message);
        });

        std::thread::spawn(move || sender.send(rust_ws::message::Message::Pong).unwrap());
    }

    println!("done");
    Ok(())
}

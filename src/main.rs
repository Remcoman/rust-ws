use rust_ws::{server::WebSocketServer, server::WebSocketServerOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let s = WebSocketServer::listen(WebSocketServerOptions { port: 3000 }).unwrap();

    println!("start");

    for mut conn in s.iter_connections().auto_accept() {
        std::thread::spawn(move || {
            for message in conn.iter_messages().ok() {
                println!("{:?}", message);
            }
            println!("close");
        });
    }

    println!("done");
    Ok(())
}

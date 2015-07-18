extern crate mio;

mod server;

use mio::*;
use mio::tcp::*;
use server::mioserver::*;
use std::thread;

fn main() {
    let addr = "127.0.0.1:8088".parse().ok().expect("Couldn't parse address");

    // Setup the server socket
    let sock = TcpSocket::v4().ok().expect("Couldn't create socket");
    sock.set_reuseaddr(true).ok().expect("Couldn't set SO_REUSEADDR");
    sock.bind(&addr).ok().expect("Couldn't bind to address");
    let listener = sock.listen(2048).ok().expect("Couldn't bind to address");

    // Create an event loop
    let mut event_loop = EventLoop::new().ok().expect("Couldn't create EventLoop");
    // Create sender
    let sender = event_loop.channel();
    thread::spawn(move || {
        loop {
            thread::sleep_ms(5000);
            let _ = sender.send(());
        }
    });

    // Create the MioServer
    let mut mio_server = MioServer::new(listener);
    mio_server.run(&mut event_loop);
}
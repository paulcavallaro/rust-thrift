extern crate mio;

mod server;

use mio::*;
use mio::tcp::*;
use server::mioserver::*;

fn main() {
    let addr = "127.0.0.1:8088".parse().ok().expect("Couldn't parse address");

    // Setup the server socket
    let sock = TcpListener::bind(&addr).ok()
        .expect("Couldn't bind to address");

    // Create an event loop
    let mut event_loop = EventLoop::new().ok().expect("Couldn't create EventLoop");

    // Create the MioServer
    let mut mio_server = MioServer::new(sock);
    mio_server.run(&mut event_loop);
}
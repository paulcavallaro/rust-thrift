extern crate mio;

mod server;

use mio::*;
use mio::tcp::*;
use server::mioserver::*;

// Setup some tokens to allow us to identify which event is
// for which socket.
const SERVER: Token = Token(0);
const CLIENT: Token = Token(1);

fn main() {
    let addr = "127.0.0.1:8088".parse().unwrap();

    // Setup the server socket
    let sock = TcpListener::bind(&addr).unwrap();

    // Create an event loop
    let mut event_loop = EventLoop::new().unwrap();

    // Create the MioServer
    let mut mio_server = MioServer::new(sock);
    mio_server.run(&mut event_loop);
}
use mio::*;
use mio::tcp::*;
use mio::util::Slab;
use std::io::{Result, Error, ErrorKind};
use std::str;

#[derive(Clone)]
pub struct ServerConfig {
    timeout_ms : u64,
}

impl ServerConfig {
    pub fn default() -> ServerConfig {
        ServerConfig {
            timeout_ms : 3000,
        }
    }
}

enum ConnState {
    Closed,
    Open,
}

pub struct MioServer {
    sock: TcpListener,
    conns: Slab<MioConn>,
    config : ServerConfig,
}

pub struct MioConn {
    sock: TcpStream,
    token: Option<Token>,
    interest: EventSet,
    timeout: Option<Timeout>,
    config: ServerConfig,
}

impl MioConn {
    pub fn new(sock: TcpStream, config: ServerConfig) -> MioConn {
        MioConn {
            sock: sock,
            token: None,
            interest: EventSet::hup() | EventSet::error(),
            timeout: None,
            config: config,
        }
    }

    fn clear_timeout(&self, event_loop: &mut EventLoop<MioServer>) {
        self.timeout.map(|timeout| event_loop.clear_timeout(timeout));
    }

    fn set_timeout(&mut self, event_loop: &mut EventLoop<MioServer>) {
        // TODO(ptc) handle timeouts and server configuration management better
        let timeout = event_loop.timeout_ms(self.token.unwrap(), self.config.timeout_ms).ok().expect("Should have set a timeout");
        self.timeout = Some(timeout);
    }

    /// Called when connection has timed out
    fn timeout(&mut self, _event_loop: &mut EventLoop<MioServer>) -> Result<()> {
        Ok(())
    }

    /// Called when connection has been closed/hung up
    fn closed(&self, event_loop: &mut EventLoop<MioServer>) -> Result<()> {
        self.clear_timeout(event_loop);
        Ok(())
    }

    fn is_done_reading(&self, buf : &[u8], len : usize) -> bool {
        str::from_utf8(buf).ok().expect("oops").contains("\r\n\r\n")
    }

    /// Called when connection has data to be read
    fn readable(&mut self, event_loop: &mut EventLoop<MioServer>) -> Result<ConnState> {
        self.clear_timeout(event_loop);
        let mut buf = [0; 2048];

        match self.sock.try_read(&mut buf) {
            Ok(None) => {
                panic!("We just got readable, but were unable to read from the socket?");
            }
            Ok(Some(read)) => {
                if self.is_done_reading(&buf, read) {
                    self.interest.remove(EventSet::readable());
                    self.interest.insert(EventSet::writable());
                }
            }
            Err(e) => {
                // TODO(ptc) better error handling?
                println!("Error e: {:?}", e)
            }
        };

        event_loop.reregister(&self.sock, self.token.unwrap(), self.interest, PollOpt::edge() | PollOpt::oneshot()).ok().expect("Should have reregistered in readable");
        self.set_timeout(event_loop);
        Ok(ConnState::Open)
    }

    fn is_done_writing(&self) -> bool {
        true
    }

    /// Called when connection can be written to
    fn writable(&mut self, event_loop: &mut EventLoop<MioServer>) -> Result<ConnState> {
        self.clear_timeout(event_loop);
        let msg = "HTTP/1.1 200 OK\r\n<h1>200 Ok</h1>";

        match self.sock.try_write(msg.as_bytes()) {
            Ok(None) => {
                // Writing would block...
                println!("Writing would block... {:?}", self.token.unwrap());
            }
            Ok(Some(_wrote)) => {
                // Wrote response to client
                if self.is_done_writing() {
                    return Ok(ConnState::Closed)
                }
            }
            Err(_e) => {
                // TODO(ptc) handle error
                println!("Error writing msg... {:?}", self.token.unwrap());
                ()
            },
        }

        event_loop.reregister(&self.sock, self.token.unwrap(), self.interest, PollOpt::edge() | PollOpt::oneshot()).ok().expect("Shoulda reregistered in writable");
        self.set_timeout(event_loop);
        Ok(ConnState::Open)
    }
}

const SERVER: Token = Token(0);

impl Handler for MioServer {
    type Timeout = Token;
    type Message = (); // TODO(ptc) Figure out Message type from workers

    fn ready(&mut self, event_loop: &mut EventLoop<Self>, token: Token,
             events: EventSet) {
        // TODO(ptc) revisit error handling, not sure if okay to ignore
        // errors on readable/writable/closed
        if events.is_readable() {
            match token {
                SERVER => {
                    self.accept(event_loop).err().map(|e| println!("Error accepting: {:?}", e));
                }
                conn => {
                    self.conn_readable(event_loop, conn).ok().expect("Should have read");
                }
            }
        }

        if events.is_writable() {
            match token {
                SERVER => {
                    panic!("received writable for SERVER events: '{:?}' token: '{:?}'", events, token);
                },
                conn => {
                    self.conn_writable(event_loop, conn).ok().expect("Should have written");
                },
            }
        }

        if events.is_hup() {
            match token {
                SERVER => panic!("listening socket closed underneath us!"),
                conn => {
                    self.conn_closed(event_loop, conn).ok().expect("Should have closed");
                },
            }
        }

        if events.is_error() {
            match token {
                SERVER => panic!("got error event for SERVER: '{:?}'", events),
                conn => {
                    println!("Got error for token: {:?}", conn);
                    self.conns.remove(conn).expect("error should remove");
                },
            }
        }
    }

    fn notify(&mut self, _event_loop: &mut EventLoop<Self>, _msg: Self::Message) {
    }

    fn timeout(&mut self, event_loop: &mut EventLoop<Self>, token: Self::Timeout) {
        // Connection associated with token has timed out on its last action
        match token {
            SERVER => panic!("Never specified a timeout on the SERVER socket!"),
            conn => {
                println!("Timeout on connection {:?}", token);
                let _ = self.conn_timeout(event_loop, conn);
            },
        }
    }
}

impl MioServer {

    pub fn new(sock: TcpListener) -> MioServer {
        MioServer {
            sock : sock,
            conns : Slab::new_starting_at(Token(SERVER.as_usize() + 1), 1024),
            config : ServerConfig::default(),
        }
    }

    /// Accept a pending connection on the listening socket
    pub fn accept(&mut self, event_loop: &mut EventLoop<Self>) -> Result<()> {
        let res = try!(self.sock.accept());
        let sock = try!(res.ok_or(Error::new(ErrorKind::WouldBlock,
                                             "Accepting would block")));
        let conn = MioConn::new(sock, self.config.clone());
        // Drop the connection on the floor if we can't allocate from the slab
        let tok = try!(self.conns.insert(conn).map_err(|_drop_conn| Error::new(ErrorKind::Other, "Exhausted connections in slab")));

        // Register the connection
        self.conns[tok].token = Some(tok);
        event_loop.register_opt(&self.conns[tok].sock, tok, EventSet::readable() | EventSet::hup() | EventSet::error(), PollOpt::edge() | PollOpt::oneshot())
            .ok().expect("could not register socket with event loop");
        // TODO(ptc) proper error handling...
        self.conns[tok].set_timeout(event_loop);

        Ok(())
    }

    /// Handle when a connection is readable
    fn conn_readable(&mut self, event_loop: &mut EventLoop<Self>, tok: Token) -> Result<()> {
        // TODO(ptc) might not be enough to just see if None, because the connection
        // could close/timeout and we might remove the conn from the slab, but then
        // reallocate the token from the slab, and incorrectly assume old events are
        // for the new conn
        let res = match self.conns.get_mut(tok) {
            // Events delivered for already closed connection
            None => {
                println!("conn_readable: for missing token: {:?}", tok);
                return Ok(());
            },
            Some(conn) => conn.readable(event_loop),
        };
        match res {
            Ok(ConnState::Closed) => {
                self.conns.remove(tok).expect("Should still be there");
                Ok(())
            },
            Ok(ConnState::Open) => Ok(()),
            Err(e) => {
                println!("Error e: {:?}", e);
                Err(e)
            },
        }
    }

    /// Handle when a connection is writable
    fn conn_writable(&mut self, event_loop: &mut EventLoop<Self>, tok: Token) -> Result<()> {
        let res = match self.conns.get_mut(tok) {
            // Events delivered for already closed connection
            None => {
                println!("conn_writable: for missing token: {:?}", tok);
                return Ok(());
            },
            Some(conn) => conn.writable(event_loop),
        };
        match res {
            Ok(ConnState::Closed) => {
                self.conns.remove(tok).expect("Should be removed");
                Ok(())
            },
            Ok(ConnState::Open) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Handle when a connection is hung up/closes
    fn conn_closed(&mut self, event_loop: &mut EventLoop<Self>, tok: Token) -> Result<()> {
        let res = match self.conns.get_mut(tok) {
            // Events delivered for already closed connection
            None => {
                println!("conn_closed: for missing token: {:?}", tok);
                return Ok(());
            },
            Some(conn) => conn.closed(event_loop),
        };
        self.conns.remove(tok).expect("Should be removed in closed");
        res
    }

    /// Handle when a connection is hung up/closes
    fn conn_timeout(&mut self, event_loop: &mut EventLoop<Self>, tok: Token) -> Result<()> {
        let res = match self.conns.get_mut(tok) {
            // Events delivered for already closed connection
            None => {
                println!("conn_timeout: for missing token: {:?}", tok);
                return Ok(());
            },
            Some(conn) => conn.timeout(event_loop),
        };
        self.conns.remove(tok).expect("Timeout removal should work");
        res
    }

    pub fn run(&mut self, event_loop : &mut EventLoop<Self>) {
        event_loop.register_opt(&self.sock, SERVER, EventSet::readable() | EventSet::hup() | EventSet::error(), PollOpt::level()).ok().expect("Unable to register server socket with event loop");
        event_loop.run(self).ok().expect("Couldn't run the event loop");
    }
}

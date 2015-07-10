use mio::*;
use mio::tcp::*;
use mio::buf::{ByteBuf, MutByteBuf};
use mio::util::Slab;
use std::io::{Result, Error, ErrorKind};

pub struct MioServer {
    sock: TcpListener,
    conns: Slab<MioConn>,
}

pub struct MioConn {
    sock: TcpStream,
    buf: Option<ByteBuf>,
    mut_buf: Option<MutByteBuf>,
    token: Option<Token>,
    interest: EventSet,
}

impl MioConn {
    pub fn new(sock: TcpStream) -> MioConn {
        MioConn {
            sock: sock,
            buf: None,
            mut_buf: Some(ByteBuf::mut_with_capacity(2048)),
            token: None,
            interest: EventSet::hup(),
        }
    }

    fn closed(&self, event_loop: &mut EventLoop<MioServer>) -> Result<()> {
        event_loop.deregister(&self.sock)
    }

    fn readable(&mut self, event_loop: &mut EventLoop<MioServer>) -> Result<()> {
        let mut buf = self.mut_buf.take().unwrap();

        match self.sock.try_read_buf(&mut buf) {
            Ok(None) => {
                panic!("We just got readable, but were unable to read from the socket?");
            }
            Ok(Some(_r)) => {
                self.interest.remove(EventSet::readable());
                self.interest.insert(EventSet::writable());
            }
            Err(_e) => {
                self.interest.remove(EventSet::readable());
            }
        };

        // prepare to provide this to writable
        self.buf = Some(buf.flip());
        event_loop.reregister(&self.sock, self.token.unwrap(), self.interest, PollOpt::edge() | PollOpt::oneshot())
    }

    fn writable(&mut self, event_loop: &mut EventLoop<MioServer>) -> Result<()> {
        let mut buf = self.buf.take().unwrap();

        match self.sock.try_write_buf(&mut buf) {
            Ok(None) => {
                self.buf = Some(buf);
                self.interest.insert(EventSet::writable());
            }
            Ok(Some(_r)) => {
                self.mut_buf = Some(buf.flip());

                self.interest.insert(EventSet::readable());
                self.interest.remove(EventSet::writable());
            }
            Err(_e) => {
                // TODO(ptc) handle error
                ()
            },
        }

        event_loop.reregister(&self.sock, self.token.unwrap(), self.interest, PollOpt::edge() | PollOpt::oneshot())
    }
}

const SERVER: Token = Token(0);

impl Handler for MioServer {
    type Timeout = MioConn;
    type Message = (); // TODO(ptc) Figure out Message type from workers

    fn ready(&mut self, event_loop: &mut EventLoop<Self>, token: Token,
             events: EventSet) {
        // TODO(ptc) revisit error handling, not sure if okay to ignore
        // errors on readable/writable/closed
        if events.is_readable() {
            match token {
                SERVER => {
                    let _ = self.accept(event_loop);
                }
                conn => {
                    let _ = self.conn_readable(event_loop, conn);
                }
            }
        }

        if events.is_writable() {
            match token {
                SERVER => {
                    println!("received writable for SERVER events: '{:?}' token: '{:?}'", events, token);
                },
                conn => {
                    let _ = self.conn_writable(event_loop, conn);
                },
            }
        }

        if events.is_hup() {
            match token {
                SERVER => panic!("listening socket closed underneath us!"),
                conn => {
                    let _ = self.conn_closed(event_loop, conn);
                },
            }
        }
    }

    fn notify(&mut self, _event_loop: &mut EventLoop<Self>, _msg: Self::Message) {
    }

    fn timeout(&mut self, _event_loop: &mut EventLoop<Self>, _msg: Self::Timeout) {
    }
}

impl MioServer {

    pub fn new(sock: TcpListener) -> MioServer {
        MioServer {
            sock : sock,
            conns : Slab::new_starting_at(Token(SERVER.as_usize() + 1), 1024),
        }
    }

    /// Accept a pending connection on the listening socket
    pub fn accept(&mut self, event_loop: &mut EventLoop<Self>) -> Result<()> {
        let res = try!(self.sock.accept());
        let sock = try!(res.ok_or(Error::new(ErrorKind::WouldBlock,
                                             "Accepting would block")));
        let conn = MioConn::new(sock);
        // TODO(ptc) proper error handling...
        let tok = self.conns.insert(conn)
            .ok().expect("could not add connection to slab");

        // Register the connection
        // TODO(ptc) need a timeout on the connection handshake...
        self.conns[tok].token = Some(tok);
        event_loop.register_opt(&self.conns[tok].sock, tok, EventSet::readable() | EventSet::hup(), PollOpt::edge() | PollOpt::oneshot())
            .ok().expect("could not register socket with event loop");

        Ok(())
    }

    /// Handle when a connection is readable
    fn conn_readable(&mut self, event_loop: &mut EventLoop<Self>, tok: Token) -> Result<()> {
        match self.conns.get_mut(tok) {
            // Events delivered for already closed connection
            None => Ok(()),
            Some(conn) => conn.readable(event_loop),
        }
    }

    /// Handle when a connection is writable
    fn conn_writable(&mut self, event_loop: &mut EventLoop<Self>, tok: Token) -> Result<()> {
        match self.conns.get_mut(tok) {
            // Events delivered for already closed connection
            None => Ok(()),
            Some(conn) => conn.writable(event_loop),
        }
    }

    /// Handle when a connection is hung up/closes
    fn conn_closed(&mut self, event_loop: &mut EventLoop<Self>, tok: Token) -> Result<()> {
        let res = match self.conns.get_mut(tok) {
            // Events delivered for already closed connection
            None => Ok(()),
            Some(conn) => conn.closed(event_loop),
        };
        self.conns.remove(tok);
        res
    }

    pub fn run(&mut self, event_loop : &mut EventLoop<Self>) {
        event_loop.register_opt(&self.sock, SERVER, EventSet::readable(), PollOpt::edge()).ok().expect("Unable to register server socket with event loop");
        event_loop.run(self).ok();
    }
}

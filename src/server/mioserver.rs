use mio::*;
use mio::tcp::*;
use mio::buf::{ByteBuf, MutByteBuf};
use mio::util::Slab;
use std::io;

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

    fn readable(&mut self, event_loop: &mut EventLoop<MioServer>) -> io::Result<()> {
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
        event_loop.reregister(&self.sock, self.token.unwrap(), self.interest, PollOpt::edge())
    }

    fn writable(&mut self, event_loop: &mut EventLoop<MioServer>) -> io::Result<()> {
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
        if events.is_readable() {
            match token {
                SERVER => {
                    self.accept(event_loop).unwrap()
                }
                conn => {
                    self.conn_readable(event_loop, conn).unwrap()
                }
            }
        }

        if events.is_writable() {
            match token {
                SERVER => panic!("received writable for SERVER token"),
                conn => self.conn_writable(event_loop, conn).unwrap(),
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
    pub fn accept(&mut self, event_loop: &mut EventLoop<Self>) -> io::Result<()> {
        // TODO(ptc) proper error handling
        let sock = self.sock.accept().unwrap().unwrap();
        let conn = MioConn::new(sock);
        let tok = self.conns.insert(conn)
            .ok().expect("could not add connection to slab");

        // Register the connection
        // TODO(ptc) need a timeout on the connection handshake...
        self.conns[tok].token = Some(tok);
        event_loop.register_opt(&self.conns[tok].sock, tok, EventSet::readable(), PollOpt::edge())
            .ok().expect("could not register socket with event loop");

        Ok(())
    }

    /// Handle when a connection is readable
    fn conn_readable(&mut self, event_loop: &mut EventLoop<Self>, tok: Token) -> io::Result<()> {
        self.conn(tok).readable(event_loop)
    }

    /// Handle when a connection is writable
    fn conn_writable(&mut self, event_loop: &mut EventLoop<Self>, tok: Token) -> io::Result<()> {
        self.conn(tok).writable(event_loop)
    }

    /// Access a specific connection
    fn conn<'a>(&'a mut self, tok: Token) -> &'a mut MioConn {
        &mut self.conns[tok]
}

  pub fn run(&mut self, event_loop : &mut EventLoop<Self>) {
      event_loop.register_opt(&self.sock, SERVER, EventSet::readable(), PollOpt::edge()).ok().expect("Unable to register server socket with event loop");
      event_loop.run(self).ok();
  }
}

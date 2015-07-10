use mio::*;
use mio::tcp::*;
use mio::buf::{ByteBuf, MutByteBuf, SliceBuf};
use mio::util::Slab;


struct MioConn {
  sock : TcpStream,
  buf : Option<ByteBuf>,
  mut_buf : Option<MutByteBuf>,
  token : Option<Token>,
  interest : Interest,
}

impl MioConn {
    fn new(sock: TcpStream) -> EchoConn {
        EchoConn {
            sock: sock,
            buf: None,
            mut_buf: Some(ByteBuf::mut_with_capacity(2048)),
            token: None,
            interest: Interest::hup()
        }
    }

    fn writable(&mut self, event_loop: &mut EventLoop<Echo>) -> io::Result<()> {
        let mut buf = self.buf.take().unwrap();

        match self.sock.try_write_buf(&mut buf) {
            Ok(None) => {
                debug!("client flushing buf; WOULDBLOCK");

                self.buf = Some(buf);
                self.interest.insert(Interest::writable());
            }
            Ok(Some(r)) => {
                debug!("CONN : we wrote {} bytes!", r);

                self.mut_buf = Some(buf.flip());

                self.interest.insert(Interest::readable());
                self.interest.remove(Interest::writable());
            }
            Err(e) => debug!("not implemented; client err={:?}", e),
        }

        event_loop.reregister(&self.sock, self.token.unwrap(), self.interest, PollOpt::edge() | PollOpt::oneshot())
    }

    fn readable(&mut self, event_loop: &mut EventLoop<Echo>) -> io::Result<()> {
        let mut buf = self.mut_buf.take().unwrap();

        match self.sock.try_read_buf(&mut buf) {
            Ok(None) => {
                panic!("We just got readable, but were unable to read from the socket?");
            }
            Ok(Some(r)) => {
                debug!("CONN : we read {} bytes!", r);
                self.interest.remove(Interest::readable());
                self.interest.insert(Interest::writable());
            }
            Err(e) => {
                debug!("not implemented; client err={:?}", e);
                self.interest.remove(Interest::readable());
            }

        };

        // prepare to provide this to writable
        self.buf = Some(buf.flip());
        event_loop.reregister(&self.sock, self.token.unwrap(), self.interest, PollOpt::edge())
    }
}
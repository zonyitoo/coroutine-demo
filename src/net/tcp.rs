use std::io;
use std::net::SocketAddr;
use std::ops::{Deref, DerefMut};

use mio::{self, Interest};
use mio::buf::{Buf, MutBuf, MutSliceBuf, SliceBuf};

use scheduler::Scheduler;

pub struct TcpSocket(::mio::tcp::TcpSocket);

impl TcpSocket {
    /// Returns a new, unbound, non-blocking, IPv4 socket
    pub fn v4() -> io::Result<TcpSocket> {
        Ok(TcpSocket(try!(::mio::tcp::TcpSocket::v4())))
    }

    /// Returns a new, unbound, non-blocking, IPv6 socket
    pub fn v6() -> io::Result<TcpSocket> {
        Ok(TcpSocket(try!(::mio::tcp::TcpSocket::v6())))
    }

    pub fn connect(self, addr: &SocketAddr) -> io::Result<(TcpStream, bool)> {
        let (stream, complete) = try!(self.0.connect(addr));
        Ok((TcpStream(stream), complete))
    }

    pub fn listen(self, backlog: usize) -> io::Result<TcpListener> {
        Ok(TcpListener(try!(self.0.listen(backlog))))
    }
}

impl Deref for TcpSocket {
    type Target = ::mio::tcp::TcpSocket;

    fn deref(&self) -> &::mio::tcp::TcpSocket {
        &self.0
    }
}

impl DerefMut for TcpSocket {
    fn deref_mut(&mut self) -> &mut ::mio::tcp::TcpSocket {
        &mut self.0
    }
}

pub struct TcpListener(::mio::tcp::TcpListener);

impl TcpListener {
    pub fn bind(addr: &SocketAddr) -> io::Result<TcpListener> {
        let listener = try!(::mio::tcp::TcpListener::bind(addr));

        Ok(TcpListener(listener))
    }

    pub fn accept(&self) -> io::Result<TcpStream> {
        match self.0.accept() {
            Ok(None) => {
                debug!("accept WOULDBLOCK; going to register into eventloop");
            },
            Ok(Some(stream)) => {
                return Ok(TcpStream(stream));
            },
            Err(err) => {
                return Err(err);
            }
        }

        try!(Scheduler::current().wait_event(&self.0, Interest::readable()));

        match self.0.accept() {
            Ok(None) => {
                panic!("accept WOULDBLOCK");
            },
            Ok(Some(stream)) => {
                Ok(TcpStream(stream))
            },
            Err(err) => {
                Err(err)
            }
        }
    }
}

impl Deref for TcpListener {
    type Target = ::mio::tcp::TcpListener;

    fn deref(&self) -> &::mio::tcp::TcpListener {
        &self.0
    }
}

impl DerefMut for TcpListener {
    fn deref_mut(&mut self) -> &mut ::mio::tcp::TcpListener {
        &mut self.0
    }
}

pub struct TcpStream(mio::tcp::TcpStream);

impl TcpStream {
    pub fn connect(addr: &SocketAddr) -> io::Result<TcpStream> {
        let stream = try!(mio::tcp::TcpStream::connect(addr));

        Ok(TcpStream(stream))
    }

    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.0.peer_addr()
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.0.local_addr()
    }

    pub fn try_clone(&self) -> io::Result<TcpStream> {
        let stream = try!(self.0.try_clone());

        Ok(TcpStream(stream))
    }
}

impl io::Read for TcpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        use mio::TryRead;

        let mut buf = MutSliceBuf::wrap(buf);
        while buf.has_remaining() {
            match self.0.read(&mut buf) {
                Ok(None) => {
                    debug!("TcpStream read WOULDBLOCK");
                    break;
                },
                Ok(Some(0)) => {
                    debug!("TcpStream read 0 bytes");
                    break;
                },
                Ok(Some(len)) => {
                    debug!("TcpStream read {} bytes", len);
                },
                Err(err) => {
                    return Err(err);
                }
            }
        }

        if buf.mut_bytes().len() != 0 {
            // We got something, just return!
            return Ok(buf.mut_bytes().len());
        }

        debug!("Read: Going to register event");
        try!(Scheduler::current().wait_event(&self.0, Interest::readable()));
        debug!("Read: Got read event");

        while buf.has_remaining() {
            match self.0.read(&mut buf) {
                Ok(None) => {
                    debug!("TcpStream read WOULDBLOCK");
                    break;
                },
                Ok(Some(0)) => {
                    debug!("TcpStream read 0 bytes");
                    break;
                },
                Ok(Some(len)) => {
                    debug!("TcpStream read {} bytes", len);
                },
                Err(err) => {
                    return Err(err);
                }
            }
        }

        Ok(buf.mut_bytes().len())
    }
}

impl io::Write for TcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        use mio::TryWrite;

        let mut buf = SliceBuf::wrap(buf);
        let mut total_len = 0;

        while buf.has_remaining() {
            match self.0.write(&mut buf) {
                Ok(None) => {
                    debug!("TcpStream write WOULDBLOCK");
                    break;
                },
                Ok(Some(0)) => {
                    debug!("TcpStream write 0 bytes");
                    break;
                },
                Ok(Some(len)) => {
                    debug!("TcpStream written {} bytes", len);
                    total_len += len;
                },
                Err(err) => {
                    return Err(err)
                }
            }
        }

        if total_len != 0 {
            // We have written something, return it!
            return Ok(total_len)
        }

        debug!("Write: Going to register event");
        try!(Scheduler::current().wait_event(&self.0, Interest::writable()));
        debug!("Write: Got write event");

        while buf.has_remaining() {
            match self.0.write(&mut buf) {
                Ok(None) => {
                    debug!("TcpStream write WOULDBLOCK");
                    break;
                },
                Ok(Some(0)) => {
                    debug!("TcpStream write 0 bytes");
                    break;
                },
                Ok(Some(len)) => {
                    debug!("TcpStream written {} bytes", len);
                    total_len += len;
                },
                Err(err) => {
                    return Err(err)
                }
            }
        }

        Ok(total_len)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Deref for TcpStream {
    type Target = ::mio::tcp::TcpStream;

    fn deref(&self) -> &::mio::tcp::TcpStream {
        &self.0
    }
}

impl DerefMut for TcpStream {
    fn deref_mut(&mut self) -> &mut ::mio::tcp::TcpStream {
        &mut self.0
    }
}

use std::io;
use std::net::SocketAddr;
use std::ops::{Deref, DerefMut};

use mio::{self, Interest};
use mio::buf::{Buf, MutBuf, MutSliceBuf, SliceBuf};

use scheduler::Scheduler;

pub struct TcpListener(::mio::tcp::TcpListener);

impl TcpListener {
    pub fn bind(addr: &SocketAddr) -> io::Result<TcpListener> {
        let listener = try!(::mio::tcp::TcpListener::bind(addr));

        Ok(TcpListener(listener))
    }

    pub fn accept(&self) -> io::Result<TcpStream> {
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

        debug!("Read: Going to register event");
        try!(Scheduler::current().wait_event(&self.0, Interest::readable()));

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
                }
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

        debug!("Write: Going to register event");

        try!(Scheduler::current().wait_event(&self.0, Interest::writable()));

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
                }
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

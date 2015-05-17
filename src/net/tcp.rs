use std::io;
use std::net::SocketAddr;
use std::ops::{Deref, DerefMut};

use mio::{self, Interest};

use scheduler::Scheduler;

pub struct TcpListener(::mio::tcp::TcpListener);

impl TcpListener {
    pub fn bind(addr: &SocketAddr) -> io::Result<TcpListener> {
        let listener = try!(::mio::tcp::TcpListener::bind(addr));

        Ok(TcpListener(listener))
    }

    pub fn accept(&self) -> io::Result<TcpStream> {
        Scheduler::current().wait_event(&self.0, Interest::readable()).unwrap();

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
        Scheduler::current().wait_event(&self.0, Interest::readable()).unwrap();

        match self.0.read_slice(buf) {
            Ok(None) => {
                panic!("TcpStream read WOULDBLOCK");
            },
            Ok(Some(len)) => {
                Ok(len)
            },
            Err(err) => {
                Err(err)
            }
        }
    }
}

impl io::Write for TcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        use mio::TryWrite;

        debug!("Write: Going to register event");

        Scheduler::current().wait_event(&self.0, Interest::writable()).unwrap();

        match self.0.write_slice(buf) {
            Ok(None) => {
                panic!("TcpStream write WOULDBLOCK");
            },
            Ok(Some(len)) => {
                Ok(len)
            },
            Err(err) => {
                Err(err)
            }
        }
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

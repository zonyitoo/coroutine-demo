
use std::ops::{Deref, DerefMut};
use std::io;
use std::net::SocketAddr;

use mio::Interest;
use mio::buf::{Buf, MutBuf};

use scheduler::Scheduler;

pub struct UdpSocket(::mio::udp::UdpSocket);

impl UdpSocket {
    /// Returns a new, unbound, non-blocking, IPv4 UDP socket
    pub fn v4() -> io::Result<UdpSocket> {
        Ok(UdpSocket(try!(::mio::udp::UdpSocket::v4())))
    }

    /// Returns a new, unbound, non-blocking, IPv6 UDP socket
    pub fn v6() -> io::Result<UdpSocket> {
        Ok(UdpSocket(try!(::mio::udp::UdpSocket::v6())))
    }

    pub fn bound(addr: &SocketAddr) -> io::Result<UdpSocket> {
        Ok(UdpSocket(try!(::mio::udp::UdpSocket::bound(addr))))
    }

    pub fn try_clone(&self) -> io::Result<UdpSocket> {
        Ok(UdpSocket(try!(self.0.try_clone())))
    }

    pub fn send_to<B: Buf>(&self, buf: &mut B, target: &SocketAddr) -> io::Result<Option<()>> {
        match try!(self.0.send_to(buf, target)) {
            None => {
                debug!("UdpSocket send_to WOULDBLOCK");
            },
            Some(..) => {
                return Ok(Some(()));
            }
        }

        try!(Scheduler::current().wait_event(&self.0, Interest::writable()));

        match try!(self.0.send_to(buf, target)) {
            None => {
                panic!("UdpSocket send_to WOULDBLOCK");
            },
            Some(..) => {
                return Ok(Some(()));
            }
        }
    }

    pub fn recv_from<B: MutBuf>(&self, buf: &mut B) -> io::Result<Option<SocketAddr>> {
        match try!(self.0.recv_from(buf)) {
            None => {
                debug!("UdpSocket recv_from WOULDBLOCK");
            },
            Some(addr) => {
                return Ok(Some(addr));
            }
        }

        try!(Scheduler::current().wait_event(&self.0, Interest::readable()));

        match try!(self.0.recv_from(buf)) {
            None => {
                panic!("UdpSocket recv_from WOULDBLOCK");
            },
            Some(addr) => {
                return Ok(Some(addr));
            }
        }
    }
}

impl Deref for UdpSocket {
    type Target = ::mio::udp::UdpSocket;

    fn deref(&self) -> &::mio::udp::UdpSocket {
        return &self.0
    }
}

impl DerefMut for UdpSocket {
    fn deref_mut(&mut self) -> &mut ::mio::udp::UdpSocket {
        return &mut self.0
    }
}

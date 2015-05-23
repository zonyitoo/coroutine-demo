
pub use self::tcp::{TcpListener, TcpStream, TcpSocket};
pub use self::udp::UdpSocket;

use std::io;
use std::net::{ToSocketAddrs, SocketAddr};

macro_rules! try_wouldblock(
    ($e:expr) => {{
        match $e {
            Ok(None) => (),
            Ok(Some(s)) => {
                return Ok(s);
            },
            Err(e) => {
                error!("try_nonblock: {}", e);
                return Err(e);
            }
        }
    }};
);

pub mod tcp;
pub mod udp;

fn each_addr<A: ToSocketAddrs, F, T>(addr: A, mut f: F) -> io::Result<T>
    where F: FnMut(&SocketAddr) -> io::Result<T>
{
    let mut last_err = None;
    for addr in try!(addr.to_socket_addrs()) {
        match f(&addr) {
            Ok(l) => return Ok(l),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput,
                       "could not resolve to any addresses")
    }))
}

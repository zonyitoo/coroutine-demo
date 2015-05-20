use std::net::IpAddr;
use std::io;

pub struct LookupHost;

pub fn lookup_host(host: &str) -> io::Result<LookupHost> {
    unimplemented!();
}

pub fn lookup_addr(addr: IpAddr) -> io::Result<String> {
    unimplemented!();
}

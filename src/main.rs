#![feature(scoped, libc)]

extern crate coroutine;
extern crate num_cpus;
extern crate deque;
#[macro_use] extern crate log;
extern crate env_logger;
extern crate mio;
extern crate libc;
extern crate clap;

use mio::Socket;

use clap::{Arg, App};

use scheduler::Scheduler;
use net::tcp::TcpListener;

mod scheduler;
mod net;

fn main() {
    env_logger::init().unwrap();

    let matches = App::new("coroutine-demo")
            .version(env!("CARGO_PKG_VERSION"))
            .author("Y. T. Chung <zonyitoo@gmail.com>")
            .arg(Arg::with_name("BIND").short("b").long("bind").takes_value(true).required(true)
                    .help("Listening on this address"))
            .arg(Arg::with_name("THREADS").short("t").long("threads").takes_value(true).required(true)
                    .help("Number of threads"))
            .get_matches();

    let bind_addr = matches.value_of("BIND").unwrap().to_owned();

    Scheduler::spawn(move|| {
        let server = TcpListener::bind(&bind_addr.parse().unwrap()).unwrap();
        server.set_reuseaddr(true).unwrap();
        server.set_reuseport(true).unwrap();

        info!("Listening on {:?}", server.local_addr().unwrap());

        loop {
            use std::io::{Read, Write};

            let mut stream = server.accept().unwrap();
            info!("Accept connection: {:?}", stream.peer_addr().unwrap());

            Scheduler::spawn(move|| {
                let mut buf = [0; 10240];

                loop {
                    debug!("Trying to Read...");
                    match stream.read(&mut buf) {
                        Ok(0) => {
                            debug!("EOF received, going to close");
                            break;
                        },
                        Ok(len) => {
                            info!("Read {} bytes, echo back!", len);
                            stream.write_all(&buf[0..len]).unwrap();
                        },
                        Err(err) => {
                            panic!("Error occurs: {:?}", err);
                        }
                    }
                }

                info!("{:?} closed", stream.peer_addr().unwrap());
            });
        }
    });

    Scheduler::run(matches.value_of("THREADS").unwrap().parse().unwrap());
}

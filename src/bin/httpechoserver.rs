extern crate clap;
#[macro_use] extern crate log;
extern crate env_logger;
extern crate mio;
extern crate hyper;

extern crate cosupport;

use std::net::SocketAddr;

use clap::{Arg, App};

use hyper::http::{parse_request, Incoming};
use hyper::buffer::BufReader;
use hyper::server::Response;
use hyper::header::Headers;

use cosupport::scheduler::Scheduler;
use cosupport::net::tcp::TcpSocket;

fn main() {
    env_logger::init().unwrap();

    let matches = App::new("coroutine-demo")
            .version(env!("CARGO_PKG_VERSION"))
            .author("Y. T. Chung <zonyitoo@gmail.com>")
            .arg(Arg::with_name("BIND").short("b").long("bind").takes_value(true).required(true)
                    .help("Listening on this address"))
            .arg(Arg::with_name("THREADS").short("t").long("threads").takes_value(true)
                    .help("Number of threads"))
            .get_matches();

    let bind_addr = matches.value_of("BIND").unwrap().to_owned();

    Scheduler::run(move|| {
        let addr = bind_addr.parse().unwrap();
        let server = match &addr {
            &SocketAddr::V4(..) => TcpSocket::v4(),
            &SocketAddr::V6(..) => TcpSocket::v6(),
        }.unwrap();
        server.set_reuseaddr(true).unwrap();
        // server.set_reuseport(true).unwrap();

        server.bind(&addr).unwrap();
        let server = server.listen(2048).unwrap();

        info!("Listening on {:?}", server.local_addr().unwrap());

        loop {
            let stream = server.accept().unwrap();
            let addr = stream.peer_addr().unwrap();
            info!("Accept connection: {:?}", addr);

            Scheduler::spawn(move|| {
                use std::io::Write;
                debug!("Begin handling {:?}", addr);

                // let mut writer = stream.try_clone().unwrap();
                let mut reader = BufReader::new(stream);

                let Incoming { version, subject: (method, uri), headers } =
                    match parse_request(&mut reader) {
                        Ok(com) => com,
                        Err(err) => {
                            // The cat shuts its eyes when stealing cream.
                            error!("Error occurs while parsing request header {:?}: {:?}", addr, err);
                            return;
                        }
                    };

                debug!("version {:?}, subject: ({:?}, {:?}), {:?}", version, method, uri, headers);

                let message = b"<html><body><h1>It works!</h1></body></html>\n";
                let mut headers = Headers::new();
                headers.set_raw("Content-Type", vec![b"text/html".to_vec()]);

                debug!("{:?} Headers {:?}", addr, headers);
                let mut wbuf = Vec::new();
                {
                    let response = Response::new(&mut wbuf, &mut headers);
                    response.send(message).unwrap_or_else(|err| {
                        panic!("Error occurs while sending to {:?}: {:?}", addr, err);
                    });
                }

                let mut writer = reader.into_inner();
                writer.write_all(&wbuf[..]).unwrap_or_else(|err| {
                    panic!("Error occurs while writing to {:?}: {:?}", addr, err);
                });
                info!("{:?} closed", addr);
            });
        }
    }, matches.value_of("THREADS").unwrap_or("1").parse().unwrap());
}

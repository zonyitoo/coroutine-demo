extern crate clap;
#[macro_use] extern crate log;
extern crate env_logger;
extern crate mio;
extern crate hyper;

extern crate cosupport;

use mio::Socket;

use clap::{Arg, App};

use hyper::http::{parse_request, Incoming};
use hyper::buffer::BufReader;
use hyper::server::Response;
use hyper::header::Headers;

use cosupport::scheduler::Scheduler;
use cosupport::net::tcp::TcpListener;

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
            let stream = server.accept().unwrap();
            info!("Accept connection: {:?}", stream.peer_addr().unwrap());

            Scheduler::spawn(move|| {
                let mut writer = stream.try_clone().unwrap();
                let mut reader = BufReader::new(stream);

                let Incoming { version, subject: (method, uri), headers } = parse_request(&mut reader).unwrap();

                debug!("version {:?}, subject: ({:?}, {:?}), {:?}", version, method, uri, headers);

                let message = b"Hello World";
                let mut headers = Headers::new();
                headers.set_raw("Content-Length", vec![message.len().to_string().as_bytes().to_vec()]);
                headers.set_raw("Content-Type", vec![b"text/html".to_vec()]);

                debug!("Headers {:?}", headers);

                {
                    let response = Response::new(&mut writer, &mut headers);
                    response.send(message).unwrap();
                }

                info!("{:?} closed", writer.peer_addr().unwrap());
            });
        }
    });

    Scheduler::run(matches.value_of("THREADS").unwrap().parse().unwrap());
}

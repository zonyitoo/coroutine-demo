
extern crate cosupport;
extern crate coroutine;
extern crate env_logger;
extern crate clap;

use std::thread;

use coroutine::coroutine::Coroutine;
use clap::{App, Arg};

use cosupport::scheduler::Scheduler;
use cosupport::processor::Processor;
use cosupport::eventloop::EventLoop;

fn main() {
    env_logger::init().unwrap();

    let matches = App::new("simple")
            .version(env!("CARGO_PKG_VERSION"))
            .author("Y. T. Chung <zonyitoo@gmail.com>")
            .arg(Arg::with_name("THREADS").short("t").long("threads").takes_value(true)
                    .help("Number of threads"))
            .get_matches();


    Scheduler::spawn(|| {
        let mut eventloop = EventLoop::new();
        loop {
            Processor::current().block(|| {
                println!("Blocking for eventloop {:?}", thread::current());
                eventloop.poll_once().unwrap();
                println!("Unblocking");
            });
            Coroutine::sched();
        }
    });

    Scheduler::spawn(|| {
        loop {
            println!("A {:?}", thread::current());
            Coroutine::sched();
        }
    });

    Scheduler::spawn(|| {
        loop {
            println!("B {:?}", thread::current());
            Coroutine::sched();
        }
    });

    Scheduler::run(matches.value_of("THREADS").unwrap_or("1").parse().unwrap());

}

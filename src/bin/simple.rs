
extern crate cosupport;
extern crate coroutine;
extern crate env_logger;
extern crate clap;

use std::thread;

use coroutine::coroutine::Coroutine;
use clap::{App, Arg};

use cosupport::scheduler::Scheduler;

fn main() {
    env_logger::init().unwrap();

    let matches = App::new("simple")
            .version(env!("CARGO_PKG_VERSION"))
            .author("Y. T. Chung <zonyitoo@gmail.com>")
            .arg(Arg::with_name("THREADS").short("t").long("threads").takes_value(true)
                    .help("Number of threads"))
            .arg(Arg::with_name("TOSLEEP").short("s").long("to-sleep").takes_value(false)
                    .help("Sleep the thread after printing"))
            .get_matches();


    const ALPHABETS: &'static str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";

    let to_sleep = match matches.value_of("TOSLEEP") {
        Some(..) => true,
        None => false,
    };

    for c in ALPHABETS.chars() {
        Scheduler::spawn(move|| {
            loop {
                Scheduler::spawn(move|| {
                    println!("Inside {} {:?}", c, thread::current());
                });
                println!("{} {:?}", c, thread::current());
                if to_sleep {
                    thread::sleep_ms(100);
                }
                Coroutine::sched();
            }
        });
    }

    Scheduler::run(matches.value_of("THREADS").unwrap_or("1").parse().unwrap());

}

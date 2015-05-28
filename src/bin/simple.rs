
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
            .get_matches();


    const ALPHABETS: &'static str = "ABCDEFGHIJKLMN";

    for c in ALPHABETS.chars() {
        Scheduler::spawn(move|| {
            loop {
                println!("{} {:?}", c, thread::current());
                thread::sleep_ms(100);
                Coroutine::sched();
            }
        });
    }

    Scheduler::run(matches.value_of("THREADS").unwrap_or("1").parse().unwrap());

}

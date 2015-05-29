extern crate cosupport;
extern crate env_logger;

use cosupport::scheduler::Scheduler;

fn main() {
    env_logger::init().unwrap();

    Scheduler::spawn(|| {
        println!("Running and going to exit");
    });

    Scheduler::run(2);
}

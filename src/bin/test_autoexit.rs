extern crate cosupport;

use cosupport::scheduler::Scheduler;

fn main() {
    Scheduler::run(|| {
        Scheduler::spawn(|| {
            println!("Fuck something ...");
        });

        println!("Running, going to exit now...");
    }, 10);
}

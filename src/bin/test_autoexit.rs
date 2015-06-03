extern crate cosupport;

use cosupport::scheduler::Scheduler;

fn main() {
    Scheduler::spawn(|| {
        Scheduler::spawn(|| {
            println!("Fuck something ...");
        });

        println!("Running, going to exit now...");
    });

    Scheduler::run(10);
}

extern crate cosupport;

use cosupport::scheduler::Scheduler;

fn main() {
    Scheduler::main(|| {
        Scheduler::spawn(|| {
            println!("Fuck something ...");
        });

        println!("Running, going to exit now...");
    }, 10);
}

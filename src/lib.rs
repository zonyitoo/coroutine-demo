#![feature(libc, std_misc)]

extern crate coroutine;
extern crate num_cpus;
extern crate deque;
#[macro_use] extern crate log;
extern crate env_logger;
extern crate mio;
extern crate libc;
extern crate rand;

pub mod scheduler;
pub mod net;

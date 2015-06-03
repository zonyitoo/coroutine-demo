// The MIT License (MIT)

// Copyright (c) 2015 Rustcc Developers

// Permission is hereby granted, free of charge, to any person obtaining a copy of
// this software and associated documentation files (the "Software"), to deal in
// the Software without restriction, including without limitation the rights to
// use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of
// the Software, and to permit persons to whom the Software is furnished to do so,
// subject to the following conditions:

// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.

// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
// FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR
// COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER
// IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
// CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

use std::thread;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, Arc};
use std::sync::mpsc::Sender;

use coroutine::spawn;
use coroutine::coroutine::{Handle, Coroutine};

use deque::Stealer;

use processor::Processor;

lazy_static! {
    static ref SCHEDULER: Scheduler = Scheduler::new();
}

pub enum SchedMessage {
    NewNeighbor((Sender<SchedMessage>, Stealer<Handle>)),
}

pub struct Scheduler {
    processors: Mutex<Vec<(Sender<SchedMessage>, Stealer<Handle>)>>,
    work_counts: AtomicUsize,
}

unsafe impl Send for Scheduler {}
unsafe impl Sync for Scheduler {}

impl Scheduler {
    pub fn new() -> Scheduler {
        Scheduler {
            processors: Mutex::new(Vec::new()),
            work_counts: AtomicUsize::new(0),
        }
    }

    pub fn get() -> &'static Scheduler {
        &SCHEDULER
    }

    pub fn processors(&self) -> &Mutex<Vec<(Sender<SchedMessage>, Stealer<Handle>)>> {
        &self.processors
    }

    pub fn finished(_: Handle) {
        Scheduler::get().work_counts.fetch_sub(1, Ordering::SeqCst);
    }

    pub fn work_count(&self) -> usize {
        Scheduler::get().work_counts.load(Ordering::SeqCst)
    }

    pub fn spawn<F>(f: F)
            where F: FnOnce() + 'static + Send {
        let coro = Coroutine::spawn(f);
        Processor::current().ready(coro);
        Scheduler::get().work_counts.fetch_add(1, Ordering::SeqCst);
        Coroutine::sched();
    }

    pub fn run(procs: usize) {
        let mut futs = Vec::new();
        for _ in 0..procs-1 {
            let fut = thread::spawn(|| {
                Processor::current().schedule().unwrap();
            });

            futs.push(fut);
        }

        Processor::current().schedule().unwrap();

        for fut in futs.into_iter() {
            fut.join().unwrap();
        }
    }
}


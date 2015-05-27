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
use std::sync::mpsc::{channel, Sender, Receiver, TryRecvError};
use std::sync::{Mutex, Once, ONCE_INIT};
use std::mem;
use std::cell::UnsafeCell;
use std::sync::atomic::{ATOMIC_BOOL_INIT, AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::collections::VecDeque;

use coroutine::spawn;
use coroutine::coroutine::{State, Handle, Coroutine, Options};

use deque::{BufferPool, Stealer, Worker, Stolen};

use processor::Processor;

lazy_static! {
    static ref SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());
}

struct Scheduler {
    global_queue: VecDeque<Handle>,

    // TODO: How to store processors??
    // std::sync::Weak?
}

impl Scheduler {
    fn new() -> Scheduler {
        Scheduler {
            global_queue: VecDeque::new(),
        }
    }

    pub fn get() -> &'static Mutex<Scheduler> {
        &mut SCHEDULER
    }

    pub fn ready(hdl: Handle) {
        let scheduler = match Scheduler::get().lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                warn!("Scheduler mutex is poisoned; ignoring");
                poisoned.into_inner()
            }
        };

        // TODO: Determine whether we should create a new processor
        // If the total number of processors has not exceeded MAX_PROC, then we may create a new processor.
        // One possible strategy is to decide a `work_load`, try to create a new processor
        // if `work_load` have exceeded 30%.
        //
        // But, should we kill some of the starving processors?

        // 1. Try to find a starved processor

        // 2. Try to run it by myself
        let hdl = match Processor::current().feed_one(hdl) {
            Some(hdl) => {
                debug!("Current processor refused the Coroutine");
                hdl
            },
            None => {
                // Ok!
                return;
            }
        };

        // 3. Push into global queue
        scheduler.global_queue.push_back(hdl);
    }
}

impl Scheduler {
    pub fn processor_exit(processor: &mut Processor) {
        // TODO: Processor exited, cleanup
    }

    pub fn processor_starving(processor: &mut Processor) {
        let scheduler = match Scheduler::get().lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                warn!("Scheduler mutex is poisoned; ignoring");
                poisoned.into_inner()
            }
        };

        // 1. Feed him with global queue
        if !scheduler.global_queue.is_empty() {
            let ret = processor.feed(scheduler.global_queue.into_iter());
            assert!(ret);
            return;
        }

        // 2. Steal some for him
        // Steal works from the most busy processors

        // 3. Exile
        // Record it as a starved processor or exit
    }

    pub fn processor_create() {
        // TODO: If the number of current working processors has not exceeded MAX_PROC
        //       Then we could create a new processor
    }

    pub fn processor_blocked(processor: &mut Processor) {
        // TODO: Takes all works from the processor
        //       Record it as a blocked processor
    }

    pub fn processor_unblocked(processor: &mut Processor) {
        // TODO: Remove it from the blocked processors
    }
}


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

        // 1. Try to find a starved processor

        // 2. Try to run it by my self
        let hdl = match Processor::current().feed_one(hdl) {
            Some(hdl) => {
                debug!("Current processor refused the Coroutine");
                hdl
            },
            None => {
                return;
            }
        };

        // 3. Push into global queue
        scheduler.global_queue.push_back(hdl);
    }
}

impl Scheduler {
    pub fn processor_exit() {

    }

    pub fn processor_starving() {
        let scheduler = match Scheduler::get().lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                warn!("Scheduler mutex is poisoned; ignoring");
                poisoned.into_inner()
            }
        };

        // 1. Feed him with global queue
        if !scheduler.global_queue.is_empty() {

        }

        // 2. Exile
    }

    pub fn processor_blocked() {

    }

    pub fn processor_unblocked() {

    }
}


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
use std::sync::{Arc, Weak};
use std::collections::{HashMap, VecDeque};

use coroutine::spawn;
use coroutine::coroutine::{State, Handle, Coroutine, Options};

use deque::{BufferPool, Stealer, Worker, Stolen};

use uuid::Uuid;

use processor::{WorkDeque, Processor};

lazy_static! {
    static ref SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());
}

pub const BUSY_THRESHOLD: usize = 10;

pub struct Scheduler {
    global_queue: VecDeque<Handle>,

    // TODO: How to store processors??
    // std::sync::Weak?
    starved_processors: VecDeque<(Uuid, Weak<WorkDeque<Handle>>)>,
    working_processors: HashMap<Uuid, Weak<WorkDeque<Handle>>>,
    blocked_processors: HashMap<Uuid, Weak<WorkDeque<Handle>>>,
}

impl Scheduler {
    fn new() -> Scheduler {
        Scheduler {
            global_queue: VecDeque::new(),

            starved_processors: VecDeque::new(),
            working_processors: HashMap::new(),
            blocked_processors: HashMap::new(),
        }
    }

    pub fn resume(&mut self, hdl: Handle, to_myself: bool) {
        // TODO: Determine whether we should create a new processor
        // If the total number of processors has not exceeded MAX_PROC, then we may create a new processor.
        // One possible strategy is to decide a `work_load`, try to create a new processor
        // if `work_load` have exceeded 30%.
        //
        // But, should we kill some of the starving processors?

        // 1. Try to find a starved processor
        loop {
            match self.starved_processors.pop_front() {
                None => break,
                Some((uuid, weakp)) => {
                    match weakp.upgrade() {
                        Some(p) => {
                            debug!("Gave it to a starved processor");
                            p.push_back(hdl);
                            self.working_processors.insert(uuid, weakp);
                            return;
                        },
                        None => {}
                    }
                }
            }
        }

        // 2. Try to run it by myself
        let hdl = if to_myself {
             match Processor::current().feed_one(hdl) {
                Some(hdl) => {
                    debug!("Current processor refused the Coroutine");
                    hdl
                },
                None => {
                    // Ok!
                    debug!("Run it by myself!");
                    return;
                }
            }
        } else {
            hdl
        };

        // 3. Push into global queue
        debug!("Push into global queue");
        self.global_queue.push_back(hdl);
    }

    pub fn get() -> &'static Mutex<Scheduler> {
        &SCHEDULER
    }

    pub fn ready(hdl: Handle) {
        let mut scheduler = Scheduler::get().lock().unwrap();
        scheduler.resume(hdl, true);
    }

    pub fn give(hdl: Handle) {
        let mut scheduler = Scheduler::get().lock().unwrap();
        scheduler.resume(hdl, false);
    }

    pub fn spawn<F>(f: F)
            where F: FnOnce() + 'static + Send {
        let coro = Coroutine::spawn(f);
        Scheduler::ready(coro);
        Coroutine::sched();
    }

    pub fn spawn_opts<F>(f: F, opts: Options)
            where F: FnOnce() + 'static + Send {
        let coro = Coroutine::spawn_opts(f, opts);
        Scheduler::ready(coro);
        Coroutine::sched();
    }
}

impl Scheduler {
    pub fn processor_exit(processor: &Processor) {
        // TODO: Processor exited, cleanup
        let mut scheduler = Scheduler::get().lock().unwrap();

        if scheduler.working_processors.remove(processor.id()).is_some() {
            return;
        }

        if scheduler.blocked_processors.remove(processor.id()).is_some() {
            return;
        }

        let mut idx = None;
        for (i, &(id, _)) in scheduler.starved_processors.iter().enumerate() {
            if &id == processor.id() {
                idx = Some(i);
                break;
            }
        }

        idx.map(|i| scheduler.starved_processors.swap_back_remove(i));
    }

    pub fn processor_starving(processor: &Processor) {
        let mut scheduler = Scheduler::get().lock().unwrap();

        // 1. Feed him with global queue
        if !scheduler.global_queue.is_empty() {
            debug!("Feed {} with global queue; count={}", processor.id(), scheduler.global_queue.len());
            let ret = processor.feed(scheduler.global_queue.drain());
            assert!(ret);
            return;
        }

        // 2. Steal some for him
        // Steal works from the most busy processors
        {
            let busy_proc = scheduler.working_processors.iter()
                    .filter_map(|(id, weakp)| {
                        match weakp.upgrade() {
                            Some(p) => {
                                let len = p.len();
                                Some((id, p, len))
                            },
                            None => None,
                        }
                    })
                    .max_by(|&(_, _, len)| len);

            match busy_proc {
                Some((_, p, _)) => {
                    let mut queue = p.work_queue().lock().unwrap();
                    let steal_length = queue.len() / 2;
                    processor.feed(queue.drain().take(steal_length));
                    return;
                },
                None => {}
            }
        }

        // 3. Exile
        // Record it as a starved processor or exit
        {
            let procid = processor.id().clone();
            scheduler.working_processors.remove(&procid);
            scheduler.starved_processors.push_back((procid, processor.work_queue().downgrade()));
            debug!("Processor {} exiled", procid);
        }
    }

    pub fn processor_create(processor: &Processor) {
        // TODO: If the number of current working processors has not exceeded MAX_PROC
        //       Then we could create a new processor
        let mut scheduler = Scheduler::get().lock().unwrap();
        scheduler.working_processors.insert(processor.id().clone(), processor.work_queue().downgrade());
    }

    pub fn processor_blocked(processor: &Processor) {
        // TODO: Takes all works from the processor
        //       Record it as a blocked processor
        let mut scheduler = Scheduler::get().lock().unwrap();
        scheduler.working_processors.remove(processor.id());
        scheduler.blocked_processors.insert(processor.id().clone(), processor.work_queue().downgrade());

        let mut queue = processor.work_queue().work_queue().lock().unwrap();
        scheduler.global_queue.extend(queue.drain());
    }

    pub fn processor_unblocked(processor: &mut Processor) {
        // TODO: Remove it from the blocked processors
        let mut scheduler = Scheduler::get().lock().unwrap();
        scheduler.blocked_processors.remove(processor.id());
        scheduler.working_processors.insert(processor.id().clone(), processor.work_queue().downgrade());
    }
}

impl Scheduler {
    pub fn run(threads: usize) {
        let mut futs = Vec::new();
        for n in 0..threads {
            let guard = thread::Builder::new().name(format!("Worker thread #{}", n)).scoped(move|| {
                Processor::current().run();
            }).unwrap();
            futs.push(guard);
        }

        for fut in futs {
            fut.join();
        }
    }
}


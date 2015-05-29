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
use std::sync::{Arc, Mutex, Weak, Barrier};
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::Sender;
use std::os::unix::io::{AsRawFd, RawFd};

use coroutine::spawn;
use coroutine::coroutine::{Handle, Coroutine, Options};

use uuid::Uuid;

use mio::Interest;

use processor::{WorkDeque, Processor};
use eventloop::EventLoop;

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

    event_loop: Vec<Sender<(RawFd, Interest, Handle)>>,
    cur_loop_idx: usize,
}

impl Scheduler {
    fn new() -> Scheduler {
        Scheduler {
            global_queue: VecDeque::new(),

            starved_processors: VecDeque::new(),
            working_processors: HashMap::new(),
            blocked_processors: HashMap::new(),

            event_loop: Vec::new(),
            cur_loop_idx: 0,
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

    pub fn processor_starving(&mut self, processor: &mut Processor) {
        // let mut scheduler = Scheduler::get().lock().unwrap();
        debug!("Processor {} is starving; thread {:?}", processor.id(), thread::current());

        // 1. Feed him with global queue
        debug!("Trying to feed {} with global queue", processor.id());
        if !self.global_queue.is_empty() {
            debug!("Feed {} with global queue; count={}", processor.id(), self.global_queue.len());
            let ret = processor.feed(self.global_queue.drain());
            assert!(ret);
            return;
        }

        if self.working_processors.len() + self.blocked_processors.len() <= 1 {
            debug!("No remaining Coroutines in the scheduler, ask all workers to stop");

            processor.stop();

            for (_, p) in self.starved_processors.drain() {
                match p.upgrade() {
                    Some(p) => {
                        p.stop();
                    },
                    None => {}
                }
            }

            return;
        }

        // 2. Steal some for him
        // Steal works from the most busy processors
        debug!("Trying to steal for {}", processor.id());
        {
            let busy_proc = self.working_processors.iter()
                    .filter_map(|(id, weakp)| {
                        if id == processor.id() {
                            return None;
                        }
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
                Some((id, p, len)) => {
                    debug!("May steal {} works from {} for {}", len, id, processor.id());
                    if len >= BUSY_THRESHOLD {
                        let mut queue = p.work_queue().lock().unwrap();
                        let steal_length = queue.len() / 2;
                        if steal_length != 0 {
                            debug!("Steal {} works from {} for {}", steal_length, id, processor.id());
                            // TODO: Bug! Using drain will actually throwing all items in the queue
                            // processor.feed(queue.drain().take(steal_length));

                            // FIXME: Do not need to re-allocate a new VecDeque
                            processor.work_queue()
                                .append(&mut queue.split_off(steal_length));
                            return;
                        }
                    }
                },
                None => {}
            }
        }

        // 3. Exile
        // Record it as a starved processor or exit
        debug!("Processor {} is going to be exiled", processor.id());
        {
            let procid = processor.id().clone();
            self.working_processors.remove(&procid);
            self.starved_processors.push_back((procid, processor.work_queue().downgrade()));
            debug!("Processor {} exiled", procid);
        }
    }

    pub fn processor_create(&mut self, processor: &Processor) {
        // TODO: If the number of current working processors has not exceeded MAX_PROC
        //       Then we could create a new processor
        // let mut scheduler = Scheduler::get().lock().unwrap();
        self.working_processors.insert(processor.id().clone(), processor.work_queue().downgrade());
    }

    pub fn processor_blocked(&mut self, processor: &Processor) {
        // TODO: Takes all works from the processor
        //       Record it as a blocked processor
        // let mut self = Scheduler::get().lock().unwrap();
        self.working_processors.remove(processor.id());
        self.blocked_processors.insert(processor.id().clone(), processor.work_queue().downgrade());

        let mut queue = processor.work_queue().work_queue().lock().unwrap();

        // Try to find a starved processor
        loop {
            match self.starved_processors.pop_front() {
                None => break,
                Some((uuid, weakp)) => {
                    match weakp.upgrade() {
                        Some(p) => {
                            debug!("Gave it to a starved processor");
                            p.work_queue().lock().unwrap().extend(queue.drain());
                            self.working_processors.insert(uuid, weakp);
                            return;
                        },
                        None => {}
                    }
                }
            }
        }

        // Give them to the global queue
        self.global_queue.extend(queue.drain());
    }

    pub fn processor_unblocked(&mut self, processor: &Processor) {
        // TODO: Remove it from the blocked processors
        // let mut scheduler = Scheduler::get().lock().unwrap();
        self.blocked_processors.remove(processor.id());
        self.working_processors.insert(processor.id().clone(), processor.work_queue().downgrade());

        debug!("Trying to feed {} with global queue", processor.id());
        if !self.global_queue.is_empty() {
            debug!("Feed {} with global queue; count={}", processor.id(), self.global_queue.len());
            let ret = processor.feed(self.global_queue.drain());
            assert!(ret);
        }
    }
}

impl Scheduler {
    pub fn run(threads: usize) {
        let mut futs = Vec::new();
        let barrier = Arc::new(Barrier::new(threads + 1));
        for n in 0..threads {
            let barrier = barrier.clone();
            let guard = thread::Builder::new().name(format!("Worker thread #{}", n)).spawn(move|| {
                {
                    let mut sched = Scheduler::get().lock().unwrap();
                    sched.processor_create(Processor::current());
                }
                barrier.wait();
                Processor::current().run();
            }).unwrap();
            futs.push(guard);
        }
        barrier.wait();

        // Start eventloops
        {
            let mut eventloop = {
                let mut sched = Scheduler::get().lock().unwrap();
                let eventloop = EventLoop::new();
                sched.event_loop.push(eventloop.event_sender());
                eventloop
            };
            Scheduler::spawn(move|| {
                loop {
                    debug!("Eventloop registering events");
                    eventloop.register_events().unwrap();
                    debug!("Eventloop has {} waiting", eventloop.waiting_count());
                    Processor::current().block(|| {
                        debug!("Eventloop polling");
                        eventloop.poll_once().unwrap();
                    });
                    Coroutine::sched();
                }
            });
        }

        for fut in futs {
            fut.join().unwrap();
        }
    }

    pub fn register_event<F: AsRawFd>(&mut self, fd: &F, inst: Interest, hdl: Handle) {
        debug!("Registering event {:?} to idx {}; total eventloop: {}",
               inst, self.cur_loop_idx, self.event_loop.len());
        self.event_loop[self.cur_loop_idx].send((fd.as_raw_fd(), inst, hdl)).unwrap();
        self.cur_loop_idx = (self.cur_loop_idx + 1) % self.event_loop.len();
    }
}


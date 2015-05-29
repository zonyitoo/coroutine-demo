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

use std::collections::VecDeque;
use std::cell::{UnsafeCell, RefCell};
use std::io;
#[cfg(target_os = "linux")]
use std::os::unix::io::AsRawFd;
#[cfg(target_os = "linux")]
use std::convert::From;
use std::sync::{Arc, Weak, Mutex, Condvar};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::iter::{IntoIterator, DoubleEndedIterator, ExactSizeIterator, Extend};

use coroutine::coroutine::{Coroutine, Handle, State};

use uuid::Uuid;

use scheduler::{self, Scheduler};

thread_local!(static PROCESSOR: Arc<UnsafeCell<Processor>> = Arc::new(UnsafeCell::new(Processor::new())));

const WORKQUEUE_DEFAULT_CAPACITY: usize = 1024;

pub struct WorkDeque<T> {
    work_queue: Mutex<VecDeque<T>>,
    condvar: Condvar,
    is_stopped: AtomicBool,
}

impl<T> WorkDeque<T> {
    pub fn new() -> WorkDeque<T> {
        WorkDeque::with_capacity(WORKQUEUE_DEFAULT_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> WorkDeque<T> {
        WorkDeque {
            work_queue: Mutex::new(VecDeque::with_capacity(capacity)),
            condvar: Condvar::new(),
            is_stopped: AtomicBool::new(false),
        }
    }

    pub fn len(&self) -> usize {
        let guard = self.work_queue.lock().unwrap();
        guard.len()
    }

    pub fn is_empty(&self) -> bool {
        let guard = self.work_queue.lock().unwrap();
        guard.is_empty()
    }

    pub fn push_back(&self, data: T) {
        {
            let mut guard = self.work_queue.lock().unwrap();
            guard.push_back(data);
        }
        self.condvar.notify_one();
    }

    pub fn push_front(&self, data: T) {
        {
            let mut guard = self.work_queue.lock().unwrap();
            guard.push_front(data);
        }
        self.condvar.notify_one();
    }

    // pub fn pop_back(&self) -> Option<T> {
    //     let mut guard = self.work_queue.lock().unwrap();
    //     while !self.is_stopped && guard.is_empty() {
    //         guard = self.condvar.wait(guard).unwrap();
    //     }
    //     guard.pop_back()
    // }

    pub fn pop_front(&self) -> Option<T> {
        let mut guard = self.work_queue.lock().unwrap();
        while !self.is_stopped.load(Ordering::SeqCst) && guard.is_empty() {
            guard = self.condvar.wait(guard).unwrap();
            // let (g, _) = self.condvar.wait_timeout_ms(guard, 1000).unwrap();
            // guard = g;
        }
        guard.pop_front()
    }

    pub fn pop_front_timeout_ms(&self, ms: u32) -> Option<T> {
        let mut guard = self.work_queue.lock().unwrap();
        while !self.is_stopped.load(Ordering::SeqCst) && guard.is_empty() {
            // guard = self.condvar.wait(guard).unwrap();
            let (g, _) = self.condvar.wait_timeout_ms(guard, ms).unwrap();
            guard = g;
        }
        guard.pop_front()
    }

    pub fn append(&self, other: &mut VecDeque<T>) {
        let mut guard = self.work_queue.lock().unwrap();
        guard.append(other);
        self.condvar.notify_one();
    }

    pub fn work_queue(&self) -> &Mutex<VecDeque<T>> {
        &self.work_queue
    }

    pub fn stop(&self) {
        self.is_stopped.store(true, Ordering::SeqCst);
        self.condvar.notify_all();
    }
}

// unsafe impl<T> Send for WorkDeque<T> {}
// unsafe impl<T> Sync for WorkDeque<T> {}

pub struct Processor {
    id: Uuid,
    work_queue: Arc<WorkDeque<Handle>>,
    is_blocked: bool,
    is_stopped: bool,
}

impl Processor {
    fn new() -> Processor {
        Processor {
            id: Uuid::new_v4(),
            work_queue: Arc::new(WorkDeque::new()),
            is_blocked: false,
            is_stopped: true,
        }
    }

    pub fn id(&self) -> &Uuid {
        &self.id
    }

    pub fn current() -> &'static mut Processor {
        PROCESSOR.with(|p| unsafe {
            &mut *p.get()
        })
    }

    pub fn feed_one(&self, hdl: Handle) -> Option<Handle> {
        if self.is_blocked || self.is_stopped {
            return Some(hdl);
        }

        self.work_queue.push_back(hdl);
        None
    }

    pub fn feed<I>(&self, iter: I) -> bool
            where I: Iterator<Item=Handle> {
        if self.is_blocked || self.is_stopped {
            return false;
        }

        self.work_queue.work_queue().lock().unwrap()
            .extend(iter);
        true
    }

    pub fn work_queue(&self) -> &Arc<WorkDeque<Handle>> {
        &self.work_queue
    }

    pub fn stop(&mut self) {
        self.work_queue.is_stopped.store(true, Ordering::SeqCst);
        self.is_stopped = true;
        self.work_queue.condvar.notify_all();
    }

    pub fn run(&mut self) {
        // struct RunGuard<'a>(&'a mut Processor);

        // impl<'a> Drop for RunGuard<'a> {
        //     fn drop(&mut self) {
        //         self.0.is_stopped = true;
        //     }
        // }

        self.is_stopped = false;
        // let _guard = RunGuard(self);

        while !self.work_queue.is_stopped.load(Ordering::SeqCst) {
            let is_starving = self.work_queue.is_empty();

            if is_starving {
                // Ask scheduler for more works
                let mut sched = Scheduler::get().lock().unwrap();
                sched.processor_starving(self);
            }

            if self.work_queue.is_stopped.load(Ordering::SeqCst) {
                break;
            }

            while !self.work_queue.is_stopped.load(Ordering::SeqCst) {
                match self.work_queue.pop_front_timeout_ms(1000) {
                    None => {
                        // There must someone wake me up to stop!!
                        info!("Processor may going to stop");
                    },
                    Some(coro) => {
                        if let Err(err) = coro.resume() {
                            let msg = match err.downcast_ref::<&'static str>() {
                                Some(s) => *s,
                                None => match err.downcast_ref::<String>() {
                                    Some(s) => &s[..],
                                    None => "Box<Any>",
                                }
                            };
                            error!("Coroutine resume failed because of {:?}", msg);
                        }

                        match coro.state() {
                            State::Suspended => {
                                let mut queue = self.work_queue.work_queue().lock().unwrap();
                                if queue.len() >= scheduler::BUSY_THRESHOLD {
                                    Scheduler::give(coro);
                                } else {
                                    queue.push_back(coro);
                                }
                            },
                            _ => {
                                debug!("Coroutine state is {:?}, will not be reschedured automatically", coro.state());
                            }
                        }
                        break;
                    }
                }
            }
        }
        debug!("Processor {} exited", self.id());
    }
}

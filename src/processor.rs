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
use std::cell::UnsafeCell;
use std::io;
#[cfg(target_os = "linux")]
use std::os::unix::io::AsRawFd;
#[cfg(target_os = "linux")]
use std::convert::From;
use std::sync::{Mutex, Condvar};
use std::thread;
use std::iter::Extend;

use coroutine::coroutine::{Coroutine, Handle, State};

use mio::{EventLoop, Evented, Handler, Token, ReadHint, Interest, PollOpt};
use mio::util::Slab;
#[cfg(target_os = "linux")]
use mio::Io;

thread_local!(static PROCESSOR: UnsafeCell<Processor> = UnsafeCell::new(Processor::new()));

pub struct Processor {
    work_queue: Mutex<VecDeque<Handle>>,
    work_queue_condvar: Condvar,
    is_blocked: bool,
    is_stopped: bool,
}

impl Processor {
    fn new() -> Processor {
        Processor {
            work_queue: Mutex::new(VecDeque::with_capacity(1024)),
            work_queue_condvar: Condvar::new(),
            is_blocked: false,
            is_stopped: true,
        }
    }

    pub fn current() -> &'static mut Processor {
        PROCESSOR.with(|p| {
            &mut *p.get()
        })
    }

    pub fn ready(hdl: Handle) {

    }

    pub fn feed_one(&mut self, hdl: Handle) -> Option<Handle> {
        if self.is_blocked || self.is_stopped {
            return Some(hdl);
        }

        let guard = match self.work_queue.lock() {
            Ok(g) => g,
            Err(poisoned) => {
                warn!("Work queue mutex is poisoned; thread {:?}", thread::current());
                poisoned.into_inner()
            }
        };
        guard.push_back(hdl);

        self.work_queue_condvar.notify_one();
        None
    }

    pub fn feed<I>(&mut self, mut iter: I) -> bool
            where I: IntoIterator<Item=Handle> {
        if self.is_blocked || self.is_stopped {
            return false;
        }

        let guard = match self.work_queue.lock() {
            Ok(g) => g,
            Err(poisoned) => {
                warn!("Work queue mutex is poisoned; thread {:?}", thread::current());
                poisoned.into_inner()
            }
        };
        guard.extend(iter);

        self.work_queue_condvar.notify_one();
        true
    }

    pub fn work_count(&self) -> usize {
        let guard = match self.work_queue.lock() {
            Ok(g) => g,
            Err(poisoned) => {
                warn!("Work queue mutex is poisoned; thread {:?}", thread::current());
                poisoned.into_inner()
            }
        };
        guard.len()
    }

    pub fn run(&mut self) {
        struct RunGuard<'a>(&'a mut Processor);

        impl<'a> Drop for RunGuard<'a> {
            fn drop(&mut self) {
                self.0.is_stopped = true;
            }
        }

        self.is_stopped = false;
        let _guard = RunGuard(self);

        loop {
            let is_starving = {
                let guard = match self.work_queue.lock() {
                    Ok(g) => g,
                    Err(poisoned) => {
                        warn!("Work queue mutex is poisoned; thread {:?}", thread::current());
                        poisoned.into_inner()
                    }
                };
                guard.is_empty()
            };

            if is_starving {
                // Ask scheduler for more works
            }

            let coro = {
                let mut guard = match self.work_queue.lock() {
                    Ok(g) => g,
                    Err(poisoned) => {
                        warn!("Work queue mutex is poisoned; thread {:?}", thread::current());
                        poisoned.into_inner()
                    }
                };

                while guard.is_empty() {
                    guard = match self.work_queue_condvar.wait(guard) {
                        Ok(g) => g,
                        Err(poisoned) => {
                            warn!("Work queue condvar is poisoned; thread {:?}", thread::current());
                            poisoned.into_inner()
                        }
                    };
                }

                match guard.pop_front() {
                    None => {
                        // There must someone wake me up to stop!!
                        info!("Processor is going to stop");
                        return;
                    },
                    Some(coro) => coro
                }
            };

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
                    assert!(self.feed_one(coro).is_none());
                },
                _ => {
                    debug!("Coroutine state is {:?}, will not be reschedured automatically", coro.state());
                }
            }
        }
    }
}




const MAX_TOKEN_NUM: usize = 102400;
impl IoHandler {
    fn new() -> IoHandler {
        IoHandler {
            // slabs: Slab::new_starting_at(Token(1), MAX_TOKEN_NUM),
            slabs: Slab::new(MAX_TOKEN_NUM),
        }
    }
}

#[cfg(any(target_os = "linux",
          target_os = "android"))]
impl Processor {
    pub fn wait_event<E: Evented + AsRawFd>(&mut self, fd: &E, interest: Interest) -> io::Result<()> {
        let token = self.handler.slabs.insert((Coroutine::current(), From::from(fd.as_raw_fd()))).unwrap();
        try!(self.event_loop.register_opt(fd, token, interest,
                                          PollOpt::level()|PollOpt::oneshot()));

        debug!("wait_event: Blocked current Coroutine ...; token={:?}", token);
        Coroutine::block();
        debug!("wait_event: Waked up; token={:?}", token);

        Ok(())
    }
}

#[cfg(any(target_os = "linux",
          target_os = "android"))]
struct IoHandler {
    slabs: Slab<(Handle, Io)>,
}

#[cfg(any(target_os = "linux",
          target_os = "android"))]
impl Handler for IoHandler {
    type Timeout = ();
    type Message = ();

    fn writable(&mut self, event_loop: &mut EventLoop<Self>, token: Token) {

        debug!("In writable, token {:?}", token);

        match self.slabs.remove(token) {
            Some((hdl, fd)) => {
                // Linux EPoll needs to explicit EPOLL_CTL_DEL the fd
                event_loop.deregister(&fd).unwrap();
                mem::forget(fd);
                Scheduler::current().ready(hdl);
            },
            None => {
                warn!("No coroutine is waiting on writable {:?}", token);
            }
        }

    }

    fn readable(&mut self, event_loop: &mut EventLoop<Self>, token: Token, hint: ReadHint) {

        debug!("In readable, token {:?}, hint {:?}", token, hint);

        match self.slabs.remove(token) {
            Some((hdl, fd)) => {
                // Linux EPoll needs to explicit EPOLL_CTL_DEL the fd
                event_loop.deregister(&fd).unwrap();
                mem::forget(fd);
                Scheduler::current().ready(hdl);
            },
            None => {
                warn!("No coroutine is waiting on readable {:?}", token);
            }
        }

    }
}

#[cfg(any(target_os = "macos",
          target_os = "freebsd",
          target_os = "dragonfly",
          target_os = "ios",
          target_os = "bitrig",
          target_os = "openbsd"))]
impl Processor {
    pub fn wait_event<E: Evented>(&mut self, fd: &E, interest: Interest) -> io::Result<()> {
        // let token = self.io_handler.slabs.insert(Coroutine::current()).unwrap();
        // try!(self.event_loop.register_opt(fd, token, interest,
        //                                  PollOpt::level()|PollOpt::oneshot()));

        // debug!("wait_event: Blocked current Coroutine ...; token={:?}", token);
        // Coroutine::block();
        // debug!("wait_event: Waked up; token={:?}", token);

        Ok(())
    }
}

#[cfg(any(target_os = "macos",
          target_os = "freebsd",
          target_os = "dragonfly",
          target_os = "ios",
          target_os = "bitrig",
          target_os = "openbsd"))]
struct IoHandler {
    slabs: Slab<Handle>,
}

#[cfg(any(target_os = "macos",
          target_os = "freebsd",
          target_os = "dragonfly",
          target_os = "ios",
          target_os = "bitrig",
          target_os = "openbsd"))]
impl Handler for IoHandler {
    type Timeout = ();
    type Message = ();

    fn writable(&mut self, _: &mut EventLoop<Self>, token: Token) {

        debug!("In writable, token {:?}", token);

        match self.slabs.remove(token) {
            Some(hdl) => {
                // Scheduler::current().ready(hdl);
                // Scheduler::ready(hdl);
                Processor::ready(hdl);
            },
            None => {
                warn!("No coroutine is waiting on writable {:?}", token);
            }
        }

    }

    fn readable(&mut self, _: &mut EventLoop<Self>, token: Token, hint: ReadHint) {

        debug!("In readable, token {:?}, hint {:?}", token, hint);

        match self.slabs.remove(token) {
            Some(hdl) => {
                // Scheduler::current().ready(hdl);
                // Scheduler::ready(hdl);
                Processor::ready(hdl);
            },
            None => {
                warn!("No coroutine is waiting on readable {:?}", token);
            }
        }

    }
}



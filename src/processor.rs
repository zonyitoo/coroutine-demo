use std::cell::UnsafeCell;
use std::io;
#[cfg(target_os = "linux")]
use std::os::unix::io::AsRawFd;
#[cfg(target_os = "linux")]
use std::convert::From;
use std::sync::mpsc::{channel, Sender, Receiver, TryRecvError};

use coroutine::{State, Handle, Coroutine};

use mio::{EventLoop, Evented, Handler, Token, ReadHint, Interest, PollOpt};
use mio::util::Slab;
#[cfg(target_os = "linux")]
use mio::Io;

use deque::{BufferPool, Worker, Stealer, Stolen};

use rand::random;

use scheduler::{Scheduler, SchedMessage};

thread_local!(static PROCESSOR: UnsafeCell<Processor> = UnsafeCell::new(Processor::new()));

pub struct Processor {
    event_loop: EventLoop<IoHandler>,
    queue_worker: Worker<Handle>,
    queue_stealer: Stealer<Handle>,
    neighbors: Vec<(Sender<SchedMessage>, Stealer<Handle>)>,
    message_receiver: Receiver<SchedMessage>,
    handler: IoHandler,
    steal_buffer: Vec<Handle>,
}

impl Processor {
    pub fn new() -> Processor {
        let pool = BufferPool::new();
        let (w, s) = pool.deque();

        let (tx, rx) = channel();

        let neighbors = {
            let mut neigh = Scheduler::get().processors().lock().unwrap();

            for &(ref ntx, _) in neigh.iter() {
                let _ = ntx.send(SchedMessage::NewNeighbor((tx.clone(), s.clone())));
            }

            let cloned = neigh.clone();
            neigh.push((tx, s.clone()));
            cloned
        };

        Processor {
            event_loop: EventLoop::new().unwrap(),
            queue_worker: w,
            queue_stealer: s,
            neighbors: neighbors,
            message_receiver: rx,
            handler: IoHandler::new(),
            steal_buffer: Vec::new(),
        }
    }

    pub fn ready(&self, hdl: Handle) {
        self.queue_worker.push(hdl)
    }

    pub fn current() -> &'static mut Processor {
        PROCESSOR.with(|p| unsafe { &mut *p.get() })
    }

    pub fn schedule(&mut self) -> io::Result<()> {
        loop {
            match self.message_receiver.try_recv() {
                Ok(SchedMessage::NewNeighbor(handle)) => {
                    self.neighbors.push(handle);
                },
                Err(TryRecvError::Empty) => {},
                Err(err) => {
                    panic!("Failed to receive sched messages {:?}", err);
                }
            }

            if self.handler.slabs.count() != 0 {
                try!(self.event_loop.run_once(&mut self.handler));
            }

            loop {
                match self.queue_stealer.steal() {
                    Stolen::Data(hdl) => {
                        match hdl.resume() {
                            Ok(State::Suspended) => {
                                Processor::current().ready(hdl);
                            },
                            Ok(State::Finished) | Ok(State::Panicked) => {
                                Scheduler::finished(hdl);
                            },
                            Ok(State::Blocked) => (),
                            Ok(..) => unreachable!(),
                            Err(err) => {
                                error!("Coroutine resume error {:?}", err);
                            }
                        }
                        // break;
                    },
                    Stolen::Abort => {},
                    Stolen::Empty => {
                        break;
                    }
                }
            }

            if self.handler.slabs.count() != 0 {
                continue;
            }

            if !self.neighbors.is_empty() {
                let rand_idx = random::<usize>() % self.neighbors.len();
                // match self.neighbors[rand_idx].1.steal_half(&mut self.steal_buffer) {
                //     Some(n) => {
                //         debug!("Stolen {} coroutines", n);
                //         self.queue_worker.push_all(&mut self.steal_buffer);
                //         continue;
                //     },
                //     None => {}
                // }
                match self.neighbors[rand_idx].1.steal() {
                    Stolen::Data(hdl) => {
                        match hdl.resume() {
                            Ok(State::Suspended) => {
                                Processor::current().ready(hdl);
                            },
                            Ok(State::Finished) | Ok(State::Panicked) => {
                                Scheduler::finished(hdl);
                            },
                            Ok(State::Blocked) => (),
                            Ok(..) => unreachable!(),
                            Err(err) => {
                                error!("Coroutine resume error {:?}", err);
                            }
                        }
                        continue;
                    },
                    _ => {}
                }
            }

            if Scheduler::get().work_count() == 0 {
                break;
            }
        }

        Ok(())
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
        let token = self.handler.slabs.insert((Coroutine::current().clone(), From::from(fd.as_raw_fd()))).unwrap();
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
                Processor::current().ready(hdl);
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
                Processor::current().ready(hdl);
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
        let token = self.handler.slabs.insert(Coroutine::current().clone()).unwrap();
        try!(self.event_loop.register_opt(fd, token, interest,
                                         PollOpt::level()|PollOpt::oneshot()));

        debug!("wait_event: Blocked current Coroutine ...; token={:?}", token);
        Coroutine::block();
        debug!("wait_event: Waked up; token={:?}", token);

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
                Processor::current().ready(hdl);
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
                Processor::current().ready(hdl);
            },
            None => {
                warn!("No coroutine is waiting on readable {:?}", token);
            }
        }

    }
}

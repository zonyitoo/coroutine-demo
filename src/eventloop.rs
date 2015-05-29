use std::io;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::os::unix::io::RawFd;
use std::mem;
use std::convert::From;

use coroutine::coroutine::Handle;

use mio::{self, Evented, Handler, Token, ReadHint, Interest, PollOpt};
use mio::util::Slab;
// #[cfg(target_os = "linux")]
use mio::Io;

use scheduler::Scheduler;

const MAX_TOKEN_NUM: usize = 102400;

pub struct EventLoop {
    handler: IoHandler,
    event_loop: mio::EventLoop<IoHandler>,

    event_sender: Sender<(RawFd, Interest, Handle)>,
    event_receiver: Receiver<(RawFd, Interest, Handle)>,
}

impl EventLoop {
    pub fn new() -> EventLoop {
        EventLoop::with_tokens(MAX_TOKEN_NUM)
    }

    pub fn with_tokens(token_num: usize) -> EventLoop {
        let (tx, rx) = channel();
        EventLoop {
            handler: IoHandler::new(token_num),
            event_loop: mio::EventLoop::new().unwrap(),

            event_sender: tx,
            event_receiver: rx,
        }
    }

    #[cfg(any(target_os = "macos",
              target_os = "freebsd",
              target_os = "dragonfly",
              target_os = "ios",
              target_os = "bitrig",
              target_os = "openbsd"))]
    pub fn register_event(&mut self, io: &Io, inst: Interest, hdl: Handle) -> io::Result<()> {
        let token = self.handler.slabs.insert(hdl).unwrap();
        debug!("Registering {:?} with {:?} {:?}", io, inst, token);
        self.event_loop.register_opt(io, token, inst, PollOpt::level()|PollOpt::oneshot())
    }

    #[cfg(any(target_os = "linux",
              target_os = "android"))]
    pub fn register_event(&mut self, io: &Io, inst: Interest, hdl: Handle) -> io::Result<()> {
        let token = self.handler.slabs.insert((hdl, io.as_raw_fd())).unwrap();
        debug!("Registering {:?} with {:?} {:?}", io, inst, token);
        self.event_loop.register_opt(io, token, inst, PollOpt::level()|PollOpt::oneshot())
    }

    pub fn poll_once(&mut self) -> io::Result<()> {
        self.event_loop.run_once(&mut self.handler)
    }

    pub fn event_sender(&self) -> Sender<(RawFd, Interest, Handle)> {
        self.event_sender.clone()
    }

    pub fn register_events(&mut self) -> io::Result<()> {
        while let Ok((fd, inst, hdl)) = self.event_receiver.try_recv() {
            let io = From::from(fd);
            let ret = self.register_event(&io, inst, hdl);
            mem::forget(io);
            try!(ret);
        }
        Ok(())
    }

    pub fn waiting_count(&self) -> usize {
        self.handler.slabs.count()
    }
}

impl IoHandler {
    fn new(max_token: usize) -> IoHandler {
        IoHandler {
            // slabs: Slab::new_starting_at(Token(1), MAX_TOKEN_NUM),
            slabs: Slab::new(max_token),
        }
    }
}

// #[cfg(any(target_os = "linux",
//           target_os = "android"))]
// impl Processor {
//     pub fn wait_event<E: Evented + AsRawFd>(&mut self, fd: &E, interest: Interest) -> io::Result<()> {
//         let token = self.handler.slabs.insert((Coroutine::current(), From::from(fd.as_raw_fd()))).unwrap();
//         try!(self.event_loop.register_opt(fd, token, interest,
//                                           PollOpt::level()|PollOpt::oneshot()));

//         debug!("wait_event: Blocked current Coroutine ...; token={:?}", token);
//         Coroutine::block();
//         debug!("wait_event: Waked up; token={:?}", token);

//         Ok(())
//     }
// }

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

    fn writable(&mut self, event_loop: &mut mio::EventLoop<Self>, token: Token) {

        debug!("In writable, token {:?}", token);

        match self.slabs.remove(token) {
            Some((hdl, fd)) => {
                // Linux EPoll needs to explicit EPOLL_CTL_DEL the fd
                event_loop.deregister(&fd).unwrap();
                mem::forget(fd);
                // Scheduler::current().ready(hdl);
                Scheduler::ready(hdl);
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
                // Scheduler::current().ready(hdl);
                Scheduler::ready(hdl);
            },
            None => {
                warn!("No coroutine is waiting on readable {:?}", token);
            }
        }

    }
}

// #[cfg(any(target_os = "macos",
//           target_os = "freebsd",
//           target_os = "dragonfly",
//           target_os = "ios",
//           target_os = "bitrig",
//           target_os = "openbsd"))]
// impl Processor {
//     pub fn wait_event<E: Evented>(&mut self, fd: &E, interest: Interest) -> io::Result<()> {
//         // let token = self.io_handler.slabs.insert(Coroutine::current()).unwrap();
//         // try!(self.event_loop.register_opt(fd, token, interest,
//         //                                  PollOpt::level()|PollOpt::oneshot()));

//         // debug!("wait_event: Blocked current Coroutine ...; token={:?}", token);
//         // Coroutine::block();
//         // debug!("wait_event: Waked up; token={:?}", token);

//         Ok(())
//     }
// }

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

    fn writable(&mut self, _: &mut mio::EventLoop<Self>, token: Token) {

        debug!("In writable, token {:?}", token);

        match self.slabs.remove(token) {
            Some(hdl) => {
                // Scheduler::current().ready(hdl);
                Scheduler::ready(hdl);
                // Processor::ready(hdl);
            },
            None => {
                warn!("No coroutine is waiting on writable {:?}", token);
            }
        }

    }

    fn readable(&mut self, _: &mut mio::EventLoop<Self>, token: Token, hint: ReadHint) {

        debug!("In readable, token {:?}, hint {:?}", token, hint);

        match self.slabs.remove(token) {
            Some(hdl) => {
                // Scheduler::current().ready(hdl);
                Scheduler::ready(hdl);
                // Processor::ready(hdl);
            },
            None => {
                warn!("No coroutine is waiting on readable {:?}", token);
            }
        }

    }
}

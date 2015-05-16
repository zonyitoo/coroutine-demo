#![feature(scoped, libc)]

extern crate coroutine;
extern crate num_cpus;
extern crate deque;
#[macro_use] extern crate log;
extern crate env_logger;
extern crate mio;
extern crate libc;

use std::thread;
use std::sync::mpsc::{channel, Sender, Receiver, TryRecvError};
use std::sync::{Mutex, Once, ONCE_INIT};
use std::mem;
use std::cell::UnsafeCell;
use std::io;
use std::net::SocketAddr;
use std::os::unix::io::{RawFd, AsRawFd};

use coroutine::spawn;
use coroutine::coroutine::{State, Handle, Coroutine};

use deque::{BufferPool, Stealer, Worker, Stolen};

use mio::{EventLoop, Io, Handler, Token, ReadHint, Interest, PollOpt, Evented};
use mio::util::Slab;
use mio::buf::Buf;

static mut THREAD_HANDLES: *const Mutex<Vec<(Sender<SchedMessage>, Stealer<Handle>)>> =
    0 as *const Mutex<Vec<(Sender<SchedMessage>, Stealer<Handle>)>>;
static THREAD_HANDLES_ONCE: Once = ONCE_INIT;

fn schedulers() -> &'static Mutex<Vec<(Sender<SchedMessage>, Stealer<Handle>)>> {
    unsafe {
        THREAD_HANDLES_ONCE.call_once(|| {
            let handles: Box<Mutex<Vec<(Sender<SchedMessage>, Stealer<Handle>)>>> =
                Box::new(Mutex::new(Vec::new()));

            THREAD_HANDLES = mem::transmute(handles);
        });

        & *THREAD_HANDLES
    }
}

thread_local!(static SCHEDULER: UnsafeCell<Scheduler> = UnsafeCell::new(Scheduler::new()));

pub enum SchedMessage {
    NewNeighbor(Sender<SchedMessage>, Stealer<Handle>),
}

pub struct Scheduler {
    workqueue: Worker<Handle>,
    workstealer: Stealer<Handle>,

    commchannel: Receiver<SchedMessage>,

    neighbors: Vec<(Sender<SchedMessage>, Stealer<Handle>)>,

    eventloop: EventLoop<SchedulerHandler>,
    handler: SchedulerHandler,
}

impl Scheduler {

    fn new() -> Scheduler {
        let bufpool = BufferPool::new();
        let (worker, stealer) = bufpool.deque();

        let (tx, rx) = channel();

        let scheds = schedulers();
        let mut guard = scheds.lock().unwrap();

        for &(ref rtx, _) in guard.iter() {
            let _ = rtx.send(SchedMessage::NewNeighbor(tx.clone(), stealer.clone()));
        }

        guard.push((tx, stealer.clone()));

        Scheduler {
            workqueue: worker,
            workstealer: stealer,

            commchannel: rx,

            neighbors: guard.clone(),

            eventloop: EventLoop::new().unwrap(),
            handler: SchedulerHandler::new(),
        }
    }

    pub fn current() -> &'static mut Scheduler {
        SCHEDULER.with(|s| unsafe {
            &mut *s.get()
        })
    }

    pub fn spawn<F>(f: F)
            where F: FnOnce() + Send + 'static {

        let coro = spawn(f);

        let sc = Scheduler::current();

        sc.workqueue.push(coro);
    }

    fn schedule(&mut self) {
        loop {
            match self.commchannel.try_recv() {
                Ok(SchedMessage::NewNeighbor(tx, st)) => {
                    self.neighbors.push((tx, st));
                },
                Err(TryRecvError::Empty) => {},
                _ => panic!("Receiving from channel: Unknown message")
            }

            self.eventloop.run_once(&mut self.handler).unwrap();

            match self.workstealer.steal() {
                Stolen::Data(work) => {
                    match work.state() {
                        State::Suspended | State::Blocked => {},
                        _ => {
                            error!("Trying to resume coroutine {:?}, but its state is {:?}",
                                   work, work.state());
                            continue;
                        }
                    }

                    if let Err(msg) = work.resume() {
                        error!("Coroutine panicked! {:?}", msg);
                    }

                    match work.state() {
                        State::Suspended => self.workqueue.push(work),
                        _ => {}
                    }

                    continue;
                },
                Stolen::Empty => {
                    debug!("Nothing to do, try to steal from neighbors");
                },
                Stolen::Abort => {
                    error!("Abort!?");
                }
            }

            for &(_, ref st) in self.neighbors.iter() {
                match st.steal() {
                    Stolen::Empty => {},
                    Stolen::Data(coro) => {
                        match coro.state() {
                            State::Suspended => self.workqueue.push(coro),
                            _ => {}
                        }

                        break;
                    },
                    Stolen::Abort => {}
                }
            }
        }
    }

    fn resume(&mut self, handle: Handle) {
        self.workqueue.push(handle);
    }

}

struct SchedulerHandler {
    slabs: Slab<(Handle, RawFd)>,
}

const MAX_TOKEN_NUM: usize = 102400;
impl SchedulerHandler {
    fn new() -> SchedulerHandler {
        SchedulerHandler {
            slabs: Slab::new_starting_at(Token(1), MAX_TOKEN_NUM),
        }
    }
}

impl Handler for SchedulerHandler {
    type Timeout = ();
    type Message = ();

    fn writable(&mut self, eventloop: &mut EventLoop<Self>, token: Token) {

        debug!("In writable, token {:?}", token);

        // FIXME: WHY??!
        if token == Token(0) {
            error!("In writable got Token(0)!!");
            return;
        }

        match self.slabs.remove(token) {
            Some((hdl, fd)) => {
                Scheduler::current().resume(hdl.clone());

                let fdio = Io::new(fd);
                eventloop.deregister(&fdio).unwrap();
                mem::forget(fdio);
            },
            None => {
                warn!("No coroutine is waiting on writable {:?}", token);
            }
        }

    }

    fn readable(&mut self, eventloop: &mut EventLoop<Self>, token: Token, _: ReadHint) {

        debug!("In readable, token {:?}", token);

        // FIXME: WHY??!
        if token == Token(0) {
            error!("In readable got Token(0)!!");
            return;
        }

        match self.slabs.remove(token) {
            Some((hdl, fd)) => {
                Scheduler::current().resume(hdl.clone());

                let fdio = Io::new(fd);
                eventloop.deregister(&fdio).unwrap();
                mem::forget(fdio);
            },
            None => {
                warn!("No coroutine is waiting on readable {:?}", token);
            }
        }

    }
}

pub struct TcpListener(::mio::tcp::TcpListener);

impl TcpListener {
    pub fn bind(addr: &SocketAddr) -> io::Result<TcpListener> {
        let listener = try!(::mio::tcp::TcpListener::bind(addr));

        Ok(TcpListener(listener))
    }

    pub fn accept(&self) -> io::Result<TcpStream> {
        let mut scheduler = Scheduler::current();

        let token = scheduler.handler.slabs.insert((Coroutine::current(), self.0.as_raw_fd())).unwrap();
        info!("Accepter token {:?}", token);
        scheduler.eventloop.register_opt(&self.0, token, Interest::readable(),
                                         PollOpt::edge()|PollOpt::oneshot()).unwrap();

        Coroutine::block();

        info!("Accept wake up");

        match self.0.accept() {
            Ok(None) => {
                panic!("accept WOULDBLOCK: {:?}", token);
            },
            Ok(Some(stream)) => {
                // scheduler.handler.slabs.remove(token).expect("Unable to remove token from slab");
                // scheduler.eventloop.deregister(&self.0).unwrap();
                Ok(TcpStream(stream))
            },
            Err(err) => {
                // scheduler.handler.slabs.remove(token).expect("Unable to remove token from slab");
                // scheduler.eventloop.deregister(&self.0).unwrap();
                Err(err)
            }
        }
    }
}

pub struct TcpStream(mio::tcp::TcpStream);

impl TcpStream {
    pub fn connect(addr: &SocketAddr) -> io::Result<TcpStream> {
        let stream = try!(mio::tcp::TcpStream::connect(addr));

        Ok(TcpStream(stream))
    }

    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.0.peer_addr()
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.0.local_addr()
    }

    pub fn try_clone(&self) -> io::Result<TcpStream> {
        let stream = try!(self.0.try_clone());

        Ok(TcpStream(stream))
    }
}

impl io::Read for TcpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        use mio::TryRead;

        debug!("Read: Going to register event");

        let mut scheduler = Scheduler::current();

        let token = scheduler.handler.slabs.insert((Coroutine::current(), self.0.as_raw_fd())).unwrap();
        scheduler.eventloop.register_opt(&self.0, token, Interest::readable(),
                                         PollOpt::edge()|PollOpt::oneshot()).unwrap();

        debug!("Read: Blocked current Coroutine ...");
        Coroutine::block();

        match self.0.read_slice(buf) {
            Ok(None) => {
                panic!("TcpStream read WOULDBLOCK");
            },
            Ok(Some(len)) => {
                // scheduler.eventloop.deregister(&self.0).unwrap();
                // scheduler.handler.slabs.remove(token).expect("Unable to remove token from slab");
                debug!("Read {} bytes, {:?}", len, token);
                Ok(len)
            },
            Err(err) => {
                // scheduler.eventloop.deregister(&self.0).unwrap();
                // scheduler.handler.slabs.remove(token).expect("Unable to remove token from slab");
                Err(err)
            }
        }
    }
}

impl io::Write for TcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        use mio::TryWrite;

        debug!("Write: Going to register event");

        let mut scheduler = Scheduler::current();

        let token = scheduler.handler.slabs.insert((Coroutine::current(), self.0.as_raw_fd())).unwrap();
        scheduler.eventloop.register_opt(&self.0, token, Interest::writable(),
                                         PollOpt::edge()|PollOpt::oneshot()).unwrap();

        debug!("Write: Blocked current Coroutine ...");
        Coroutine::block();

        match self.0.write_slice(buf) {
            Ok(None) => {
                panic!("TcpStream write WOULDBLOCK");
            },
            Ok(Some(len)) => {
                // scheduler.eventloop.deregister(&self.0).unwrap();
                // scheduler.handler.slabs.remove(token).expect("Unable to remove token from slab");
                debug!("Written {} bytes, {:?}", len, token);
                Ok(len)
            },
            Err(err) => {
                // scheduler.eventloop.deregister(&self.0).unwrap();
                // scheduler.handler.slabs.remove(token).expect("Unable to remove token from slab");
                Err(err)
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn main() {
    env_logger::init().unwrap();

    Scheduler::spawn(|| {
        let server = TcpListener::bind(&"127.0.0.1:7".parse().unwrap()).unwrap();

        loop {
            use std::io::{Read, Write};

            let mut stream = server.accept().unwrap();
            info!("Accept connection: {:?}", stream.peer_addr().unwrap());

            Scheduler::spawn(move|| {
                let mut buf = [0; 10240];

                loop {
                    info!("Trying to Read...");
                    match stream.read(&mut buf) {
                        Ok(0) => {
                            debug!("EOF received, going to close");
                            break;
                        },
                        Ok(len) => {
                            debug!("Read {} bytes, echo back!", len);
                            stream.write_all(&buf[0..len]).unwrap();
                        },
                        Err(err) => {
                            panic!("Error occurs: {:?}", err);
                        }
                    }
                }

                info!("{:?} closed", stream.peer_addr().unwrap());
            });
        }
    });

    let mut threads = Vec::new();
    for tid in 0..num_cpus::get() {
        let fut = thread::Builder::new().name(format!("Thread {}", tid)).scoped(|| {
            Scheduler::current().schedule();
        }).unwrap();
        threads.push(fut);
    }

    for fut in threads.into_iter() {
        fut.join();
    }

    // Scheduler::current().schedule();
}

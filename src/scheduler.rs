

use std::thread;
use std::sync::mpsc::{channel, Sender, Receiver, TryRecvError};
use std::sync::{Mutex, Once, ONCE_INIT};
use std::mem;
use std::cell::UnsafeCell;
use std::io;
use std::os::unix::io::{RawFd, AsRawFd};
use std::sync::atomic::{ATOMIC_BOOL_INIT, AtomicBool, Ordering};

use coroutine::spawn;
use coroutine::coroutine::{State, Handle, Coroutine};

use deque::{BufferPool, Stealer, Worker, Stolen};

use mio::{EventLoop, Io, Evented, Handler, Token, ReadHint, Interest, PollOpt};
use mio::util::Slab;

static mut THREAD_HANDLES: *const Mutex<Vec<(Sender<SchedMessage>, Stealer<Handle>)>> =
    0 as *const Mutex<Vec<(Sender<SchedMessage>, Stealer<Handle>)>>;
static THREAD_HANDLES_ONCE: Once = ONCE_INIT;
static SCHEDULER_HAS_STARTED: AtomicBool = ATOMIC_BOOL_INIT;

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
    Shutdown,
}

pub struct Scheduler {
    workqueue: Worker<Handle>,
    // workstealer: Stealer<Handle>,

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

        let neighbors = guard.clone();
        guard.push((tx, stealer.clone()));

        Scheduler {
            workqueue: worker,
            // workstealer: stealer,

            commchannel: rx,

            neighbors: neighbors,

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

    pub fn run<F>(f: F, threads: usize)
            where F: FnOnce() + Send + 'static {
        if SCHEDULER_HAS_STARTED.compare_and_swap(false, true, Ordering::SeqCst) != false {
            panic!("Schedulers are already running!");
        }

        Scheduler::spawn(|| {
            struct Guard;

            // Send Shutdown to all schedulers
            impl Drop for Guard {
                fn drop(&mut self) {
                    let guard = match schedulers().lock() {
                        Ok(g) => g,
                        Err(poisoned) => poisoned.into_inner()
                    };

                    for &(ref chan, _) in guard.iter() {
                        let _ = chan.send(SchedMessage::Shutdown);
                    }
                }
            }

            let _guard = Guard;

            f();
        });

        Scheduler::start(threads);

        SCHEDULER_HAS_STARTED.store(false, Ordering::SeqCst);
    }

    fn schedule(&mut self) {
        loop {
            match self.commchannel.try_recv() {
                Ok(SchedMessage::NewNeighbor(tx, st)) => {
                    self.neighbors.push((tx, st));
                },
                Ok(SchedMessage::Shutdown) => {
                    info!("Shutting down");
                    break;
                },
                Err(TryRecvError::Empty) => {},
                _ => panic!("Receiving from channel: Unknown message")
            }

            self.eventloop.run_once(&mut self.handler).unwrap();

            debug!("Trying to resume all ready coroutines: {:?}", thread::current().name());
            // Run all ready coroutines
            let mut need_steal = true;
            while let Some(work) = self.workqueue.pop() {
                match work.state() {
                    State::Suspended | State::Blocked => {
                        debug!("Resuming Coroutine: {:?}", work);
                        need_steal = false;

                        if let Err(err) = work.resume() {
                            let msg = match err.downcast_ref::<&'static str>() {
                                Some(s) => *s,
                                None => match err.downcast_ref::<String>() {
                                    Some(s) => &s[..],
                                    None => "Box<Any>",
                                }
                            };

                            error!("Coroutine panicked! {:?}", msg);
                        }

                        match work.state() {
                            State::Normal | State::Running => {
                                unreachable!();
                            },
                            State::Suspended => {
                                debug!("Coroutine suspended, going to be resumed next round");
                                self.workqueue.push(work);
                            },
                            State::Blocked => {
                                debug!("Coroutine blocked, maybe waiting for I/O");
                            },
                            State::Finished | State::Panicked => {
                                debug!("Coroutine state: {:?}, will not be resumed automatically", work.state());
                            }
                        }
                    },
                    _ => {
                        error!("Trying to resume coroutine {:?}, but its state is {:?}",
                               work, work.state());
                    }
                }
            }

            if !need_steal {
                continue;
            }

            debug!("Trying to steal from neighbors: {:?}", thread::current().name());
            for &(_, ref st) in self.neighbors.iter() {
                match st.steal() {
                    Stolen::Empty => {},
                    Stolen::Data(coro) => {
                        self.workqueue.push(coro);

                        break;
                    },
                    Stolen::Abort => {}
                }
            }
        }
    }

    pub fn wait_event<E: Evented>(&mut self, fd: &E, interest: Interest) -> io::Result<()> {
        let token = self.handler.slabs.insert((Coroutine::current(), fd.as_raw_fd())).unwrap();
        try!(self.eventloop.register_opt(fd, token, interest,
                                         PollOpt::edge()|PollOpt::oneshot()));

        debug!("wait_event: Blocked current Coroutine ...; token={:?}", token);
        Coroutine::block();
        debug!("wait_event: Waked up; token={:?}", token);

        Ok(())
    }

    fn resume(&mut self, handle: Handle) {
        self.workqueue.push(handle);
    }

    fn start(threads: usize) {
        assert!(threads >= 1, "Threads must >= 1");

        for tid in 0..threads - 1 {
            thread::Builder::new().name(format!("Thread {}", tid)).spawn(|| {
                Scheduler::current().schedule();
            }).unwrap();
        }

        Scheduler::current().schedule();
    }
}

struct SchedulerHandler {
    slabs: Slab<(Handle, RawFd)>,
}

const MAX_TOKEN_NUM: usize = 102400;
impl SchedulerHandler {
    fn new() -> SchedulerHandler {
        SchedulerHandler {
            // slabs: Slab::new_starting_at(Token(1), MAX_TOKEN_NUM),
            slabs: Slab::new(MAX_TOKEN_NUM),
        }
    }
}

impl Handler for SchedulerHandler {
    type Timeout = ();
    type Message = ();

    fn writable(&mut self, event_loop: &mut EventLoop<Self>, token: Token) {

        debug!("In writable, token {:?}", token);

        match self.slabs.remove(token) {
            Some((hdl, fd)) => {
                if cfg!(target_os = "linux") {
                    let fd = Io::new(fd);
                    event_loop.deregister(&fd).unwrap();
                    mem::forget(fd);
                }
                Scheduler::current().resume(hdl);
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
                if cfg!(target_os = "linux") {
                    let fd = Io::new(fd);
                    event_loop.deregister(&fd).unwrap();
                    mem::forget(fd);
                }
                Scheduler::current().resume(hdl);
            },
            None => {
                warn!("No coroutine is waiting on readable {:?}", token);
            }
        }

    }
}



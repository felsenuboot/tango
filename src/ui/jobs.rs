//! The job queue: downloads, imports and removals run one after the other on a worker thread
//! while the window stays usable. Clicking Download on three sources queues three jobs.
//!
//! The worker cannot touch widgets, so it sends progress through a channel and a future on the
//! main loop applies it (the standard gtk-rs threading pattern). Whoever shows job state (the
//! Dictionaries page rows, the empty state, the spinner in the header) registers a listener and
//! reads `state` / `current` when it fires.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use gtk::glib;

use crate::store::import::Report;

pub type JobFn = Box<dyn FnOnce(Report) -> anyhow::Result<String> + Send>;
pub type DoneFn = Box<dyn FnOnce(Result<String, String>)>;

/// What a source's row shows.
#[derive(Debug, Clone, PartialEq)]
pub enum State {
    Idle,
    Queued,
    Running { message: String, fraction: Option<f64> },
}

struct Job {
    /// `Source::id` the job works on; the rows key their state by it.
    source: &'static str,
    /// "Downloading Wadoku", for the header spinner's tooltip and the quit prompt.
    title: String,
    run: JobFn,
    done: DoneFn,
}

/// The job on the worker thread right now.
#[derive(Debug, Clone)]
pub struct Current {
    pub source: &'static str,
    pub title: String,
    pub message: String,
    pub fraction: Option<f64>,
}

enum Progress {
    Status(String, Option<f64>),
    Finished(Result<String, String>),
}

#[derive(Default)]
pub struct Jobs {
    queue: RefCell<VecDeque<Job>>,
    current: RefCell<Option<Current>>,
    /// Called after every change; a listener returning `false` is dropped (its widget is gone).
    listeners: RefCell<Vec<Box<dyn Fn() -> bool>>>,
}

impl Jobs {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// Queues a job; it starts at once when nothing else is running. `done` runs on the main
    /// thread with the job's toast text or its error.
    pub fn enqueue(self: &Rc<Self>, source: &'static str, title: String, run: JobFn, done: DoneFn) {
        self.queue.borrow_mut().push_back(Job {
            source,
            title,
            run,
            done,
        });
        self.notify();
        self.start_next();
    }

    pub fn state(&self, source: &str) -> State {
        if let Some(current) = self.current.borrow().as_ref()
            && current.source == source
        {
            return State::Running {
                message: current.message.clone(),
                fraction: current.fraction,
            };
        }
        if self.queue.borrow().iter().any(|j| j.source == source) {
            State::Queued
        } else {
            State::Idle
        }
    }

    pub fn current(&self) -> Option<Current> {
        self.current.borrow().clone()
    }

    pub fn is_busy(&self) -> bool {
        self.current.borrow().is_some() || !self.queue.borrow().is_empty()
    }

    /// Jobs waiting behind the running one.
    pub fn queued(&self) -> usize {
        self.queue.borrow().len()
    }

    pub fn connect(&self, listener: impl Fn() -> bool + 'static) {
        self.listeners.borrow_mut().push(Box::new(listener));
    }

    fn notify(&self) {
        // Listeners may register listeners (a rebuilt row does), so they run outside the borrow.
        let listeners = std::mem::take(&mut *self.listeners.borrow_mut());
        let mut kept: Vec<Box<dyn Fn() -> bool>> = listeners.into_iter().filter(|l| l()).collect();
        let mut added = self.listeners.borrow_mut();
        kept.append(&mut added);
        *added = kept;
    }

    fn start_next(self: &Rc<Self>) {
        if self.current.borrow().is_some() {
            return;
        }
        let Some(job) = self.queue.borrow_mut().pop_front() else {
            return;
        };
        *self.current.borrow_mut() = Some(Current {
            source: job.source,
            title: job.title.clone(),
            message: "Starting…".into(),
            fraction: None,
        });
        self.notify();

        let (tx, rx) = async_channel::unbounded::<Progress>();
        let run = job.run;
        std::thread::Builder::new()
            .name("tango-jobs".into())
            .spawn(move || {
                let mut report = |message: String, fraction: Option<f64>| {
                    let _ = tx.send_blocking(Progress::Status(message, fraction));
                };
                let result = run(&mut report).map_err(|e| format!("{e:#}"));
                let _ = tx.send_blocking(Progress::Finished(result));
            })
            .expect("spawning the job thread");

        let this = self.clone();
        let mut done = Some(job.done);
        glib::spawn_future_local(async move {
            while let Ok(progress) = rx.recv().await {
                match progress {
                    Progress::Status(message, fraction) => {
                        if let Some(current) = this.current.borrow_mut().as_mut() {
                            current.message = message;
                            current.fraction = fraction;
                        }
                        this.notify();
                    }
                    Progress::Finished(result) => {
                        if let Err(e) = &result {
                            log::error!("job failed: {e}");
                        }
                        *this.current.borrow_mut() = None;
                        if let Some(done) = done.take() {
                            done(result);
                        }
                        this.notify();
                        this.start_next();
                        break;
                    }
                }
            }
        });
    }
}

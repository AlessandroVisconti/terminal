//! The background worker that compresses terminals' cold history.
//!
//! With history compression on, a terminal's old history blocks are compressed by history jobs
//! (feathercritty's `Term::next_history_jobs`). A job holds a copy of its block, so the work runs
//! with no lock held; only taking jobs and installing results lock the terminal. One worker
//! thread, `fbterm-history-worker` at a lowered priority, serves every terminal in the process:
//! [`enqueue`] after a feed that leaves jobs, and it sleeps on a condition variable while no
//! terminal has any (it never ticks).
//!
//! Jobs go in batches of [`BATCH`]: one lock hold installs the previous batch's results and takes
//! the next, so the worker keeps up with a parser that holds the lock almost all the time (with
//! one job per hold, a sustained flood grew the backlog without bound). Terminals take turns,
//! one batch each per turn; a terminal's next batch, taken in the hold that installed the last,
//! waits in its queue entry for its next turn.

use std::collections::VecDeque;
use std::sync::{Arc, Once, OnceLock, Weak};
use std::time::{Duration, Instant};

use feathercritty::term::HistoryJob;
use parking_lot::{Condvar, Mutex};

use crate::{SharedTerminal, Terminal};

/// History jobs taken (and installed) per lock hold.
pub const BATCH: usize = 8;

/// A terminal waiting for its turn.
struct Entry {
    term: Weak<Mutex<Terminal>>,
    /// Jobs taken at the end of its last turn, run at the start of its next.
    jobs: Vec<HistoryJob>,
}

/// The queued terminals and whether a turn is in progress. `busy` lives under the same lock as
/// the entries, so "empty and not busy" is read without a gap between a turn's end and the
/// check of a waiter.
#[derive(Default)]
struct Queue {
    entries: VecDeque<Entry>,
    busy: bool,
}

/// Ends a turn when dropped, also when a job panicked: clears `busy` and wakes waiters, so
/// they never wait on a turn that is gone.
struct TurnGuard<'a>(&'a Worker);

impl Drop for TurnGuard<'_> {
    fn drop(&mut self) {
        self.0.finish_turn();
    }
}

/// Runs terminals' history jobs, round robin. The app uses the process-wide one through
/// [`enqueue`]; tests drive their own with [`Worker::run_until_idle`].
#[derive(Default)]
pub struct Worker {
    queue: Mutex<Queue>,
    wake: Condvar,
    /// A test makes the next turn's job panic.
    #[cfg(test)]
    panic_next: std::sync::atomic::AtomicBool,
    /// Signalled (with `queue` locked) when the queue is empty and no turn is in progress.
    idle: Condvar,
    /// The terminal (as a pointer) of every install hold, in order.
    #[cfg(test)]
    installs: Mutex<Vec<usize>>,
}

impl Worker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue `term` for history jobs and wake the worker. Cheap when it is already queued
    /// (a scan of the few queued terminals, no allocation): it is called after every feed that
    /// leaves jobs.
    pub fn enqueue(&self, term: &SharedTerminal) {
        let mut queue = self.queue.lock();
        let ptr = Arc::as_ptr(term);
        if queue.entries.iter().any(|e| e.term.as_ptr() == ptr) {
            return;
        }
        queue.entries.push_back(Entry {
            term: Arc::downgrade(term),
            jobs: Vec::new(),
        });
        drop(queue);
        self.wake.notify_one();
    }

    /// Run turns until no terminal is queued (tests; the thread blocks instead).
    pub fn run_until_idle(&self) {
        loop {
            let Some(entry) = self.take(&mut self.queue.lock()) else {
                return;
            };
            let _guard = TurnGuard(self);
            self.turn(entry);
        }
    }

    /// Block until the queue is empty and no turn is in progress. A carried batch keeps its
    /// entry queued, so it counts as work. Sleeps on a condition variable.
    pub fn wait_idle(&self) {
        let mut queue = self.queue.lock();
        while !queue.entries.is_empty() || queue.busy {
            self.idle.wait(&mut queue);
        }
    }

    /// Like [`Worker::wait_idle`], but gives up at one deadline `timeout` from now. Returns
    /// whether the worker was idle.
    pub fn wait_idle_for(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut queue = self.queue.lock();
        while !queue.entries.is_empty() || queue.busy {
            if self.idle.wait_until(&mut queue, deadline).timed_out() {
                return queue.entries.is_empty() && !queue.busy;
            }
        }
        true
    }

    /// Pop the next entry, marking a turn in progress in the same hold.
    fn take(&self, queue: &mut Queue) -> Option<Entry> {
        let entry = queue.entries.pop_front()?;
        queue.busy = true;
        Some(entry)
    }

    /// A turn ended (any requeue already happened): wake waiters if nothing is left.
    fn finish_turn(&self) {
        let mut queue = self.queue.lock();
        queue.busy = false;
        if queue.entries.is_empty() {
            self.idle.notify_all();
        }
    }

    /// The worker thread's loop: never returns.
    fn run(&self) -> ! {
        loop {
            let entry = {
                let mut queue = self.queue.lock();
                loop {
                    if let Some(entry) = self.take(&mut queue) {
                        break entry;
                    }
                    self.wake.wait(&mut queue);
                }
            };
            let _guard = TurnGuard(self);
            self.turn(entry);
        }
    }

    /// One turn of one terminal: run its batch with no lock held, then in one hold install the
    /// results and take the next batch. Requeued while it has work left.
    fn turn(&self, entry: Entry) {
        // A closed terminal: its jobs are dropped with it.
        let Some(term) = entry.term.upgrade() else {
            return;
        };
        #[cfg(test)]
        if self
            .panic_next
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            panic!("test: a history job panicked");
        }
        let mut jobs = entry.jobs;
        if jobs.is_empty() {
            jobs = term.lock().next_history_jobs(BATCH);
            if jobs.is_empty() {
                return;
            }
        }
        let done: Vec<_> = jobs.into_iter().map(HistoryJob::run).collect();
        let (next, more) = {
            let mut t = term.lock();
            for d in done {
                // `false`: stale (the history changed meanwhile); the result is dropped.
                t.install_history_job(d);
            }
            let next = t.next_history_jobs(BATCH);
            let more = !next.is_empty() || t.has_history_jobs();
            (next, more)
        };
        #[cfg(test)]
        self.installs.lock().push(Arc::as_ptr(&term) as usize);
        if more {
            self.requeue(&term, next);
        }
    }

    /// Put `term` back at the end of the queue with its next batch, unless a feed queued it
    /// meanwhile (then that entry takes the batch).
    fn requeue(&self, term: &SharedTerminal, jobs: Vec<HistoryJob>) {
        let mut queue = self.queue.lock();
        let ptr = Arc::as_ptr(term);
        if let Some(entry) = queue.entries.iter_mut().find(|e| e.term.as_ptr() == ptr) {
            if entry.jobs.is_empty() {
                entry.jobs = jobs;
            }
            return;
        }
        queue.entries.push_back(Entry {
            term: Arc::downgrade(term),
            jobs,
        });
    }
}

/// Queue a terminal that has history jobs on the process-wide worker, starting its thread on
/// the first call.
pub fn enqueue(term: &SharedTerminal) {
    global().enqueue(term);
}

/// Block until the process-wide worker has nothing queued and no turn running. Returns at once
/// when its thread was never started.
pub fn wait_idle() {
    if let Some(worker) = WORKER.get() {
        worker.wait_idle();
    }
}

/// [`wait_idle`] for at most `timeout`; returns whether the worker was idle.
pub fn wait_idle_for(timeout: Duration) -> bool {
    WORKER.get().is_none_or(|w| w.wait_idle_for(timeout))
}

static WORKER: OnceLock<Worker> = OnceLock::new();

fn global() -> &'static Worker {
    static START: Once = Once::new();
    let worker = WORKER.get_or_init(Worker::new);
    START.call_once(|| {
        // If the thread cannot start, history simply stays uncompressed.
        let _ = std::thread::Builder::new()
            .name("fbterm-history-worker".into())
            .spawn(move || {
                lower_priority();
                worker.run()
            });
    });
    worker
}

/// Lower the calling thread's priority, so compression yields to parsing and drawing. A
/// failure is ignored: the worker then runs at normal priority.
fn lower_priority() {
    #[cfg(target_os = "macos")]
    // SAFETY: sets the quality of service of the calling thread only; it takes two plain values
    // and touches no memory of ours.
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0);
    }
    #[cfg(target_os = "linux")]
    // SAFETY: on Linux the nice value belongs to the calling thread, not the process; `nice`
    // takes a plain value and touches no memory of ours.
    unsafe {
        libc::nice(10);
    }
    // Windows: `SetThreadPriority` needs windows-sys's `Win32_System_Threading` feature, which
    // the workspace does not enable, so the worker keeps normal priority there.
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use feathercritty::grid::{Dimensions, RowReader};
    use feathercritty::index::Line;
    use parking_lot::Mutex;

    use super::*;
    use crate::{GridSize, Terminal};

    fn size() -> GridSize {
        GridSize {
            cols: 120,
            rows: 24,
            cell_width: 8.0,
            cell_height: 16.0,
        }
    }

    /// `lines` numbered lines of about `width` characters.
    fn output(lines: usize, width: usize) -> Vec<u8> {
        let mut out = Vec::new();
        for i in 0..lines {
            let text = format!("line {i:06} ");
            out.extend_from_slice(text.as_bytes());
            for k in 0..width.saturating_sub(text.len()) {
                out.push(b'a' + ((i + k) % 26) as u8);
            }
            out.extend_from_slice(b"\r\n");
        }
        out
    }

    fn terminal(scrollback: usize, keep: Option<usize>, bytes: &[u8]) -> Terminal {
        let mut t = Terminal::new(size(), scrollback);
        t.set_history_compression(keep);
        t.feed(bytes);
        t
    }

    fn shared(t: Terminal) -> SharedTerminal {
        Arc::new(Mutex::new(t))
    }

    #[test]
    fn worker_runs_jobs_until_none() {
        let bytes = output(20_000, 40);
        let term = shared(terminal(5_000, Some(1_000), &bytes));
        assert!(term.lock().has_history_jobs());

        let worker = Worker::new();
        worker.enqueue(&term);
        worker.run_until_idle();

        let t = term.lock();
        assert!(!t.has_history_jobs());
        let plain = terminal(5_000, None, &bytes);
        let (a, b) = (t.term().grid(), plain.term().grid());
        assert_eq!(a.history_size(), b.history_size());
        assert_eq!(a.history_size(), 5_000);
        let (mut ra, mut rb) = (RowReader::new(), RowReader::new());
        for i in 1..=a.history_size() as i32 {
            assert!(ra.row(a, Line(-i)) == rb.row(b, Line(-i)), "row {}", -i);
        }
    }

    #[test]
    fn wait_idle_returns_when_nothing_is_queued() {
        Worker::new().wait_idle();
    }

    #[test]
    fn wait_idle_blocks_until_every_job_is_done() {
        let term = shared(terminal(5_000, Some(1_000), &output(20_000, 40)));
        let worker = Arc::new(Worker::new());
        worker.enqueue(&term);
        let (tx, rx) = std::sync::mpsc::channel();
        let waiter = {
            let worker = worker.clone();
            std::thread::spawn(move || {
                worker.wait_idle();
                tx.send(()).unwrap();
            })
        };
        // Work is queued: the waiter must still be waiting.
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
        worker.run_until_idle();
        rx.recv_timeout(Duration::from_secs(30)).unwrap();
        waiter.join().unwrap();
        assert!(!term.lock().has_history_jobs());
    }

    #[test]
    fn wait_idle_for_gives_up_at_its_deadline_while_a_turn_runs() {
        let worker = Worker::new();
        assert!(worker.wait_idle_for(Duration::from_millis(50)));
        worker.queue.lock().busy = true;
        let start = Instant::now();
        assert!(!worker.wait_idle_for(Duration::from_millis(50)));
        assert!(start.elapsed() >= Duration::from_millis(50));
        worker.finish_turn();
        assert!(worker.wait_idle_for(Duration::from_millis(50)));
    }

    #[test]
    fn a_panicking_job_leaves_the_worker_idle() {
        let term = shared(terminal(5_000, Some(1_000), &output(20_000, 40)));
        let worker = Worker::new();
        worker
            .panic_next
            .store(true, std::sync::atomic::Ordering::SeqCst);
        worker.enqueue(&term);
        let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            worker.run_until_idle();
        }));
        assert!(ran.is_err());
        assert!(!worker.queue.lock().busy);
        worker.wait_idle();
    }

    #[test]
    fn worker_skips_dropped_terminal() {
        let term = shared(terminal(5_000, Some(1_000), &output(20_000, 40)));
        let worker = Worker::new();
        worker.enqueue(&term);
        drop(term);
        worker.run_until_idle();
    }

    #[test]
    fn worker_round_robins_terminals() {
        // About 600 rows per 64 KiB block: dozens of blocks, so several batches each.
        let bytes = output(30_000, 110);
        let a = shared(terminal(30_000, Some(1_000), &bytes));
        let b = shared(terminal(30_000, Some(1_000), &bytes));
        let worker = Worker::new();
        worker.enqueue(&a);
        worker.enqueue(&b);
        // Enqueuing again changes nothing.
        worker.enqueue(&a);
        worker.run_until_idle();

        assert!(!a.lock().has_history_jobs());
        assert!(!b.lock().has_history_jobs());

        let (pa, pb) = (Arc::as_ptr(&a) as usize, Arc::as_ptr(&b) as usize);
        let order = worker.installs.lock().clone();
        let batches = |p: usize| order.iter().filter(|&&q| q == p).count();
        assert!(batches(pa) >= 3, "{order:?}");
        assert!(batches(pb) >= 3, "{order:?}");
        // One batch per terminal per turn while both have work: strictly alternating until the
        // one with fewer batches is done.
        let alternating = 2 * batches(pa).min(batches(pb));
        for pair in order[..alternating].windows(2) {
            assert_ne!(pair[0], pair[1], "{order:?}");
        }
    }
}

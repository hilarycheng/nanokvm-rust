//! Cooperative cancellation and completion for device-owning threads.
use std::{sync::{Arc, atomic::{AtomicBool, Ordering}}, thread::{self, JoinHandle}, time::{Duration, Instant}};

pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

pub struct Worker {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<Result<(), String>>>,
}

impl Worker {
    pub fn spawn(run: impl FnOnce(Arc<AtomicBool>) -> Result<(), String> + Send + 'static) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let signal = stop.clone();
        Self { stop, thread: Some(thread::spawn(move || run(signal))) }
    }

    pub fn stop(&self) { self.stop.store(true, Ordering::Release); }

    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    pub fn finish(&mut self) -> Option<Result<(), String>> {
        if !self.is_finished() { return None; }
        self.thread.take().map(|thread| thread.join().unwrap_or_else(|_| Err("Device worker panicked".into())))
    }

    pub fn wait_until(&mut self, deadline: Instant) -> Result<(), String> {
        loop {
            if let Some(result) = self.finish() { return result; }
            if self.thread.is_none() { return Ok(()); }
            if Instant::now() >= deadline { return Err("Device worker did not stop before the shutdown deadline".into()); }
            thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) { self.stop(); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_waits_for_resource_drop_and_is_repeatable() {
        let (tx, rx) = crossbeam_channel::bounded::<()>(1);
        let mut worker = Worker::spawn(move |stop| {
            let _owned_resource = tx;
            while !stop.load(Ordering::Acquire) { thread::yield_now(); }
            Ok(())
        });
        worker.stop();
        worker.stop();
        worker.wait_until(Instant::now() + SHUTDOWN_TIMEOUT).unwrap();
        assert!(rx.recv().is_err());
        worker.wait_until(Instant::now()).unwrap();
    }

    #[test]
    fn deadline_does_not_claim_a_stalled_worker_has_closed() {
        let (tx, rx) = crossbeam_channel::bounded(1);
        let mut worker = Worker::spawn(move |_| { rx.recv().unwrap(); Ok(()) });
        worker.stop();
        assert!(worker.wait_until(Instant::now()).is_err());
        assert!(!worker.is_finished());
        tx.send(()).unwrap();
        worker.wait_until(Instant::now() + SHUTDOWN_TIMEOUT).unwrap();
    }
}

//! Progress line on standard error.

use std::io::Write;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Shortest time between two redraws of the progress line, so that a fast batch does not
/// spend its time writing to the terminal.
const REDRAW: Duration = Duration::from_millis(200);

/// Counts finished plants and redraws one status line on standard error.
#[derive(Debug)]
pub struct Progress {
    total: usize,
    skipped: usize,
    done: AtomicUsize,
    failed: AtomicUsize,
    start: Instant,
    last_draw: Mutex<Instant>,
    quiet: bool,
}

impl Progress {
    /// Progress over `total` plants to generate, `skipped` more being already done.
    pub fn new(total: usize, skipped: usize, quiet: bool) -> Self {
        let start = Instant::now();
        Self {
            total,
            skipped,
            done: AtomicUsize::new(0),
            failed: AtomicUsize::new(0),
            start,
            last_draw: Mutex::new(start),
            quiet,
        }
    }

    /// Records one finished plant and redraws the line if it is due.
    pub fn tick(&self, ok: bool) {
        self.done.fetch_add(1, Ordering::Relaxed);
        if !ok {
            self.failed.fetch_add(1, Ordering::Relaxed);
        }
        if self.quiet {
            return;
        }
        if let Ok(mut last) = self.last_draw.try_lock()
            && last.elapsed() >= REDRAW
        {
            *last = Instant::now();
            self.draw();
        }
    }

    /// Draws the final line.
    pub fn finish(&self) {
        if !self.quiet {
            self.draw();
            eprintln!();
        }
    }

    /// Seconds since the start.
    pub fn elapsed(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    /// Redraws the line in place: done, rate, elapsed, remaining, skipped, failed.
    fn draw(&self) {
        let done = self.done.load(Ordering::Relaxed);
        let elapsed = self.elapsed();
        let rate = if elapsed > 0.0 {
            done as f64 / elapsed
        } else {
            0.0
        };
        let eta = if rate > 0.0 {
            format!(", {:.0} s left", (self.total - done) as f64 / rate)
        } else {
            String::new()
        };
        let skipped = if self.skipped > 0 {
            format!(", {} skipped", self.skipped)
        } else {
            String::new()
        };
        let failed = self.failed.load(Ordering::Relaxed);
        let failed = if failed > 0 {
            format!(", {failed} failed")
        } else {
            String::new()
        };
        let line = format!(
            "\r{done}/{} plants, {rate:.1}/s, {elapsed:.1} s{eta}{skipped}{failed}   ",
            self.total
        );
        let mut err = std::io::stderr().lock();
        let _ = err.write_all(line.as_bytes());
        let _ = err.flush();
    }
}

//! A check that the time of a test case grows in proportion to its size.

use std::time::Duration;

/// A time below this floor is too short to compare.
pub(crate) const FLOOR: Duration = Duration::from_millis(100);

/// The number of runs at each size.
const RUNS: usize = 3;

/// Measures the processor time of the current thread. A timing case uses it
/// in place of `std::time::Instant`. A loaded machine gives a thread less
/// time, but the thread needs the same processor time for the same work.
pub(crate) struct Stopwatch(Duration);

impl Stopwatch {
    pub(crate) fn start() -> Self {
        Self(thread_time())
    }

    pub(crate) fn elapsed(&self) -> Duration {
        thread_time().saturating_sub(self.0)
    }
}

#[cfg(unix)]
fn thread_time() -> Duration {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `time` is a valid `timespec` that the call writes.
    let result = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut time) };
    assert_eq!(result, 0, "the processor time of the thread");
    Duration::new(time.tv_sec as u64, time.tv_nsec as u32)
}

/// Without a clock for the thread, the wall time since the first call.
#[cfg(not(unix))]
fn thread_time() -> Duration {
    static ORIGIN: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    ORIGIN.get_or_init(std::time::Instant::now).elapsed()
}

/// Runs `run` on a new pool of one thread. The parallel work of `run` thus
/// occurs on that thread, and `Stopwatch` measures all of it.
pub(crate) fn on_one_thread<T: Send>(run: impl FnOnce() -> T + Send) -> T {
    rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .expect("a thread for the timing run")
        .install(run)
}

/// Runs a timing case at `size` and at twice that size, and checks that the
/// time grows in proportion to the size. A quadratic search takes four times
/// as long for twice the input. The check uses the best time of each size.
/// `run` gives the time of the case, which it measures with `Stopwatch`.
pub(crate) fn assert_scales_linearly(
    label: &str,
    size: usize,
    run: impl Fn(usize) -> Duration + Sync,
) {
    assert_scales_linearly_above(label, size, FLOOR, run);
}

/// Like `assert_scales_linearly`, with a different floor for a time that is
/// too short to compare.
pub(crate) fn assert_scales_linearly_above(
    label: &str,
    size: usize,
    floor: Duration,
    run: impl Fn(usize) -> Duration + Sync,
) {
    // A case measures the processor time of its thread with `Stopwatch`. Thus
    // each run has a new pool of one thread, which also does the parallel
    // work of the run. The runs of the two sizes alternate, so that a slow
    // period of the machine affects the two sizes.
    let run_on_new_thread = |size: usize| on_one_thread(|| run(size));
    let mut small = Duration::MAX;
    let mut large = Duration::MAX;
    for _ in 0..RUNS {
        small = small.min(run_on_new_thread(size));
        large = large.min(run_on_new_thread(size * 2));
    }
    let allowed = small.max(floor) * 3;
    assert!(
        large < allowed,
        "{label}: {small:?} at {size}, {large:?} at {}",
        size * 2
    );
    assert!(large.as_secs() < 60, "{label}: {large:?}");
}

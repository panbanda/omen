//! Tests of the timing check in `crate::index::timing`.

use crate::index::timing::*;
use std::sync::Mutex;
use std::time::Duration;

/// The size and the thread of each run, in the order of the runs.
fn runs_of(times: impl Fn(usize) -> Duration + Sync) -> Vec<(usize, std::thread::ThreadId)> {
    let runs = Mutex::new(Vec::new());
    assert_scales_linearly("order", 10, |size| {
        runs.lock()
            .unwrap()
            .push((size, std::thread::current().id()));
        times(size)
    });
    runs.into_inner().unwrap()
}

/// A thread that waits uses no processor time.
#[test]
fn test_stopwatch_counts_work_and_not_a_wait() {
    let watch = Stopwatch::start();
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        watch.elapsed() < Duration::from_millis(100),
        "{:?}",
        watch.elapsed()
    );
    // A loaded runner can give this thread little processor time in a fixed
    // wall time, so the loop runs until the stopwatch shows the work.
    let watch = Stopwatch::start();
    let started = std::time::Instant::now();
    while watch.elapsed() < Duration::from_millis(20) {
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "the stopwatch counted no work in 60 s: {:?}",
            watch.elapsed()
        );
        std::hint::black_box(0);
    }
}

#[test]
fn test_sizes_alternate_and_each_run_has_a_new_thread() {
    let runs = runs_of(|_| Duration::ZERO);
    let sizes: Vec<usize> = runs.iter().map(|(size, _)| *size).collect();
    assert_eq!(sizes, [10, 20, 10, 20, 10, 20]);
    let threads: std::collections::HashSet<_> = runs.iter().map(|(_, id)| *id).collect();
    assert_eq!(threads.len(), runs.len());
    assert!(!threads.contains(&std::thread::current().id()));
}

/// The parallel work of a run occurs on the thread of the run, not on
/// the global pool.
#[test]
fn test_parallel_work_of_a_run_has_the_thread_of_the_run() {
    use rayon::prelude::*;
    assert_scales_linearly("parallel", 10, |size| {
        let run = std::thread::current().id();
        let threads: std::collections::HashSet<_> = (0..size * 100)
            .into_par_iter()
            .map(|_| std::thread::current().id())
            .collect();
        assert_eq!(threads.into_iter().collect::<Vec<_>>(), [run]);
        Duration::ZERO
    });
}

/// The check compares the best time of each size. One slow run of the
/// large size does not fail the check.
#[test]
fn test_best_time_of_each_size_is_compared() {
    let count = std::sync::atomic::AtomicUsize::new(0);
    runs_of(|size| {
        let run = count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        match (size, run) {
            (20, 1) => Duration::from_secs(5),
            (20, _) => Duration::from_millis(250),
            _ => Duration::from_millis(100),
        }
    });
}

#[test]
#[should_panic(expected = "quadratic: 100ms at 10, 400ms at 20")]
fn test_time_that_grows_with_the_square_fails() {
    assert_scales_linearly("quadratic", 10, |size| {
        Duration::from_millis((size * size) as u64)
    });
}

/// Below the floor, a ratio of two times is noise.
#[test]
fn test_time_below_the_floor_passes() {
    assert_scales_linearly("short", 10, |size| Duration::from_millis(size as u64 * 7));
}

#[test]
#[should_panic(expected = "the timing run failed")]
fn test_failure_in_a_run_fails_the_check() {
    assert_scales_linearly("failure", 10, |_| panic!("the timing run failed"));
}

/// The `size * size` additions of a quadratic case, or `size * 1000`
/// additions of a linear case.
fn work(size: usize, quadratic: bool) -> Duration {
    let started = Stopwatch::start();
    let inner = if quadratic { size } else { 1_000 };
    let mut total = 0u64;
    for outer in 0..size {
        for step in 0..inner {
            total = std::hint::black_box(total.wrapping_add((outer ^ step) as u64));
        }
    }
    std::hint::black_box(total);
    started.elapsed()
}

#[test]
#[ignore = "timing: run in release"]
fn timing_linear_work_passes_the_check() {
    assert_scales_linearly("linear", 400_000, |size| work(size, false));
}

#[test]
#[ignore = "timing: run in release"]
fn timing_quadratic_work_fails_the_check() {
    // Below the floor the check passes any ratio, so the small size must
    // take at least twice the floor on this machine.
    let mut size = 20_000;
    while work(size, true) < FLOOR * 2 {
        size *= 2;
        assert!(size <= 1 << 24, "quadratic work never reached the floor");
    }
    let failed = std::panic::catch_unwind(|| {
        assert_scales_linearly("quadratic", size, |size| work(size, true));
    });
    assert!(failed.is_err(), "quadratic work passed the check");
}

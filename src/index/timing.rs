//! The check of the `timing_` tests: the time of a case grows in proportion to its size.

/// Runs a timing case at `size` and at twice that size, and checks that the time grows in
/// proportion to the size. A quadratic search takes four times as long for twice the input. The
/// check uses the best of two runs for each size, and a floor for a time that is too short to
/// compare, so the speed of the machine does not change the result. The absolute bound is far
/// above any speed that a CI machine has.
pub fn assert_scales_linearly(
    label: &str,
    size: usize,
    run: impl Fn(usize) -> std::time::Duration,
) {
    let best = |size: usize| (0..2).map(|_| run(size)).min().expect("two runs");
    let small = best(size);
    let large = best(size * 2);
    let allowed = small.max(std::time::Duration::from_millis(100)) * 3;
    assert!(
        large < allowed,
        "{label}: {small:?} at {size}, {large:?} at {}",
        size * 2
    );
    assert!(large.as_secs() < 60, "{label}: {large:?}");
}

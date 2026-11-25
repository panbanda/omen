//! Tests of the timing check in `crate::index::timing`.

use crate::index::timing::*;
use std::sync::Mutex;
use std::time::Duration;

/// A time that is linear in `size` and above the floor at the size 10.
fn above_the_floor(size: usize) -> Duration {
    Duration::from_millis(size as u64 * 15)
}

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
    let runs = runs_of(above_the_floor);
    let sizes: Vec<usize> = runs.iter().map(|(size, _)| *size).collect();
    assert_eq!(sizes, [10, 40, 10, 40, 10, 40]);
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
        above_the_floor(size)
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
            (40, 1) => Duration::from_secs(5),
            (40, _) => Duration::from_millis(400),
            _ => Duration::from_millis(100),
        }
    });
}

#[test]
#[should_panic(expected = "quadratic: 100ms at 10, 1.6s at 40")]
fn test_time_that_grows_with_the_square_fails() {
    assert_scales_linearly("quadratic", 10, |size| {
        Duration::from_millis((size * size) as u64)
    });
}

/// Below the floor, a ratio of two times is noise. The check thus doubles
/// the two sizes until the small size takes the floor, and compares there.
#[test]
#[should_panic(expected = "quadratic: 102.4ms at 320, 1.6384s at 1280")]
fn test_short_quadratic_time_grows_to_the_floor_and_fails() {
    assert_scales_linearly("quadratic", 10, |size| {
        Duration::from_micros((size * size) as u64)
    });
}

#[test]
fn test_short_linear_time_grows_to_the_floor_and_passes() {
    let sizes = Mutex::new(Vec::new());
    assert_scales_linearly("linear", 10, |size| {
        sizes.lock().unwrap().push(size);
        Duration::from_millis(size as u64)
    });
    let sizes = sizes.into_inner().unwrap();
    assert_eq!(sizes.first(), Some(&10));
    assert_eq!(sizes.iter().max(), Some(&640));
}

/// A case that cannot take the floor at 64 times its size cannot show a
/// quadratic search. The author must give a larger size.
#[test]
#[should_panic(expected = "short: 640ns at 640 is below the floor of 100ms; give a larger size")]
fn test_time_that_cannot_reach_the_floor_fails() {
    assert_scales_linearly("short", 10, |size| Duration::from_nanos(size as u64));
}

#[test]
#[should_panic(expected = "the timing run failed")]
fn test_failure_in_a_run_fails_the_check() {
    assert_scales_linearly("failure", 10, |_| panic!("the timing run failed"));
}

/// The quadratic case starts at a size that takes much less than the floor.
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
    let failed = std::panic::catch_unwind(|| {
        assert_scales_linearly("quadratic", 2_000, |size| work(size, true));
    });
    assert!(failed.is_err(), "quadratic work passed the check");
}

/// The functions of `text`, by name, each with the text up to the next function.
fn functions_of(text: &str) -> std::collections::HashMap<&str, String> {
    let mut functions: std::collections::HashMap<&str, String> = std::collections::HashMap::new();
    let mut current: Option<&str> = None;
    for line in text.lines() {
        if let Some(name) = declared_name(line) {
            current = Some(name);
        }
        if let Some(name) = current {
            let body = functions.entry(name).or_default();
            body.push_str(line);
            body.push('\n');
        }
    }
    functions
}

/// The byte range of the arguments of each call of `function` in `text`.
fn arguments_of(text: &str, function: &str) -> Vec<std::ops::Range<usize>> {
    let call = format!("{function}(");
    let mut calls = Vec::new();
    for (start, _) in text.match_indices(&call) {
        let first = start + call.len();
        let mut depth = 1;
        let end = text[first..]
            .char_indices()
            .find_map(|(position, character)| {
                match character {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
                (depth == 0).then_some(first + position)
            })
            .unwrap_or(text.len());
        calls.push(first..end);
    }
    calls
}

/// The text of the arguments of each call of `assert_scales_linearly` in `text`.
fn timing_calls(text: &str) -> Vec<&str> {
    arguments_of(text, "assert_scales_linearly")
        .into_iter()
        .map(|range| &text[range])
        .collect()
}

/// The places in `text` where a timing case of `assert_scales_linearly`
/// measures wall time. The search follows the calls of the functions of the
/// same file.
fn wall_clocks_in_timing_cases(text: &str) -> Vec<String> {
    let functions = functions_of(text);
    let mut found = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut pending: Vec<(String, String)> = timing_calls(text)
        .into_iter()
        .map(|arguments| {
            (
                "a call of assert_scales_linearly".to_string(),
                arguments.to_string(),
            )
        })
        .collect();
    while let Some((place, body)) = pending.pop() {
        if body.contains("Instant::now") {
            found.push(place);
        }
        let words =
            body.split(|character: char| !(character.is_alphanumeric() || character == '_'));
        for word in words {
            if let Some(called) = functions.get(word) {
                if seen.insert(word.to_string()) {
                    pending.push((format!("fn {word}"), called.clone()));
                }
            }
        }
    }
    found.sort();
    found
}

#[test]
fn test_wall_clock_in_a_timing_case_is_found() {
    let text = "fn case_at(size: usize) -> Duration {\n    let started = std::time::Instant::now();\n    started.elapsed()\n}\n\nfn other() {\n    let started = std::time::Instant::now();\n}\n\nfn timing() {\n    assert_scales_linearly(\"case\", 10, |size| helper(size));\n}\n\nfn helper(size: usize) -> Duration {\n    case_at(size)\n}\n";
    assert_eq!(wall_clocks_in_timing_cases(text), ["fn case_at"]);
    let inline = "fn timing() {\n    assert_scales_linearly(\"case\", 10, |size| {\n        let started = std::time::Instant::now();\n        started.elapsed()\n    });\n}\n";
    assert_eq!(
        wall_clocks_in_timing_cases(inline),
        ["a call of assert_scales_linearly"]
    );
}

/// A timing case measures with `Stopwatch`. Wall time changes with the load
/// of the machine, and the ratio check then fails without a regression.
#[test]
fn test_timing_cases_do_not_measure_wall_time() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut wrong = Vec::new();
    for entry in ignore::WalkBuilder::new(root.join("src"))
        .add(root.join("tests"))
        .build()
        .flatten()
    {
        let path = entry.path();
        let own = path.ends_with("index/timing.rs") || path.ends_with("index/timing_tests.rs");
        if own || path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(path).expect("a source file");
        // In `src`, the code before the tests can measure wall time for its output.
        let tests = match text.find("#[cfg(test)]") {
            Some(start) if path.starts_with(root.join("src")) => &text[start..],
            _ => text.as_str(),
        };
        for place in wall_clocks_in_timing_cases(tests) {
            wrong.push(format!(
                "{}: {place}",
                path.strip_prefix(root).unwrap_or(path).display()
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "use `Stopwatch` in place of `Instant`:\n{}",
        wrong.join("\n")
    );
}

#[test]
fn test_stopwatch_outside_a_run_of_one_thread_is_found() {
    let text = "fn case_at(size: usize) -> Duration {\n    let started = Stopwatch::start();\n    started.elapsed()\n}\n\nfn helper(size: usize) -> Duration {\n    case_at(size)\n}\n\n#[test]\n#[ignore = \"timing: run in release\"]\nfn timing_inside() {\n    assert_scales_linearly(\"case\", 10, |size| helper(size));\n    let took = on_one_thread(|| case_at(100));\n}\n\n#[test]\n#[ignore = \"timing: run in release\"]\nfn timing_outside() {\n    assert_scales_linearly(\"case\", 10, |size| helper(size));\n    assert!(helper(100) < LIMIT);\n}\n\n#[test]\nfn untimed() {\n    helper(10);\n}\n\nmacro_rules! timing {\n    ($scenario:ident, $test:ident) => {\n        #[test]\n        #[ignore = \"timing: run in release\"]\n        fn $test() {\n            let (_, took) = $scenario(100);\n        }\n    };\n}\n";
    assert_eq!(
        stopwatches_outside_one_thread(text),
        ["fn $test", "fn timing_outside"]
    );
}

/// The timing tests of `text` that measure with `Stopwatch` outside a run of
/// `assert_scales_linearly` or `on_one_thread`.
fn stopwatches_outside_one_thread(text: &str) -> Vec<String> {
    let functions = functions_of(text);
    let mut measuring: std::collections::HashSet<&str> = functions
        .iter()
        .filter(|(_, body)| body.contains("Stopwatch::start"))
        .map(|(name, _)| *name)
        .collect();
    loop {
        let callers: Vec<&str> = functions
            .iter()
            .filter(|(name, body)| {
                !measuring.contains(*name)
                    && called_names(body).any(|called| measuring.contains(called))
            })
            .map(|(name, _)| *name)
            .collect();
        if callers.is_empty() {
            break;
        }
        measuring.extend(callers);
    }
    let mut timing_tests = Vec::new();
    let mut marked = false;
    for line in text.lines() {
        if line.trim_start().starts_with("#[ignore = \"timing") {
            marked = true;
        } else if let Some(name) = declared_name(line) {
            if marked {
                timing_tests.push(name);
            }
            marked = false;
        }
    }
    let mut found: Vec<String> = timing_tests
        .into_iter()
        .filter(|name| {
            let mut rest = functions[name].clone();
            for function in ["assert_scales_linearly", "on_one_thread"] {
                for range in arguments_of(&rest.clone(), function) {
                    rest.replace_range(range.clone(), &" ".repeat(range.len()));
                }
            }
            rest.contains("Stopwatch::start")
                || called_names(&rest).any(|called| {
                    called != *name && (called.starts_with('$') || measuring.contains(called))
                })
        })
        .map(|name| format!("fn {name}"))
        .collect();
    found.sort();
    found
}

/// The name of the function that `line` declares.
fn declared_name(line: &str) -> Option<&str> {
    let declaration = line
        .trim_start()
        .trim_start_matches("pub(crate) ")
        .trim_start_matches("pub(super) ")
        .trim_start_matches("pub ");
    let rest = declaration.strip_prefix("fn ")?;
    let end = rest.find(['(', '<']).unwrap_or(rest.len());
    Some(&rest[..end])
}

/// The names that `text` calls, with the `$` of a macro variable.
fn called_names(text: &str) -> impl Iterator<Item = &str> {
    text.match_indices('(').filter_map(|(open, _)| {
        let before = &text[..open];
        let start = before
            .rfind(|character: char| {
                !(character.is_alphanumeric() || character == '_' || character == '$')
            })
            .map_or(0, |position| position + 1);
        (start < open).then(|| &text[start..open])
    })
}

/// `Stopwatch` measures the processor time of its thread. Outside a run of one
/// thread, the parallel work of a case is on other threads, and the time
/// misses it.
#[test]
fn test_timing_cases_measure_on_one_thread() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut wrong = Vec::new();
    for entry in ignore::WalkBuilder::new(root.join("src"))
        .add(root.join("tests"))
        .build()
        .flatten()
    {
        let path = entry.path();
        let own = path.ends_with("index/timing.rs") || path.ends_with("index/timing_tests.rs");
        if own || path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(path).expect("a source file");
        for place in stopwatches_outside_one_thread(&text) {
            wrong.push(format!(
                "{}: {place}",
                path.strip_prefix(root).unwrap_or(path).display()
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "measure in `assert_scales_linearly` or `on_one_thread`:\n{}",
        wrong.join("\n")
    );
}

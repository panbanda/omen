use std::path::Path;

/// Return the percentile at the rounded `p / 100 * (len - 1)` index.
///
/// This definition selects the nearest observed value without interpolation.
/// The input must already be sorted. Empty inputs return `T::default()`.
pub fn percentile<T: Copy + Default>(sorted: &[T], p: f64) -> T {
    if sorted.is_empty() {
        return T::default();
    }

    let percentile = p.clamp(0.0, 100.0);
    let index = (percentile / 100.0 * (sorted.len() - 1) as f64).round() as usize;
    sorted[index]
}

/// Return whether a path follows a supported test-file convention.
pub fn is_test_file(path: impl AsRef<Path>) -> bool {
    let path = path.as_ref();
    let normalized = path.to_string_lossy().replace('\\', "/").to_lowercase();
    let parts: Vec<&str> = normalized.split('/').collect();

    if parts.iter().any(|part| {
        matches!(
            *part,
            "test"
                | "tests"
                | "spec"
                | "specs"
                | "__tests__"
                | "__mocks__"
                | "test_helpers"
                | "testdata"
                | "fixtures"
        )
    }) {
        return true;
    }

    let Some(filename) = parts.last() else {
        return false;
    };
    filename.starts_with("test_")
        || filename.contains("_test.")
        || filename.contains("_spec.")
        || (filename.matches('.').count() >= 2
            && filename
                .split('.')
                .rev()
                .nth(1)
                .is_some_and(|segment| segment == "test" || segment == "spec"))
}

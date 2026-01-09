//! Progress reporting utilities using indicatif.
//!
//! Provides consistent progress bars across all analyzers and long-running operations.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

/// Style templates for different progress bar types.
pub mod styles {
    use super::*;

    /// Standard progress bar style for file processing.
    pub fn file_progress() -> ProgressStyle {
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .expect("valid template")
            .progress_chars("#>-")
    }

    /// Progress bar style for operations with known duration.
    pub fn phase_progress() -> ProgressStyle {
        ProgressStyle::default_bar()
            .template("{prefix:.bold.dim} [{bar:30.green/white}] {pos}/{len} {msg}")
            .expect("valid template")
            .progress_chars("=>-")
    }

    /// Spinner style for indeterminate operations.
    pub fn spinner() -> ProgressStyle {
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .expect("valid template")
    }

    /// Progress bar style for multi-phase operations.
    pub fn multi_phase() -> ProgressStyle {
        ProgressStyle::default_bar()
            .template("{prefix:.bold} [{bar:25.cyan/blue}] {pos}/{len} ({eta}) {msg}")
            .expect("valid template")
            .progress_chars("#>-")
    }
}

/// A thread-safe progress tracker for parallel operations.
#[derive(Clone)]
pub struct ProgressTracker {
    bar: ProgressBar,
    counter: Arc<AtomicUsize>,
}

impl ProgressTracker {
    /// Create a new progress tracker with the given total count.
    pub fn new(total: usize, message: &str) -> Self {
        let bar = ProgressBar::new(total as u64);
        bar.set_style(styles::file_progress());
        bar.set_message(message.to_string());

        Self {
            bar,
            counter: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Create a hidden progress tracker (for non-TTY output).
    pub fn hidden(total: usize) -> Self {
        let bar = ProgressBar::hidden();
        bar.set_length(total as u64);

        Self {
            bar,
            counter: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Increment the progress counter by one.
    pub fn inc(&self) {
        self.counter.fetch_add(1, Ordering::Relaxed);
        self.bar.inc(1);
    }

    /// Set the current progress message.
    pub fn set_message(&self, msg: impl Into<String>) {
        self.bar.set_message(msg.into());
    }

    /// Finish the progress bar with a completion message.
    pub fn finish(&self, msg: &str) {
        self.bar.finish_with_message(msg.to_string());
    }

    /// Finish and clear the progress bar.
    pub fn finish_and_clear(&self) {
        self.bar.finish_and_clear();
    }

    /// Get the underlying progress bar for advanced customization.
    pub fn bar(&self) -> &ProgressBar {
        &self.bar
    }

    /// Get the current count.
    pub fn count(&self) -> usize {
        self.counter.load(Ordering::Relaxed)
    }
}

/// Builder for creating progress bars with common configurations.
pub struct ProgressBuilder {
    total: u64,
    message: String,
    prefix: String,
    style: ProgressStyle,
    hidden: bool,
}

impl ProgressBuilder {
    /// Create a new progress builder.
    pub fn new(total: usize) -> Self {
        Self {
            total: total as u64,
            message: String::new(),
            prefix: String::new(),
            style: styles::file_progress(),
            hidden: false,
        }
    }

    /// Set the progress message.
    pub fn message(mut self, msg: impl Into<String>) -> Self {
        self.message = msg.into();
        self
    }

    /// Set the progress prefix.
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    /// Use phase progress style.
    pub fn phase_style(mut self) -> Self {
        self.style = styles::phase_progress();
        self
    }

    /// Use multi-phase style.
    pub fn multi_phase_style(mut self) -> Self {
        self.style = styles::multi_phase();
        self
    }

    /// Use spinner style.
    pub fn spinner_style(mut self) -> Self {
        self.style = styles::spinner();
        self
    }

    /// Make the progress bar hidden.
    pub fn hidden(mut self) -> Self {
        self.hidden = true;
        self
    }

    /// Build the progress bar.
    pub fn build(self) -> ProgressBar {
        let bar = if self.hidden {
            ProgressBar::hidden()
        } else {
            ProgressBar::new(self.total)
        };

        bar.set_style(self.style);
        if !self.message.is_empty() {
            bar.set_message(self.message);
        }
        if !self.prefix.is_empty() {
            bar.set_prefix(self.prefix);
        }

        bar
    }
}

/// Manager for multi-phase operations with multiple progress bars.
pub struct MultiPhaseProgress {
    multi: MultiProgress,
    bars: Vec<ProgressBar>,
}

impl MultiPhaseProgress {
    /// Create a new multi-phase progress manager.
    pub fn new() -> Self {
        Self {
            multi: MultiProgress::new(),
            bars: Vec::new(),
        }
    }

    /// Add a new phase with the given total and message.
    pub fn add_phase(&mut self, total: usize, prefix: &str, message: &str) -> &ProgressBar {
        let bar = ProgressBuilder::new(total)
            .prefix(prefix)
            .message(message)
            .multi_phase_style()
            .build();
        let bar = self.multi.add(bar);
        self.bars.push(bar);
        self.bars.last().unwrap()
    }

    /// Get a phase by index.
    pub fn phase(&self, index: usize) -> Option<&ProgressBar> {
        self.bars.get(index)
    }

    /// Finish all phases and clear.
    pub fn finish_all(&self) {
        for bar in &self.bars {
            bar.finish_and_clear();
        }
    }
}

impl Default for MultiPhaseProgress {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if stderr is a TTY (for deciding whether to show progress bars).
pub fn is_tty() -> bool {
    use std::io::IsTerminal;
    std::io::stderr().is_terminal()
}

/// Create an appropriate progress bar based on TTY status.
pub fn create_progress(total: usize, message: &str) -> ProgressBar {
    if is_tty() {
        ProgressBuilder::new(total).message(message).build()
    } else {
        ProgressBar::hidden()
    }
}

/// Create a spinner for indeterminate operations.
pub fn create_spinner(message: &str) -> ProgressBar {
    if is_tty() {
        let bar = ProgressBar::new_spinner();
        bar.set_style(styles::spinner());
        bar.set_message(message.to_string());
        bar.enable_steady_tick(std::time::Duration::from_millis(100));
        bar
    } else {
        ProgressBar::hidden()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_tracker_new() {
        let tracker = ProgressTracker::new(100, "Testing");
        assert_eq!(tracker.count(), 0);
    }

    #[test]
    fn test_progress_tracker_inc() {
        let tracker = ProgressTracker::new(100, "Testing");
        tracker.inc();
        tracker.inc();
        assert_eq!(tracker.count(), 2);
    }

    #[test]
    fn test_progress_tracker_hidden() {
        let tracker = ProgressTracker::hidden(100);
        tracker.inc();
        assert_eq!(tracker.count(), 1);
    }

    #[test]
    fn test_progress_builder() {
        let bar = ProgressBuilder::new(50)
            .message("Processing")
            .prefix("Phase 1")
            .build();
        assert_eq!(bar.length(), Some(50));
    }

    #[test]
    fn test_progress_builder_hidden() {
        let bar = ProgressBuilder::new(50).hidden().build();
        // Hidden bars still track progress
        bar.inc(1);
        assert_eq!(bar.position(), 1);
    }

    #[test]
    fn test_multi_phase_progress() {
        let mut multi = MultiPhaseProgress::new();
        multi.add_phase(10, "Phase 1", "Scanning");
        multi.add_phase(20, "Phase 2", "Processing");

        assert!(multi.phase(0).is_some());
        assert!(multi.phase(1).is_some());
        assert!(multi.phase(2).is_none());
    }

    #[test]
    fn test_create_progress_hidden_in_tests() {
        // In test environment, is_tty() is usually false
        let bar = create_progress(100, "Test");
        bar.inc(1);
        assert_eq!(bar.position(), 1);
    }

    #[test]
    fn test_create_spinner() {
        let spinner = create_spinner("Loading...");
        spinner.finish();
    }

    #[test]
    fn test_styles_dont_panic() {
        let _ = styles::file_progress();
        let _ = styles::phase_progress();
        let _ = styles::spinner();
        let _ = styles::multi_phase();
    }
}

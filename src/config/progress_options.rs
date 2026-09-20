//! Progress Bar Config

use std::{fmt::Write, io::Write as _, time::Duration};

use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use bytesize::ByteSize;
use indicatif::{HumanDuration, MultiProgress, ProgressBar, ProgressState, ProgressStyle};

use clap::Parser;
use conflate::Merge;
use jiff::SignedDuration;
use log::info;

use serde::{Deserialize, Serialize};
use serde_with::{DisplayFromStr, serde_as};

use rustic_core::{Progress, ProgressBars, ProgressType, RusticProgress, format_upload_stats};

/// Returns the global `MultiProgress` instance used by all interactive progress bars.
///
/// Live bars: logs print above them (see `logging.rs`). After the last bar
/// finishes, logs use the console appender so the backup summary is a real line.
pub fn multi_progress() -> &'static MultiProgress {
    static MP: OnceLock<MultiProgress> = OnceLock::new();
    MP.get_or_init(|| {
        // Overwrite-in-place (`move_cursor`) pads to the claimed tty width and
        // does not clear to end-of-line, so a shorter log line leaves the tail
        // of the status bar (`GiB added`) on the Files/Dirs summary.
        MultiProgress::new()
    })
}

static LIVE_INTERACTIVE_BARS: AtomicUsize = AtomicUsize::new(0);
static NEED_PROGRESS_BREAK: AtomicBool = AtomicBool::new(false);

/// True while at least one interactive progress bar is on screen.
///
/// `MultiProgress::println` pads to the terminal width and moves the cursor
/// instead of writing newlines, so a PTY log capture can drop the backup
/// summary. Write ordinary console lines when this is false.
pub(crate) fn has_live_progress_bars() -> bool {
    LIVE_INTERACTIVE_BARS.load(Ordering::Relaxed) > 0
}

/// Console logs go through `MultiProgress::println` only while a bar is on screen.
///
/// After the last bar finishes this is false even if stderr is a TTY, so the
/// backup Files/snapshot summary is a real newline-terminated line.
pub(crate) fn log_above_progress_bars() -> bool {
    !multi_progress().is_hidden() && has_live_progress_bars()
}

/// Clear leftover bar cells before the next console log.
///
/// Called from the log appender when bars have just gone away, not at every
/// bar finish — otherwise index/parent counters flash a blank line between them.
/// `\r` + erase-line parks the cursor at column 0 without inserting a blank row.
pub(crate) fn take_progress_break() {
    if !NEED_PROGRESS_BREAK.swap(false, Ordering::Relaxed) {
        return;
    }
    let _ = multi_progress().clear();
    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(b"\r\x1b[K");
    let _ = stderr.flush();
}

/// Visible width of stderr, for sizing progress templates so they do not wrap.
fn stderr_width() -> usize {
    ioctl_stderr_cols()
        .or_else(|| {
            std::env::var("COLUMNS")
                .ok()
                .and_then(|cols| cols.parse().ok())
        })
        .filter(|&cols| cols >= 40)
        .unwrap_or(80)
}

#[cfg(unix)]
fn ioctl_stderr_cols() -> Option<usize> {
    let mut size = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // Safety: `TIOCGWINSZ` writes a `winsize` for this fd.
    let ok = unsafe { libc::ioctl(libc::STDERR_FILENO, libc::TIOCGWINSZ, &mut size) == 0 };
    (ok && size.ws_col >= 40).then_some(usize::from(size.ws_col))
}

#[cfg(not(unix))]
fn ioctl_stderr_cols() -> Option<usize> {
    None
}

const ELAPSED_WIDTH: usize = 11;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BytesStats {
    Bare,
    WithTotal,
    WithEta,
}

impl BytesStats {
    fn width(self) -> usize {
        match self {
            Self::Bare => 1 + 10 + 1 + 12,
            Self::WithTotal => 1 + 10 + 1 + 10 + 1 + 12,
            Self::WithEta => 1 + 10 + 1 + 10 + 1 + 12 + 13,
        }
    }

    #[allow(clippy::literal_string_with_formatting_args)]
    fn suffix(self) -> &'static str {
        match self {
            Self::Bare => " {bytes:>10} {bytes_per_sec:<12}",
            Self::WithTotal => " {bytes:>10}/{total_bytes:<10} {bytes_per_sec:<12}",
            Self::WithEta => " {bytes:>10}/{total_bytes:<10} {bytes_per_sec:<12} (ETA {my_eta})",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BytesLayout {
    prefix: usize,
    bar: usize,
    stats: BytesStats,
}

impl BytesLayout {
    fn total_width(self) -> usize {
        ELAPSED_WIDTH + self.prefix + 1 + self.bar + self.stats.width()
    }

    fn template(self) -> String {
        format!(
            "[{{elapsed_precise}}] {{prefix:{}}} {{bar:{}.cyan/blue}}{}",
            self.prefix,
            self.bar,
            self.stats.suffix()
        )
    }
}

fn bytes_layout(width: usize, with_length: bool) -> BytesLayout {
    let budget = width.max(40).saturating_sub(1);
    let try_fit = |prefix: usize, stats: BytesStats, min_bar: usize| -> Option<BytesLayout> {
        let used = ELAPSED_WIDTH + prefix + 1 + stats.width();
        let bar = budget.saturating_sub(used);
        (bar >= min_bar).then_some(BytesLayout {
            prefix,
            bar: bar.min(40),
            stats,
        })
    };

    let layout = if with_length {
        try_fit(14, BytesStats::WithEta, 10)
            .or_else(|| try_fit(14, BytesStats::WithTotal, 8))
            .or_else(|| try_fit(10, BytesStats::WithTotal, 6))
            .or_else(|| try_fit(10, BytesStats::Bare, 4))
            .unwrap_or(BytesLayout {
                prefix: 8,
                bar: 4,
                stats: BytesStats::Bare,
            })
    } else {
        try_fit(14, BytesStats::Bare, 8)
            .or_else(|| try_fit(10, BytesStats::Bare, 4))
            .unwrap_or(BytesLayout {
                prefix: 8,
                bar: 4,
                stats: BytesStats::Bare,
            })
    };
    debug_assert!(layout.total_width() <= width.max(40));
    layout
}

fn prefix_width(width: usize, extra: usize) -> usize {
    let budget = width.max(40).saturating_sub(1);
    let leftover = budget.saturating_sub(ELAPSED_WIDTH + extra);
    leftover.clamp(8, 14)
}

mod constants {
    use std::time::Duration;

    pub(super) const DEFAULT_INTERVAL: Duration = Duration::from_millis(100);
    pub(super) const DEFAULT_LOG_INTERVAL: Duration = Duration::from_secs(10);
}

/// Progress Bar Config
#[serde_as]
#[derive(Default, Debug, Parser, Clone, Copy, Deserialize, Serialize, Merge)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct ProgressOptions {
    /// Don't show any progress bar
    #[clap(long, global = true, env = "RUSTIC_NO_PROGRESS")]
    #[merge(strategy=conflate::bool::overwrite_false)]
    pub no_progress: bool,

    /// Write progress as newline-delimited JSON
    #[clap(
        long,
        global = true,
        env = "RUSTIC_JSON_PROGRESS",
        conflicts_with = "no_progress"
    )]
    #[merge(strategy=conflate::bool::overwrite_false)]
    pub json_progress: bool,

    /// Interval to update progress bars (default: 100ms)
    #[clap(
        long,
        global = true,
        env = "RUSTIC_PROGRESS_INTERVAL",
        value_name = "DURATION",
        conflicts_with = "no_progress"
    )]
    #[serde_as(as = "Option<DisplayFromStr>")]
    #[merge(strategy=conflate::option::overwrite_none)]
    pub progress_interval: Option<SignedDuration>,
}

impl ProgressOptions {
    /// Get interval for interactive progress bars
    fn interactive_interval(&self) -> Duration {
        self.progress_interval
            .map_or(constants::DEFAULT_INTERVAL, |i| {
                i.try_into().expect("negative durations are not allowed")
            })
    }

    /// Get interval for non-interactive logging
    fn log_interval(&self) -> Duration {
        self.progress_interval
            .map_or(constants::DEFAULT_LOG_INTERVAL, |i| {
                i.try_into().expect("negative durations are not allowed")
            })
    }

    /// Factory Pattern: Create progress indicator based on terminal capabilities
    ///
    /// * `Hidden`: If --no-progress is set.
    /// * `Interactive`: If running in a TTY.
    /// * `NonInteractive`: If running in a pipe/service (logs to stderr).
    fn create_progress(&self, prefix: &str, kind: ProgressType) -> Progress {
        if self.no_progress {
            return Progress::hidden();
        }

        let interval = self.log_interval();
        if self.json_progress {
            return if interval > Duration::ZERO
                && matches!(kind, ProgressType::Bytes | ProgressType::Status)
            {
                Progress::new(JsonProgress::new(prefix, interval, kind))
            } else {
                Progress::hidden()
            };
        }

        if std::io::stderr().is_terminal() {
            Progress::new(InteractiveProgress::new(
                prefix,
                kind,
                self.interactive_interval(),
            ))
        } else {
            if interval > Duration::ZERO {
                Progress::new(NonInteractiveProgress::new(prefix, interval, kind))
            } else {
                Progress::hidden()
            }
        }
    }
}

impl ProgressBars for ProgressOptions {
    fn progress(&self, progress_kind: ProgressType, prefix: &str) -> Progress {
        self.create_progress(prefix, progress_kind)
    }
}

// ================ Interactive ================
/// Wrapper around `indicatif::ProgressBar` for interactive terminal usage
#[derive(Debug)]
pub struct InteractiveProgress {
    bar: ProgressBar,
    kind: ProgressType,
    tick_interval: Duration,
    shown: AtomicBool,
    live: AtomicBool,
}

impl InteractiveProgress {
    fn new(prefix: &str, kind: ProgressType, tick_interval: Duration) -> Self {
        let style = Self::initial_style(kind);
        let bar = ProgressBar::new(0).with_style(style);
        bar.set_prefix(prefix.to_string());
        // Empty prefix is used for index/parent scans; a 40-column bar with no
        // label just flashes and makes the start of backup look like stutter.
        let shown = !matches!(kind, ProgressType::Status) && !prefix.is_empty();
        if shown {
            let bar = multi_progress().add(bar);
            bar.enable_steady_tick(tick_interval);
            let this = Self {
                bar,
                kind,
                tick_interval,
                shown: AtomicBool::new(true),
                live: AtomicBool::new(false),
            };
            this.register_live();
            return this;
        }
        Self {
            bar,
            kind,
            tick_interval,
            shown: AtomicBool::new(false),
            live: AtomicBool::new(false),
        }
    }

    fn register_live(&self) {
        if self
            .live
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            _ = LIVE_INTERACTIVE_BARS.fetch_add(1, Ordering::Relaxed);
            NEED_PROGRESS_BREAK.store(false, Ordering::Relaxed);
        }
    }

    fn unregister_live(&self) {
        if self
            .live
            .compare_exchange(true, false, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            let prev = LIVE_INTERACTIVE_BARS.fetch_sub(1, Ordering::Relaxed);
            if prev == 1 {
                NEED_PROGRESS_BREAK.store(true, Ordering::Relaxed);
            }
        }
    }

    fn ensure_shown(&self) {
        if self
            .shown
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            let _ = multi_progress().add(self.bar.clone());
            self.bar.enable_steady_tick(self.tick_interval);
            self.register_live();
        }
    }

    fn initial_style(kind: ProgressType) -> ProgressStyle {
        Self::style_for(kind, false)
    }

    fn style_with_length(kind: ProgressType) -> ProgressStyle {
        Self::style_for(kind, true)
    }

    #[allow(clippy::literal_string_with_formatting_args)]
    fn style_for(kind: ProgressType, with_length: bool) -> ProgressStyle {
        let width = stderr_width();
        let template = match kind {
            ProgressType::Spinner => {
                let prefix = prefix_width(width, 2);
                format!("[{{elapsed_precise}}] {{prefix:{prefix}}} {{spinner}}")
            }
            ProgressType::Counter => {
                let layout = bytes_layout(width, with_length);
                if with_length {
                    format!(
                        "[{{elapsed_precise}}] {{prefix:{}}} {{bar:{}.cyan/blue}} {{pos:>10}}/{{len:10}}",
                        layout.prefix, layout.bar
                    )
                } else {
                    format!(
                        "[{{elapsed_precise}}] {{prefix:{}}} {{bar:{}.cyan/blue}} {{pos:>10}}",
                        layout.prefix, layout.bar
                    )
                }
            }
            ProgressType::Bytes => bytes_layout(width, with_length).template(),
            ProgressType::Status => {
                let prefix = prefix_width(width, 24);
                format!("[{{elapsed_precise}}] {{prefix:{prefix}}} {{msg}}")
            }
        };
        let mut style = ProgressStyle::default_bar();
        if matches!(kind, ProgressType::Bytes) && with_length {
            style = style.with_key("my_eta", |s: &ProgressState, w: &mut dyn Write| {
                let _ = match (s.pos(), s.len()) {
                    (pos, Some(len)) if pos != 0 && len > pos => {
                        let eta_secs = s.elapsed().as_secs() * (len - pos) / pos;
                        write!(w, "{:#}", HumanDuration(Duration::from_secs(eta_secs)))
                    }
                    _ => write!(w, "-"),
                };
            });
        }
        style.template(&template).unwrap()
    }
}

impl RusticProgress for InteractiveProgress {
    fn is_hidden(&self) -> bool {
        false
    }

    fn set_length(&self, len: u64) {
        if matches!(self.kind, ProgressType::Bytes | ProgressType::Counter) {
            self.bar.set_style(Self::style_with_length(self.kind));
        }
        self.bar.set_length(len);
    }

    fn set_title(&self, title: &str) {
        self.bar.set_prefix(title.to_string());
        if !title.is_empty() {
            self.ensure_shown();
        }
    }

    fn inc(&self, inc: u64) {
        self.bar.inc(inc);
    }

    fn finish(&self) {
        if matches!(self.kind, ProgressType::Status) && !self.shown.load(Ordering::Relaxed) {
            return;
        }
        self.bar.finish_and_clear();
        self.unregister_live();
    }

    fn set_message(&self, msg: &str) {
        self.bar.set_message(msg.to_string());
        if matches!(self.kind, ProgressType::Status) {
            self.ensure_shown();
        }
    }
}

impl Drop for InteractiveProgress {
    fn drop(&mut self) {
        self.unregister_live();
    }
}

// ================ Non-Interactive ================

/// Store state for non-interactive progress
#[derive(Debug)]
struct NonInteractiveState {
    prefix: String,
    position: u64,
    length: Option<u64>,
    last_log: Instant,
    error_count: u64,
    message: String,
    files_new: Option<u64>,
    files_changed: Option<u64>,
    bytes_added: Option<u64>,
}

impl NonInteractiveState {
    fn progress_text(&self, kind: ProgressType) -> String {
        if matches!(kind, ProgressType::Status) && !self.message.is_empty() {
            return self.message.clone();
        }
        let format_value = |value| match kind {
            ProgressType::Bytes => ByteSize(value).to_string(),
            ProgressType::Counter | ProgressType::Spinner | ProgressType::Status => {
                value.to_string()
            }
        };

        self.length.map_or_else(
            || format_value(self.position),
            |len| format!("{} / {}", format_value(self.position), format_value(len)),
        )
    }

    fn should_log(&self, interval: Duration) -> bool {
        self.last_log.elapsed() >= interval
    }

    fn mark_logged(&mut self) {
        self.last_log = Instant::now();
    }
}

/// Periodic logger for non-interactive environments (i.e. systemd)
/// Implemented thread-safe and decouples logging logic from indicatif
#[derive(Clone, Debug)]
pub struct NonInteractiveProgress {
    state: Arc<Mutex<NonInteractiveState>>,
    start: Instant,
    interval: Duration,
    kind: ProgressType,
}

impl NonInteractiveProgress {
    fn new(prefix: &str, interval: Duration, kind: ProgressType) -> Self {
        let now = Instant::now();
        Self {
            state: Arc::new(Mutex::new(NonInteractiveState {
                prefix: prefix.to_string(),
                position: 0,
                length: None,
                last_log: now,
                error_count: 0,
                message: String::new(),
                files_new: None,
                files_changed: None,
                bytes_added: None,
            })),
            start: now,
            interval,
            kind,
        }
    }

    fn format_value(&self, value: u64) -> String {
        match self.kind {
            ProgressType::Bytes => ByteSize(value).to_string(), // delegate bytesize handling
            ProgressType::Counter | ProgressType::Spinner | ProgressType::Status => {
                value.to_string()
            }
        }
    }

    fn log_progress(&self, state: &NonInteractiveState) {
        info!("{}: {}", state.prefix, state.progress_text(self.kind));
    }
}

impl RusticProgress for NonInteractiveProgress {
    fn is_hidden(&self) -> bool {
        false
    }

    fn set_length(&self, len: u64) {
        if let Ok(mut state) = self.state.lock() {
            state.length = Some(len);
        }
    }

    fn set_title(&self, title: &str) {
        if let Ok(mut state) = self.state.lock() {
            state.prefix = title.to_string();
        }
    }

    fn inc(&self, inc: u64) {
        if let Ok(mut state) = self.state.lock() {
            state.position += inc;

            if state.should_log(self.interval) {
                self.log_progress(&state);
                state.mark_logged();
            }
        }
    }

    fn finish(&self) {
        let Ok(state) = self.state.lock() else {
            return;
        };

        if matches!(self.kind, ProgressType::Status) {
            if !state.message.is_empty() {
                info!(
                    "{}: {} done in {:.2?}",
                    state.prefix,
                    state.message,
                    self.start.elapsed()
                );
            }
            return;
        }

        info!(
            "{}: {} done in {:.2?}",
            state.prefix,
            self.format_value(state.position),
            self.start.elapsed()
        );
    }

    fn set_message(&self, msg: &str) {
        if let Ok(mut state) = self.state.lock() {
            state.message = msg.to_string();
            if state.should_log(self.interval) {
                self.log_progress(&state);
                state.mark_logged();
            }
        }
    }
}

// ================ JSON ================

/// Periodic JSON lines progress for machine-readable consumers
#[derive(Clone, Debug)]
pub struct JsonProgress {
    state: Arc<Mutex<NonInteractiveState>>,
    start: Instant,
    interval: Duration,
    kind: ProgressType,
}

#[derive(Serialize)]
struct JsonProgressStatus {
    message_type: &'static str,
    seconds_elapsed: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    seconds_remaining: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    percent_done: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytes_done: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    files_new: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    files_changed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytes_added: Option<u64>,
    error_count: u64,
}

#[derive(Serialize)]
struct JsonErrorMessage {
    message: String,
}

#[derive(Serialize)]
struct JsonError {
    message_type: &'static str,
    error: JsonErrorMessage,
    during: String,
    item: String,
}

impl JsonProgress {
    fn new(prefix: &str, interval: Duration, kind: ProgressType) -> Self {
        let now = Instant::now();
        Self {
            state: Arc::new(Mutex::new(NonInteractiveState {
                prefix: prefix.to_string(),
                position: 0,
                length: None,
                last_log: now,
                error_count: 0,
                message: String::new(),
                files_new: None,
                files_changed: None,
                bytes_added: None,
            })),
            start: now,
            interval,
            kind,
        }
    }

    fn log_progress(&self, state: &NonInteractiveState) {
        let is_bytes = matches!(self.kind, ProgressType::Bytes);
        let elapsed = self.start.elapsed().as_secs();
        let percent_done = state
            .length
            .filter(|len| *len > 0)
            .map(|len| (state.position as f64 / len as f64).min(1.0));
        let seconds_remaining = match (state.position, state.length) {
            (position, Some(len)) if position > 0 && len > position => {
                Some(elapsed.saturating_mul(len - position) / position)
            }
            _ => None,
        };

        let status = JsonProgressStatus {
            message_type: "status",
            seconds_elapsed: elapsed,
            seconds_remaining,
            percent_done,
            total_bytes: is_bytes.then_some(state.length).flatten(),
            bytes_done: is_bytes.then_some(state.position),
            files_new: state.files_new,
            files_changed: state.files_changed,
            bytes_added: state.bytes_added,
            error_count: state.error_count,
        };

        let mut stdout = std::io::stdout().lock();
        _ = serde_json::to_writer(&mut stdout, &status);
        _ = writeln!(stdout);
    }
}

impl RusticProgress for JsonProgress {
    fn is_hidden(&self) -> bool {
        false
    }

    fn set_length(&self, len: u64) {
        if let Ok(mut state) = self.state.lock() {
            state.length = Some(len);
            self.log_progress(&state);
            state.mark_logged();
        }
    }

    fn set_title(&self, title: &str) {
        if let Ok(mut state) = self.state.lock() {
            state.prefix = title.to_string();
        }
    }

    fn inc(&self, inc: u64) {
        if let Ok(mut state) = self.state.lock() {
            state.position += inc;

            if state.should_log(self.interval) {
                self.log_progress(&state);
                state.mark_logged();
            }
        }
    }

    fn finish(&self) {
        let Ok(state) = self.state.lock() else {
            return;
        };
        if matches!(self.kind, ProgressType::Status)
            && state.files_new.is_none()
            && state.files_changed.is_none()
            && state.bytes_added.is_none()
        {
            return;
        }

        self.log_progress(&state);
    }

    fn set_upload_stats(&self, files_new: u64, files_changed: u64, bytes_added: u64) {
        if let Ok(mut state) = self.state.lock() {
            state.files_new = Some(files_new);
            state.files_changed = Some(files_changed);
            state.bytes_added = Some(bytes_added);
            state.message = format_upload_stats(files_new, files_changed, bytes_added);
            if state.should_log(self.interval) {
                self.log_progress(&state);
                state.mark_logged();
            }
        }
    }

    fn error(&self, item: Option<&str>, during: &str, message: &str) {
        if let Ok(mut state) = self.state.lock() {
            state.error_count += 1;
        }

        let error = JsonError {
            message_type: "error",
            error: JsonErrorMessage {
                message: message.to_string(),
            },
            during: during.to_string(),
            item: item.unwrap_or_default().to_string(),
        };

        let mut stderr = std::io::stderr().lock();
        _ = serde_json::to_writer(&mut stderr, &error);
        _ = writeln!(stderr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, PoisonError};

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn lock_tests() -> MutexGuard<'static, ()> {
        TEST_LOCK.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn live_count() -> usize {
        LIVE_INTERACTIVE_BARS.load(Ordering::Relaxed)
    }

    #[test]
    fn live_bar_count_tracks_show_and_finish() {
        let _lock = lock_tests();
        let before = live_count();
        let p = InteractiveProgress::new(
            "backing up...",
            ProgressType::Bytes,
            Duration::from_millis(100),
        );
        assert!(has_live_progress_bars());
        assert_eq!(live_count(), before + 1);
        p.finish();
        assert_eq!(live_count(), before);
        p.finish();
        assert_eq!(live_count(), before);
    }

    #[test]
    fn live_bar_count_drops_without_finish() {
        let _lock = lock_tests();
        let before = live_count();
        {
            let _p = InteractiveProgress::new(
                "backing up...",
                ProgressType::Bytes,
                Duration::from_millis(100),
            );
            assert_eq!(live_count(), before + 1);
        }
        assert_eq!(live_count(), before);
    }

    #[test]
    fn empty_prefix_counter_is_not_shown() {
        let _lock = lock_tests();
        let before = live_count();
        let p = InteractiveProgress::new("", ProgressType::Counter, Duration::from_millis(100));
        assert_eq!(live_count(), before);
        p.finish();
        assert_eq!(live_count(), before);
    }

    #[test]
    fn last_bar_defers_line_break_until_taken() {
        let _lock = lock_tests();
        NEED_PROGRESS_BREAK.store(false, Ordering::Relaxed);
        let p = InteractiveProgress::new(
            "backing up...",
            ProgressType::Bytes,
            Duration::from_millis(100),
        );
        assert!(!NEED_PROGRESS_BREAK.load(Ordering::Relaxed));
        p.finish();
        assert!(NEED_PROGRESS_BREAK.load(Ordering::Relaxed));
        take_progress_break();
        assert!(!NEED_PROGRESS_BREAK.load(Ordering::Relaxed));
    }

    #[test]
    fn hidden_status_bar_is_not_live_until_shown() {
        let _lock = lock_tests();
        let before = live_count();
        let p = InteractiveProgress::new(
            "uploading",
            ProgressType::Status,
            Duration::from_millis(100),
        );
        assert_eq!(live_count(), before);
        p.set_message("1 new  0 changed  0 B added");
        assert_eq!(live_count(), before + 1);
        p.finish();
        assert_eq!(live_count(), before);
    }

    /// Draw target that is never hidden, unlike a piped stderr in `cargo test`.
    #[derive(Debug)]
    struct VisibleTerm;

    impl indicatif::TermLike for VisibleTerm {
        fn width(&self) -> u16 {
            80
        }

        fn move_cursor_up(&self, _: usize) -> std::io::Result<()> {
            Ok(())
        }

        fn move_cursor_down(&self, _: usize) -> std::io::Result<()> {
            Ok(())
        }

        fn move_cursor_right(&self, _: usize) -> std::io::Result<()> {
            Ok(())
        }

        fn move_cursor_left(&self, _: usize) -> std::io::Result<()> {
            Ok(())
        }

        fn write_line(&self, _: &str) -> std::io::Result<()> {
            Ok(())
        }

        fn write_str(&self, _: &str) -> std::io::Result<()> {
            Ok(())
        }

        fn clear_line(&self) -> std::io::Result<()> {
            Ok(())
        }

        fn flush(&self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct RestoreStderrTarget;

    impl Drop for RestoreStderrTarget {
        fn drop(&mut self) {
            multi_progress().set_draw_target(indicatif::ProgressDrawTarget::stderr());
        }
    }

    #[test]
    fn backup_summary_does_not_use_progress_println_after_finish() {
        let _lock = lock_tests();
        let _restore = RestoreStderrTarget;
        multi_progress().set_draw_target(indicatif::ProgressDrawTarget::term_like(Box::new(
            VisibleTerm,
        )));

        assert!(
            !multi_progress().is_hidden(),
            "test requires a visible MultiProgress"
        );

        let p = InteractiveProgress::new(
            "backing up...",
            ProgressType::Bytes,
            Duration::from_millis(100),
        );
        assert!(
            log_above_progress_bars(),
            "live bars on a TTY must print above the bar"
        );

        p.finish();
        assert!(
            !log_above_progress_bars(),
            "Files/snapshot summary must not use MultiProgress::println after the bar finishes"
        );
    }

    #[test]
    fn bytes_layout_fits_typical_consoles() {
        for width in [80, 100, 120, 160] {
            for with_length in [false, true] {
                let layout = bytes_layout(width, with_length);
                assert!(
                    layout.total_width() <= width,
                    "width={width} with_length={with_length}: {} > {width} ({layout:?})",
                    layout.total_width()
                );
                assert!(layout.bar >= 4, "bar too small: {layout:?}");
            }
        }
        assert!(bytes_layout(120, true).bar > bytes_layout(80, true).bar);
        assert_eq!(bytes_layout(80, true).stats, BytesStats::WithTotal);
        assert_eq!(bytes_layout(120, true).stats, BytesStats::WithEta);
    }

    #[test]
    fn bytes_template_is_valid_indicatif_style() {
        for width in [80, 120] {
            let template = bytes_layout(width, true).template();
            _ = ProgressStyle::default_bar()
                .with_key("my_eta", |_: &ProgressState, w: &mut dyn Write| {
                    let _ = write!(w, "-");
                })
                .template(&template)
                .expect(&template);
        }
    }
}

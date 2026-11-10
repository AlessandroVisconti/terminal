//! Something to collect when fbterm crashes or cannot start (Windows release builds and apps
//! started from the macOS Finder have no visible stderr).
//!
//! - The run log `fbterm.log` in the config dir gets everything stderr shows, without colors;
//!   the log of the run before is kept as `fbterm.previous.log`.
//! - A panic, or `daemon.run()` failing, appends a report to `crash.log` (moved aside to
//!   `crash.previous.log` once it is over [`MAX_CRASH_LOG`]) and creates the marker
//!   `crash.pending`, which the next launch consumes to say so once in the window banner.
//! - A crash with a known cause ([`Cause`]) gets a `hint:` line in its report, and the marker
//!   names the cause, so the banner says what happened and what to try.
//!
//! Nothing here ever fails start or panics: every I/O error is ignored.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use fbterm_config::Paths;
use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

/// Size above which `crash.log` is moved to `crash.previous.log` before the next report.
pub const MAX_CRASH_LOG: u64 = 256 * 1024;

/// Log at INFO to stderr (as always) and to the run log, when it can be opened.
///
/// Libraries that log through the `log` crate (iced, wgpu) reach the same output through
/// `tracing-log`'s bridge, which `init` installs. That is how the run log names the graphics
/// adapter, backend and surface format iced picked, which a GPU crash report needs.
pub fn init_logging(paths: &Paths) {
    let file =
        open_run_log(paths).map(|f| fmt::layer().with_ansi(false).with_writer(Mutex::new(f)));
    tracing_subscriber::registry()
        .with(log_filter())
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(file)
        .init();
}

/// INFO for everything, except iced's window layer: it dumps every window's attributes,
/// title included, at INFO.
fn log_filter() -> Targets {
    Targets::new()
        .with_default(LevelFilter::INFO)
        .with_target("iced_winit", LevelFilter::WARN)
}

/// Start a new run log, keeping the previous one as `fbterm.previous.log` (replacing an
/// older one). `None` when the file cannot be created.
pub fn open_run_log(paths: &Paths) -> Option<File> {
    let _ = fs::create_dir_all(&paths.config_dir);
    let log = paths.run_log();
    if log.exists() {
        let _ = fs::rename(&log, paths.previous_run_log());
    }
    File::create(&log).ok()
}

/// On every panic, append a report to `crash.log`, then let the previous (default) hook print
/// to stderr as before.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("(no message)");
        let location = info.location().map(|l| l.to_string());
        let thread = std::thread::current();
        let report = Report {
            time: now_rfc3339(),
            thread: thread.name().unwrap_or("(unnamed)").to_owned(),
            what: format!("panic: {message}"),
            location,
            cause: Cause::of(message),
            backtrace: std::backtrace::Backtrace::force_capture().to_string(),
        };
        write_report(&crate::app::paths(), &report);
        previous(info);
    }));
}

/// `daemon.run()` failed: log it, write a report and, on Windows (no console), say where it
/// is in a message box.
pub fn start_failed(error: &str) {
    tracing::error!("fbterm could not start: {error}");
    let paths = crate::app::paths();
    let report = Report {
        time: now_rfc3339(),
        thread: std::thread::current()
            .name()
            .unwrap_or("(unnamed)")
            .to_owned(),
        what: format!("fbterm could not start: {error}"),
        location: None,
        cause: Cause::of(error),
        backtrace: std::backtrace::Backtrace::force_capture().to_string(),
    };
    let path = write_report(&paths, &report);
    #[cfg(windows)]
    {
        let text = format!(
            "fbterm could not start: {error}. A report was saved to {}.",
            path.display()
        );
        let _ = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("fbterm")
            .set_description(text)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    }
    #[cfg(not(windows))]
    let _ = path;
}

/// The current UTC time in RFC 3339, to the second.
fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// A crash cause fbterm recognizes from its message, and can say something useful about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    /// The GPU device went away under the renderer (wgpu's `Parent device is lost`): a driver
    /// reset or update, or a backend this GPU and driver cannot run. wgpu panics on it while
    /// presenting a frame, so iced cannot recover.
    GpuLost,
}

impl Cause {
    /// The cause a panic or start-failure message names, if fbterm knows it.
    pub fn of(message: &str) -> Option<Self> {
        message
            .to_ascii_lowercase()
            .contains("device is lost")
            .then_some(Self::GpuLost)
    }

    /// The `hint:` line of the report.
    fn report_hint(self) -> &'static str {
        match self {
            Self::GpuLost => {
                "the graphics device was lost (a driver reset, or a graphics backend this GPU \
                 cannot run); the run log names the adapter and backend in use"
            }
        }
    }

    /// What `crash.pending` holds for this cause.
    fn marker(self) -> &'static str {
        match self {
            Self::GpuLost => "gpu-lost",
        }
    }

    /// The cause a `crash.pending` marker names; an empty or unknown one names none.
    fn from_marker(marker: &str) -> Option<Self> {
        (marker.trim() == "gpu-lost").then_some(Self::GpuLost)
    }

    /// The banner line for this cause, naming the report at `log`.
    fn banner(self, log: &std::path::Path) -> String {
        match self {
            Self::GpuLost => format!(
                "fbterm stopped last time because the graphics device was lost. If it happens \
                 again, update the graphics driver{}. The report is in {}.",
                OTHER_BACKENDS,
                log.display()
            ),
        }
    }
}

/// How to ask for another graphics backend, where there is one to ask for (`WGPU_BACKEND`,
/// read by iced).
#[cfg(windows)]
const OTHER_BACKENDS: &str = ", or start fbterm with WGPU_BACKEND set to dx12, vulkan or gl";
#[cfg(all(unix, not(target_os = "macos")))]
const OTHER_BACKENDS: &str = ", or start fbterm with WGPU_BACKEND set to vulkan or gl";
#[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
const OTHER_BACKENDS: &str = "";

/// One entry of `crash.log`.
#[derive(Debug, Clone)]
pub struct Report {
    /// UTC time in RFC 3339.
    pub time: String,
    /// Name of the thread it happened on.
    pub thread: String,
    /// `panic: <message>` or `fbterm could not start: <error>`.
    pub what: String,
    /// Source location of a panic.
    pub location: Option<String>,
    /// The cause, when fbterm recognizes it.
    pub cause: Option<Cause>,
    /// The captured backtrace.
    pub backtrace: String,
}

impl Report {
    /// The report as written: a separator line, then one fact per line, the backtrace last.
    pub fn text(&self) -> String {
        let mut out = String::from("==== fbterm crash report ====\n");
        out.push_str(&format!("time: {}\n", self.time));
        out.push_str(&format!("fbterm {}\n", env!("CARGO_PKG_VERSION")));
        out.push_str(&format!(
            "os: {} {}\n",
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
        out.push_str(&format!("thread: {}\n", self.thread));
        out.push_str(&self.what);
        out.push('\n');
        if let Some(at) = &self.location {
            out.push_str(&format!("at: {at}\n"));
        }
        if let Some(cause) = self.cause {
            out.push_str(&format!("hint: {}\n", cause.report_hint()));
        }
        out.push_str("backtrace:\n");
        out.push_str(&self.backtrace);
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
        out
    }
}

/// Append `report` to `crash.log` (creating the config dir, moving a log over
/// [`MAX_CRASH_LOG`] aside first) and mark it pending for the next launch, with its cause.
/// Returns the path of `crash.log`.
pub fn write_report(paths: &Paths, report: &Report) -> PathBuf {
    append_report(paths, &report.text(), report.cause)
}

/// [`write_report`] for an already formatted report.
fn append_report(paths: &Paths, text: &str, cause: Option<Cause>) -> PathBuf {
    let _ = fs::create_dir_all(&paths.config_dir);
    let log = paths.crash_log();
    if fs::metadata(&log).is_ok_and(|m| m.len() > MAX_CRASH_LOG) {
        let _ = fs::rename(&log, paths.previous_crash_log());
    }
    let written = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .and_then(|mut f| f.write_all(text.as_bytes()));
    if written.is_ok() {
        let marker = cause.map_or("", Cause::marker);
        let _ = fs::write(paths.crash_pending(), marker);
    }
    log
}

/// The banner line for a report the last run left, once: removes the `crash.pending` marker.
/// A marker naming a known cause gets that cause's line.
pub fn take_pending(paths: &Paths) -> Option<String> {
    let marker = fs::read_to_string(paths.crash_pending()).unwrap_or_default();
    fs::remove_file(paths.crash_pending()).ok()?;
    let log = paths.crash_log();
    Some(match Cause::from_marker(&marker) {
        Some(cause) => cause.banner(&log),
        None => format!(
            "fbterm ran into an error last time. The report is in {}.",
            log.display()
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_paths() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("config"),
        };
        (dir, paths)
    }

    fn sample(what: &str, location: Option<&str>) -> Report {
        Report {
            time: "2026-10-28T09:15:00Z".into(),
            thread: "main".into(),
            what: what.into(),
            location: location.map(str::to_owned),
            cause: None,
            backtrace: "   0: fbterm::main\n".into(),
        }
    }

    #[test]
    fn report_lists_every_fact() {
        let text = sample("panic: boom", Some("src/app.rs:10:5")).text();
        let expected = format!(
            "==== fbterm crash report ====\ntime: 2026-10-28T09:15:00Z\nfbterm {}\nos: {} {}\n\
             thread: main\npanic: boom\nat: src/app.rs:10:5\nbacktrace:\n   0: fbterm::main\n\n",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        assert_eq!(text, expected);
    }

    #[test]
    fn start_failure_report_has_no_location() {
        let text = sample("fbterm could not start: no adapter", None).text();
        assert!(text.contains("\nfbterm could not start: no adapter\nbacktrace:\n"));
        assert!(!text.contains("at: "));
    }

    #[test]
    fn lost_gpu_device_is_recognized() {
        // As wgpu 27 panics while presenting a frame on a lost device.
        let message = "Error in Surface::present: Validation Error\n\nCaused by:\n  \
                       Parent device is lost";
        assert_eq!(Cause::of(message), Some(Cause::GpuLost));
        assert_eq!(Cause::of("index out of bounds"), None);
    }

    #[test]
    fn known_cause_adds_a_hint_line() {
        let mut report = sample("panic: Parent device is lost", Some("wgpu_core.rs:1:1"));
        report.cause = Some(Cause::GpuLost);
        let text = report.text();
        assert!(
            text.contains("\nat: wgpu_core.rs:1:1\nhint: the graphics device was lost"),
            "{text}"
        );
        assert!(!sample("panic: boom", None).text().contains("hint: "));
    }

    #[test]
    fn known_cause_gets_its_own_banner_once() {
        let (_dir, paths) = temp_paths();
        append_report(&paths, "report\n", Some(Cause::GpuLost));
        let line = take_pending(&paths).unwrap();
        assert!(
            line.starts_with("fbterm stopped last time because the graphics device was lost."),
            "{line}"
        );
        assert!(line.ends_with(&format!(
            "The report is in {}.",
            paths.crash_log().display()
        )));
        #[cfg(windows)]
        assert!(
            line.contains("WGPU_BACKEND set to dx12, vulkan or gl"),
            "{line}"
        );
        assert_eq!(take_pending(&paths), None);
    }

    #[test]
    fn a_later_report_without_cause_clears_the_cause() {
        let (_dir, paths) = temp_paths();
        append_report(&paths, "gpu\n", Some(Cause::GpuLost));
        append_report(&paths, "other\n", None);
        assert!(
            take_pending(&paths)
                .unwrap()
                .starts_with("fbterm ran into an error last time.")
        );
    }

    #[test]
    fn now_is_rfc3339_utc() {
        let now = now_rfc3339();
        assert!(chrono::DateTime::parse_from_rfc3339(&now).is_ok(), "{now}");
        assert!(now.ends_with('Z'), "{now}");
    }

    #[test]
    fn report_creates_dir_appends_and_marks_pending() {
        let (_dir, paths) = temp_paths();
        let log = append_report(&paths, "one\n", None);
        assert_eq!(log, paths.crash_log());
        append_report(&paths, "two\n", None);
        assert_eq!(fs::read_to_string(&log).unwrap(), "one\ntwo\n");
        assert!(paths.crash_pending().exists());
        assert!(!paths.previous_crash_log().exists());
    }

    #[test]
    fn large_crash_log_moves_aside_first() {
        let (_dir, paths) = temp_paths();
        fs::create_dir_all(&paths.config_dir).unwrap();
        let big = "x".repeat(MAX_CRASH_LOG as usize + 1);
        fs::write(paths.crash_log(), &big).unwrap();
        fs::write(paths.previous_crash_log(), "oldest").unwrap();
        append_report(&paths, "new\n", None);
        assert_eq!(fs::read_to_string(paths.crash_log()).unwrap(), "new\n");
        assert_eq!(fs::read_to_string(paths.previous_crash_log()).unwrap(), big);
    }

    #[test]
    fn crash_log_at_the_limit_stays() {
        let (_dir, paths) = temp_paths();
        fs::create_dir_all(&paths.config_dir).unwrap();
        let full = "x".repeat(MAX_CRASH_LOG as usize);
        fs::write(paths.crash_log(), &full).unwrap();
        append_report(&paths, "new\n", None);
        assert_eq!(
            fs::read_to_string(paths.crash_log()).unwrap(),
            format!("{full}new\n")
        );
        assert!(!paths.previous_crash_log().exists());
    }

    #[test]
    fn run_log_keeps_the_previous_run() {
        let (_dir, paths) = temp_paths();
        let mut first = open_run_log(&paths).expect("first log");
        first.write_all(b"run one\n").unwrap();
        drop(first);
        fs::write(paths.previous_run_log(), "run zero\n").unwrap();
        let second = open_run_log(&paths).expect("second log");
        drop(second);
        assert_eq!(fs::read_to_string(paths.run_log()).unwrap(), "");
        assert_eq!(
            fs::read_to_string(paths.previous_run_log()).unwrap(),
            "run one\n"
        );
    }

    #[test]
    fn run_log_that_cannot_open_is_none() {
        let (dir, _) = temp_paths();
        // The config dir is a file, so nothing can be created in it.
        let blocker = dir.path().join("file");
        fs::write(&blocker, "").unwrap();
        let paths = Paths {
            config_dir: blocker,
        };
        assert!(open_run_log(&paths).is_none());
    }

    #[test]
    fn pending_marker_is_consumed_once() {
        let (_dir, paths) = temp_paths();
        assert_eq!(take_pending(&paths), None);
        append_report(&paths, "report\n", None);
        assert_eq!(
            take_pending(&paths),
            Some(format!(
                "fbterm ran into an error last time. The report is in {}.",
                paths.crash_log().display()
            ))
        );
        assert!(!paths.crash_pending().exists());
        assert_eq!(take_pending(&paths), None);
        assert!(paths.crash_log().exists());
    }

    #[test]
    fn launch_shows_the_pending_report_once() {
        let paths = crate::restore_tests::paths_with(&[]);
        append_report(&paths, "report\n", None);
        let line = take_pending(&paths).unwrap();
        append_report(&paths, "report\n", None);

        let (app, _) = crate::app::App::build(paths.clone(), true);
        assert!(app.visible_notices().contains(&line), "{:?}", app.notices);
        assert!(!paths.crash_pending().exists());
        let (again, _) = crate::app::App::build(paths, true);
        assert!(!again.notices.contains(&line), "{:?}", again.notices);
    }
}

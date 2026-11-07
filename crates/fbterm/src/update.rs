//! The optional update check (`check_updates`): at launch and every 24 hours while the setting
//! is on, ask GitHub for the latest published release of fbterm and, when it is newer than
//! this build, show a dot on the tab bar's settings button and a download link in Settings.
//! Nothing is ever downloaded or installed. A failed check is logged and otherwise silent.
//!
//! The check runs as one subscription that exists only while the setting is on: it checks at
//! once, then sleeps a whole [`INTERVAL`] before the next check, so it never wakes the UI in
//! between. Turning the setting on starts it (a check at once); turning it off drops it.
//!
//! Until the user has answered (`check_updates` absent from `settings.toml`), the app asks
//! once over a terminal window ([`QUESTION_TITLE`]).

use std::time::Duration;

use iced::futures::Stream;
use semver::Version;
use serde::Deserialize;

use crate::app::App;

/// The latest published (not draft, not prerelease) release of fbterm.
pub const LATEST_URL: &str =
    "https://api.github.com/repos/featherbitplatform/terminal/releases/latest";
/// Release pages the download link may point at.
pub const RELEASES_PREFIX: &str = "https://github.com/featherbitplatform/terminal/releases/";
/// The download link when the release's own page is not one of [`RELEASES_PREFIX`].
pub const FALLBACK_URL: &str = "https://github.com/featherbitplatform/terminal/releases/latest";
/// Time between checks while fbterm runs.
pub const INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// Largest release JSON read (GitHub's are a few kilobytes plus the release notes).
const MAX_BYTES: u64 = 1024 * 1024;

/// A newer release than this build: its version and where to download it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Available {
    pub version: Version,
    /// A page under [`RELEASES_PREFIX`].
    pub url: String,
}

impl Available {
    /// The Settings line about it.
    pub fn message(&self) -> String {
        format!("Version {} is available.", self.version)
    }
}

/// The fields of GitHub's release JSON the check reads.
#[derive(Deserialize)]
struct ReleaseJson {
    tag_name: String,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// A release tag (`v1.2.3`, or `1.2.3`) as a version; `None` for anything else, and for
/// prerelease versions (`v1.2.3-rc.1`), which the check never offers. Build metadata
/// (`+...`) is dropped: it never makes a release newer.
pub fn tag_version(tag: &str) -> Option<Version> {
    let mut v = Version::parse(tag.trim().strip_prefix('v').unwrap_or(tag.trim())).ok()?;
    v.build = semver::BuildMetadata::EMPTY;
    v.pre.is_empty().then_some(v)
}

/// Whether `url` may be the download link: [`RELEASES_PREFIX`] followed only by plain path
/// characters (`A-Z a-z 0-9 . _ / -`), with no `..` and no empty segment, so it cannot leave
/// fbterm's release pages (no query, fragment, escapes or backslashes).
pub fn release_link_ok(url: &str) -> bool {
    url.strip_prefix(RELEASES_PREFIX).is_some_and(|rest| {
        rest.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'))
            && !rest.contains("..")
            && !rest.starts_with('/')
            && !rest.contains("//")
    })
}

/// The release in `json`: `None` for a draft, a prerelease or a tag that is no plain version.
/// The link is the release's page when it is one of fbterm's, else [`FALLBACK_URL`].
pub fn parse_release(json: &[u8]) -> Result<Option<Available>, String> {
    let release: ReleaseJson = serde_json::from_slice(json).map_err(|e| e.to_string())?;
    if release.draft || release.prerelease {
        return Ok(None);
    }
    let Some(version) = tag_version(&release.tag_name) else {
        return Ok(None);
    };
    let url = release
        .html_url
        .filter(|u| release_link_ok(u))
        .unwrap_or_else(|| FALLBACK_URL.to_owned());
    Ok(Some(Available { version, url }))
}

/// `release` if it is newer than `current` (this build's version); build metadata on either
/// side is ignored.
pub fn newer(current: &str, release: Option<Available>) -> Option<Available> {
    let mut current = Version::parse(current).ok()?;
    current.build = semver::BuildMetadata::EMPTY;
    release.filter(|r| {
        let mut v = r.version.clone();
        v.build = semver::BuildMetadata::EMPTY;
        v > current
    })
}

/// One check: fetch the latest release with `fetch` (url, headers, size cap) and compare it
/// with `current`. `Ok(None)`: this build is current.
pub fn check(
    fetch: impl FnOnce(&str, &[(&str, &str)], u64) -> Result<Vec<u8>, String>,
    current: &str,
) -> Result<Option<Available>, String> {
    let agent = format!("fbterm/{current}");
    let headers = [
        ("User-Agent", agent.as_str()),
        ("Accept", "application/vnd.github+json"),
    ];
    let json = fetch(LATEST_URL, &headers, MAX_BYTES)?;
    Ok(newer(current, parse_release(&json)?))
}

/// What the check subscription is keyed on. `demo`: `FBTERM_DEMO_UPDATE` (debug aid), a
/// version reported as the latest without any network access.
#[derive(Debug, Clone, Hash)]
pub struct CheckKey {
    pub demo: Option<String>,
}

/// The checks: one at once, then one per [`INTERVAL`], each on the blocking pool.
pub fn check_stream(
    key: &CheckKey,
) -> impl Stream<Item = Result<Option<Available>, String>> + use<> {
    let demo = key.demo.clone();
    iced::futures::stream::unfold(true, move |first| {
        let demo = demo.clone();
        async move {
            if !first {
                tokio::time::sleep(INTERVAL).await;
            }
            let current = env!("CARGO_PKG_VERSION");
            let result = match demo {
                Some(v) => check(|_, _, _| Ok(demo_json(&v)), current),
                None => tokio::task::spawn_blocking(move || check(crate::net::get, current))
                    .await
                    .unwrap_or_else(|e| Err(e.to_string())),
            };
            Some((result, false))
        }
    })
}

/// A release JSON for version `v`, for `FBTERM_DEMO_UPDATE`.
fn demo_json(v: &str) -> Vec<u8> {
    serde_json::json!({
        "tag_name": format!("v{}", v.trim_start_matches('v')),
        "html_url": format!("{RELEASES_PREFIX}tag/v{}", v.trim_start_matches('v')),
        "draft": false,
        "prerelease": false,
    })
    .to_string()
    .into_bytes()
}

impl App {
    /// Whether the check subscription runs.
    pub(crate) fn update_check_scheduled(&self) -> bool {
        self.settings.checks_updates()
    }

    /// Whether the tab bar's settings button shows the update dot.
    pub(crate) fn update_dot(&self) -> bool {
        self.update_available.is_some()
    }

    /// A check finished. Ignored once the setting is off; a failure keeps what was known.
    pub(crate) fn update_checked(&mut self, result: Result<Option<Available>, String>) {
        if !self.settings.checks_updates() {
            return;
        }
        match result {
            Ok(found) => {
                match &found {
                    Some(a) => tracing::info!(version = %a.version, "a newer fbterm is available"),
                    None => tracing::info!("update check: fbterm is up to date"),
                }
                self.update_available = found;
            }
            Err(e) => tracing::info!(error = %e, "update check failed"),
        }
    }

    /// `Download` in Settings: the release page in the system browser.
    pub(crate) fn open_update_page(&self) {
        if let Some(a) = &self.update_available {
            crate::sessions::spawn_reaped(crate::sessions::opener(&a.url), &a.url);
        }
    }

    /// Keep the Settings tab's copy of [`App::update_available`] in step (after every
    /// update).
    pub(crate) fn sync_update_state(&mut self) {
        if let Some((_, state)) = &mut self.settings_win
            && state.update != self.update_available
        {
            state.update.clone_from(&self.update_available);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Message;
    use crate::restore_tests::{paths_with, profile, saved};
    use fbterm_config::{Paths, SavedState};
    use iced::window;

    const RELEASE: &str = r#"{
        "tag_name": "v0.5.0",
        "html_url": "https://github.com/featherbitplatform/terminal/releases/tag/v0.5.0",
        "draft": false,
        "prerelease": false,
        "name": "0.5.0",
        "assets": []
    }"#;

    fn available(v: &str) -> Available {
        Available {
            version: Version::parse(v).unwrap(),
            url: format!("{RELEASES_PREFIX}tag/v{v}"),
        }
    }

    #[test]
    fn versions_compare_as_semver() {
        let release = |v: &str| {
            tag_version(v).map(|version| Available {
                version,
                url: FALLBACK_URL.into(),
            })
        };
        assert!(newer("0.4.0", release("v0.5.0")).is_some(), "newer");
        assert!(
            newer("0.4.0", release("v0.4.10")).is_some(),
            "numeric, not text"
        );
        assert!(newer("0.4.0", release("v0.4.0")).is_none(), "same");
        assert!(newer("0.4.0", release("v0.3.9")).is_none(), "older");
        assert!(
            newer("0.4.0-rc.1", release("v0.4.0")).is_some(),
            "release after own rc"
        );
        assert_eq!(
            tag_version("v0.5.0-rc.1"),
            None,
            "prerelease tags never offered"
        );
        assert_eq!(tag_version("0.5.0"), Version::parse("0.5.0").ok());
        assert_eq!(tag_version("latest"), None);
        assert_eq!(tag_version("v1.2"), None);
        assert_eq!(tag_version(""), None);
        assert!(newer("garbage", release("v9.9.9")).is_none());
        assert!(
            newer("0.4.0", release("v0.4.0+1")).is_none(),
            "build metadata"
        );
        assert!(newer("0.4.0+abc", release("v0.4.0")).is_none());
        assert!(newer("0.4.0+abc", release("v0.4.1+2")).is_some());
        assert_eq!(tag_version("v0.4.0+1"), Version::parse("0.4.0").ok());
    }

    #[test]
    fn release_json_gives_version_and_link() {
        assert_eq!(
            parse_release(RELEASE.as_bytes()).unwrap(),
            Some(available("0.5.0"))
        );
    }

    #[test]
    fn drafts_and_prereleases_are_ignored() {
        let draft = RELEASE.replace(r#""draft": false"#, r#""draft": true"#);
        assert_eq!(parse_release(draft.as_bytes()).unwrap(), None);
        let pre = RELEASE.replace(r#""prerelease": false"#, r#""prerelease": true"#);
        assert_eq!(parse_release(pre.as_bytes()).unwrap(), None);
    }

    #[test]
    fn a_link_elsewhere_is_replaced() {
        for url in [
            "https://evil.example/featherbitplatform/terminal/releases/tag/v0.5.0",
            "http://github.com/featherbitplatform/terminal/releases/tag/v0.5.0",
            "https://github.com/someone/terminal/releases/tag/v0.5.0",
            "https://github.com/featherbitplatform/terminal/releases/../../other/x",
            "https://github.com/featherbitplatform/terminal/releases/%2e%2e/%2E%2E/other",
            "https://github.com/featherbitplatform/terminal/releases/tag/v0.5.0?x=1",
            "https://github.com/featherbitplatform/terminal/releases/tag/v0.5.0#x",
            // A JSON-escaped backslash.
            "https://github.com/featherbitplatform/terminal/releases/a\\\\x",
            "https://github.com/featherbitplatform/terminal/releases//evil",
            "https://github.com/featherbitplatform/terminal/releases/tag/v 0.5.0",
        ] {
            let json = RELEASE.replace(
                "https://github.com/featherbitplatform/terminal/releases/tag/v0.5.0",
                url,
            );
            let found = parse_release(json.as_bytes()).unwrap().unwrap();
            assert_eq!(found.url, FALLBACK_URL, "{url}");
        }
        let json = r#"{"tag_name": "v0.5.0"}"#;
        assert_eq!(
            parse_release(json.as_bytes()).unwrap().unwrap().url,
            FALLBACK_URL
        );
    }

    #[test]
    fn plain_release_links_are_kept() {
        assert!(release_link_ok(
            "https://github.com/featherbitplatform/terminal/releases/tag/v0.5.0"
        ));
        assert!(release_link_ok(FALLBACK_URL));
    }

    #[test]
    fn bad_json_is_an_error() {
        assert!(parse_release(b"not json").is_err());
        assert!(parse_release(br#"{"name": "no tag"}"#).is_err());
    }

    #[test]
    fn check_asks_github_with_our_headers() {
        let mut asked = None;
        let found = check(
            |url, headers, cap| {
                asked = Some((
                    url.to_owned(),
                    headers
                        .iter()
                        .map(|(k, v)| format!("{k}: {v}"))
                        .collect::<Vec<_>>(),
                    cap,
                ));
                Ok(RELEASE.as_bytes().to_vec())
            },
            "0.4.0",
        );
        assert_eq!(found, Ok(Some(available("0.5.0"))));
        let (url, headers, cap) = asked.unwrap();
        assert_eq!(url, LATEST_URL);
        assert_eq!(
            headers,
            [
                "User-Agent: fbterm/0.4.0",
                "Accept: application/vnd.github+json"
            ]
        );
        assert_eq!(cap, MAX_BYTES);
        assert_eq!(
            check(|_, _, _| Err("offline".into()), "0.4.0"),
            Err("offline".into())
        );
        assert_eq!(check(|_, _, _| Ok(RELEASE.into()), "0.5.0"), Ok(None));
    }

    /// An app with one terminal window and `settings` as `settings.toml` (`None`: no file).
    fn app_with(settings: Option<&str>) -> (App, Paths, window::Id) {
        let a = profile("a");
        let paths = paths_with(std::slice::from_ref(&a));
        SavedState {
            windows: vec![saved(&[&a], 0)],
            ..Default::default()
        }
        .save(&paths.state())
        .unwrap();
        if let Some(s) = settings {
            std::fs::write(paths.settings(), s).unwrap();
        }
        let (app, _) = App::build(paths.clone(), true);
        let wid = *app.windows.keys().next().unwrap();
        (app, paths, wid)
    }

    #[test]
    fn off_means_no_check_and_no_dot() {
        let (mut app, _, _) = app_with(Some("check_updates = false\n"));
        assert!(!app.update_check_scheduled());
        app.update_checked(Ok(Some(available("9.0.0"))));
        assert!(!app.update_dot(), "a stray result while off shows nothing");
        let (app, _, _) = app_with(None);
        assert!(
            !app.update_check_scheduled(),
            "never answered counts as off"
        );
    }

    #[test]
    fn a_newer_release_shows_the_dot_and_the_message_until_turned_off() {
        let (mut app, _, _) = app_with(Some("check_updates = true\n"));
        assert!(app.update_check_scheduled());
        assert!(!app.update_dot());
        app.update_checked(Ok(Some(available("9.0.0"))));
        assert!(app.update_dot());
        assert_eq!(
            app.update_available.as_ref().unwrap().message(),
            "Version 9.0.0 is available."
        );
        // A failed check keeps what was known.
        app.update_checked(Err("offline".into()));
        assert!(app.update_dot());
        // Turned off in Settings: dot and message go, and no check runs.
        let _ = app.update(Message::Settings(crate::settings_window::SettingsMsg::Open));
        let shown = |app: &App| app.settings_win.as_ref().unwrap().1.update.clone();
        assert_eq!(shown(&app), Some(available("9.0.0")), "Settings shows it");
        let _ = app.update(Message::Settings(
            crate::settings_window::SettingsMsg::CheckUpdates(false),
        ));
        assert!(!app.update_check_scheduled());
        assert!(!app.update_dot());
        assert!(app.update_available.is_none());
        assert_eq!(shown(&app), None);
    }

    #[test]
    fn a_failed_first_check_leaves_nothing() {
        let (mut app, _, _) = app_with(Some("check_updates = true\n"));
        app.update_checked(Err("HTTP 403".into()));
        assert!(!app.update_dot());
    }

    #[test]
    fn demo_release_reads_as_newer() {
        let found = check(|_, _, _| Ok(demo_json("9.1.0")), "0.4.0").unwrap();
        assert_eq!(found, Some(available("9.1.0")));
    }
}

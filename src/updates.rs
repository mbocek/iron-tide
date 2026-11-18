use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};

use bevy::prelude::*;

use crate::settings::GameSettings;

const GITHUB_PREFIX: &str = "https://github.com/";
const DOWNLOAD_CHUNK_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Version(u32, u32, u32);

impl Version {
    pub fn parse(text: &str) -> Option<Version> {
        let mut parts = text.trim().trim_start_matches('v').split('.');
        let mut next = || parts.next()?.parse().ok();
        Some(Version(next()?, next()?, next()?))
    }

    pub fn current() -> Version {
        Version::parse(env!("CARGO_PKG_VERSION")).expect("the crate version is major.minor.patch")
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// A newer published version and the package built for this computer.
#[derive(Clone, PartialEq, Debug)]
pub struct Release {
    pub version: Version,
    pub package_name: String,
    pub package_url: String,
}

/// The end of the release package name that fits this build, matching what CI publishes.
fn package_suffix() -> &'static str {
    if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        "macos-arm64.dmg"
    } else if cfg!(target_os = "macos") {
        "macos-x86_64.dmg"
    } else if cfg!(target_os = "windows") {
        "windows-x86_64.zip"
    } else {
        "linux-x86_64.tar.gz"
    }
}

/// Reads GitHub's latest-release answer; `None` unless it is newer and has a package for this computer.
pub fn newer_release(latest_release_json: &str, current: Version) -> Option<Release> {
    let release: serde_json::Value = serde_json::from_str(latest_release_json).ok()?;
    let version = Version::parse(release["tag_name"].as_str()?)?;
    if version <= current {
        return None;
    }
    let package = release["assets"].as_array()?.iter().find(|asset| {
        asset["name"]
            .as_str()
            .is_some_and(|name| name.ends_with(package_suffix()))
    })?;
    Some(Release {
        version,
        package_name: package["name"].as_str()?.to_string(),
        package_url: package["browser_download_url"].as_str()?.to_string(),
    })
}

fn latest_release_url() -> Option<String> {
    let repository = env!("CARGO_PKG_REPOSITORY").strip_prefix(GITHUB_PREFIX)?;
    Some(format!(
        "https://api.github.com/repos/{repository}/releases/latest"
    ))
}

/// The game names itself without any detail about the player.
fn user_agent() -> String {
    format!("iron-tide/{}", env!("CARGO_PKG_VERSION"))
}

#[derive(Clone, PartialEq, Debug, Default)]
pub enum UpdateState {
    #[default]
    NothingNew,
    Available(Release),
    Downloading {
        release: Release,
        fraction: f32,
    },
    Downloaded(PathBuf),
    Failed(String),
}

enum UpdateNews {
    Found(Release),
    Progress(f32),
    Saved(PathBuf),
    Failed(String),
}

#[derive(Resource, Default)]
pub struct Updates {
    pub state: UpdateState,
    news: Option<Mutex<Receiver<UpdateNews>>>,
}

impl Updates {
    fn listen(&mut self) -> Sender<UpdateNews> {
        let (sender, receiver) = channel();
        self.news = Some(Mutex::new(receiver));
        sender
    }
}

pub fn check_for_updates(settings: Res<GameSettings>, mut updates: ResMut<Updates>) {
    if !settings.check_for_updates || crate::capture::is_capture_run() {
        return;
    }
    let Some(url) = latest_release_url() else {
        return;
    };
    let news = updates.listen();
    std::thread::spawn(move || {
        let answer = ureq::get(&url)
            .header("User-Agent", user_agent())
            .header("Accept", "application/vnd.github+json")
            .call()
            .and_then(|mut response| response.body_mut().read_to_string());
        match answer {
            Ok(json) => {
                if let Some(release) = newer_release(&json, Version::current()) {
                    let _ = news.send(UpdateNews::Found(release));
                }
            }
            Err(error) => info!("could not check for a newer version: {error}"),
        }
    });
}

pub fn receive_update_news(mut updates: ResMut<Updates>) {
    let Some(news) = updates.news.as_ref() else {
        return;
    };
    let arrived: Vec<UpdateNews> = news
        .lock()
        .map_or_else(|_| Vec::new(), |n| n.try_iter().collect());
    for item in arrived {
        updates.state = match (item, &updates.state) {
            (UpdateNews::Found(release), _) => UpdateState::Available(release),
            (UpdateNews::Progress(fraction), UpdateState::Downloading { release, .. }) => {
                UpdateState::Downloading {
                    release: release.clone(),
                    fraction,
                }
            }
            (UpdateNews::Progress(_), state) => state.clone(),
            (UpdateNews::Saved(path), _) => {
                reveal(&path);
                UpdateState::Downloaded(path)
            }
            (UpdateNews::Failed(reason), _) => UpdateState::Failed(reason),
        };
    }
}

/// Fetches the package into the player's Downloads folder; the player installs it themselves.
pub fn download_update(updates: &mut Updates) {
    let UpdateState::Available(release) = updates.state.clone() else {
        if let UpdateState::Downloaded(path) = &updates.state {
            reveal(path);
        }
        return;
    };
    let target = downloads_directory().join(&release.package_name);
    let news = updates.listen();
    updates.state = UpdateState::Downloading {
        release: release.clone(),
        fraction: 0.0,
    };
    std::thread::spawn(move || {
        let outcome = fetch_to_file(&release.package_url, &target, &news);
        let _ = news.send(match outcome {
            Ok(()) => UpdateNews::Saved(target),
            Err(reason) => UpdateNews::Failed(reason),
        });
    });
}

fn fetch_to_file(url: &str, target: &Path, news: &Sender<UpdateNews>) -> Result<(), String> {
    let response = ureq::get(url)
        .header("User-Agent", user_agent())
        .call()
        .map_err(|error| error.to_string())?;
    let total = response.body().content_length();
    let mut reader = response.into_body().into_reader();
    let partial = target.with_extension("part");
    let mut file = std::fs::File::create(&partial).map_err(|error| error.to_string())?;
    let mut buffer = vec![0; DOWNLOAD_CHUNK_BYTES];
    let mut received = 0u64;
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .map_err(|error| error.to_string())?;
        received += read as u64;
        if let Some(total) = total.filter(|&total| total > 0) {
            let _ = news.send(UpdateNews::Progress(received as f32 / total as f32));
        }
    }
    drop(file);
    std::fs::rename(&partial, target).map_err(|error| error.to_string())
}

fn downloads_directory() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|home| PathBuf::from(home).join("Downloads"))
        .filter(|directory| directory.is_dir())
        .unwrap_or_else(std::env::temp_dir)
}

/// Opens a disk image so it mounts; archives are shown in their folder instead.
fn reveal(path: &Path) {
    let opened = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(path).spawn()
    } else if cfg!(target_os = "windows") {
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn()
    } else {
        let folder = path.parent().unwrap_or(path);
        std::process::Command::new("xdg-open").arg(folder).spawn()
    };
    if let Err(error) = opened {
        warn!("could not open {}: {error}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release_json(tag: &str) -> String {
        format!(
            r#"{{"tag_name": "{tag}", "assets": [
                {{"name": "iron-tide-9.0.0-{suffix}", "browser_download_url": "https://example.invalid/pkg"}},
                {{"name": "iron-tide-9.0.0-unknown-os.zip", "browser_download_url": "https://example.invalid/other"}}
            ]}}"#,
            suffix = package_suffix()
        )
    }

    #[test]
    fn versions_compare_number_by_number() {
        assert!(Version::parse("v1.10.0") > Version::parse("1.9.3"));
        assert_eq!(Version::parse("v2.0.1"), Some(Version(2, 0, 1)));
        assert_eq!(Version::parse("nightly"), None);
    }

    #[test]
    fn a_newer_release_offers_the_package_for_this_computer() {
        let release = newer_release(&release_json("v9.0.0"), Version(1, 2, 0)).unwrap();
        assert_eq!(release.version, Version(9, 0, 0));
        assert!(release.package_name.ends_with(package_suffix()));
        assert_eq!(release.package_url, "https://example.invalid/pkg");
    }

    #[test]
    fn the_same_or_an_older_release_is_not_offered() {
        assert_eq!(
            newer_release(&release_json("v1.2.0"), Version(1, 2, 0)),
            None
        );
        assert_eq!(
            newer_release(&release_json("v1.1.9"), Version(1, 2, 0)),
            None
        );
        assert_eq!(newer_release("not json", Version(1, 2, 0)), None);
    }

    #[test]
    fn the_current_version_comes_from_the_crate() {
        assert_eq!(Version::current().to_string(), env!("CARGO_PKG_VERSION"));
    }
}

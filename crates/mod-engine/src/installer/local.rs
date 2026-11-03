//! Install from an archive the player already has (Nexus games).
//!
//! A file downloaded from a Nexus page keeps its name
//! (`Cool HUD 5000 1.2 2026-11-03T21-03Z 8TxGunSsn.zip`, or before
//! `Cool HUD-5000-1-2-1790000000.zip`): the mod's id gives its page, the
//! file's size tells which of its files it is. It then installs like a
//! Nexus file, from the cache instead of the network, and keeps its updates.
//!
//! Any other archive is tracked under its own name, without a page or
//! updates. Either way the player's file stays where it is: Unalia works on
//! a copy.

use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

use serde::Serialize;

use super::nexus::{Adapter, NexusInstall, Stored, file_version, linked_files};
use super::{InstallError, Installer, NexusFile, Package, Progress, Target};
use crate::download;
use crate::games::GameProfile;
use crate::loaders::framework::Framework;
use crate::sources::github::GitHub;
use crate::sources::nexus::{ModInfo, Nexus, iso_date, mod_key};
use crate::state::State;

/// Value of `InstalledMod::source` for a mod that isn't on any source.
const FILE_SOURCE: &str = "file";

/// Numbers tried as the mod's id in a file name (the version has some too).
const ID_CANDIDATES: usize = 2;

/// What an archive will be once installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalFile {
    pub mod_id: String,
    pub name: String,
}

/// What the name of a file downloaded from Nexus says.
#[derive(Debug, PartialEq, Eq)]
struct NexusName<'a> {
    name: &'a str,
    ids: Vec<u32>,
    /// To the minute, as in `uploaded_minute`.
    uploaded: String,
}

impl Installer<'_, Nexus> {
    /// What `path` will be installed as.
    pub async fn identify_file(&self, profile: &GameProfile, path: &Path) -> LocalFile {
        match self.recognize(profile, path).await {
            Some((info, _)) => LocalFile {
                mod_id: mod_key(info.mod_id),
                name: info.name,
            },
            None => unknown_file(path),
        }
    }

    /// Installs an archive of the player's and the frameworks it needs.
    #[allow(clippy::too_many_arguments)]
    pub async fn install_file(
        &self,
        state: &mut State,
        target: &Target<'_>,
        profile: &GameProfile,
        github: &GitHub,
        path: &Path,
        before: Option<&str>,
        variant: Option<&str>,
        mut on_progress: impl FnMut(Progress),
    ) -> Result<NexusInstall, InstallError> {
        on_progress(Progress::Resolving);
        let file = unknown_file(path);
        let metadata = fs::metadata(path).map_err(|_| InstallError::NoFile(file.name.clone()))?;
        if !metadata.is_file() {
            return Err(InstallError::NoFile(file.name));
        }

        if let Some((info, file_id)) = self.recognize(profile, path).await {
            let id = mod_key(info.mod_id);
            let existing = state.list(target.game)?;
            let previous = existing.iter().find(|m| m.id == id);
            let version_of = |file_id: u32| {
                info.files
                    .iter()
                    .find(|f| f.file_id == file_id)
                    .map(|f| file_version(&info, f, before))
                    .unwrap_or_default()
            };
            let version = version_of(file_id);
            if !self.cached(target.game, &id, &version, Some(file_id)) {
                self.take_copy(path, &id, &version, Some(file_id)).await?;
            }
            // What the mod already has stays with it. A file that is no
            // longer stored would need a download.
            let ids: Vec<u32> = linked_files(&info, previous, file_id)
                .into_iter()
                .filter(|f| *f == file_id || self.cached(target.game, &id, &version_of(*f), Some(*f)))
                .collect();
            return self
                .install_nexus(
                    state,
                    target,
                    profile,
                    github,
                    info.mod_id,
                    NexusFile::Files { ids: &ids, link: None },
                    before,
                    variant,
                    on_progress,
                )
                .await;
        }

        // The date stands for the version, the hash keeps two archives of
        // the same day apart.
        let owned = path.to_owned();
        let sha256 = tokio::task::spawn_blocking(move || download::sha256_file(&owned))
            .await
            .map_err(|e| InstallError::Io(std::io::Error::other(e)))??;
        let hash = u32::from_str_radix(&sha256[..8], 16).unwrap_or_default();
        let modified = metadata
            .modified()
            .ok()
            .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |at| at.as_secs() as i64);
        let version = iso_date(modified)[..10].to_owned();
        if !self.cached(target.game, &file.mod_id, &version, Some(hash)) {
            self.take_copy(path, &file.mod_id, &version, Some(hash)).await?;
        }

        let adapter = Adapter::for_game(profile, target.game_dir)?;
        let frameworks: Vec<Framework> = profile
            .frameworks
            .iter()
            .map(|f| f.resolve(adapter.project(), adapter.binaries()))
            .collect();
        let package = Package {
            connections: None,
            id: file.mod_id.clone(),
            name: file.name,
            author: String::new(),
            version,
            icon_url: None,
            adult: false,
            download_url: String::new(),
            sha256: None,
            file_id: Some(hash),
        };
        let item = self
            .fetch(target, &adapter, package, 0, 1, &mut on_progress)
            .await?;
        let existing = state.list(target.game)?;
        let mut done = self
            .place_mod(
                state,
                target,
                profile,
                github,
                Stored {
                    adapter,
                    frameworks,
                    existing,
                    item,
                    requested: file.mod_id.clone(),
                    chosen: Vec::new(),
                    requirements: Vec::new(),
                    before,
                    variant,
                },
                on_progress,
            )
            .await?;
        if let Some(mut record) = state.get(target.game, &file.mod_id)? {
            record.source = FILE_SOURCE.to_owned();
            state.save(&record)?;
        }
        done.mods = state.list(target.game)?;
        Ok(done)
    }

    /// The Nexus mod and file `path` comes from, by its name and its size.
    /// Nexus out of reach: an unknown file.
    async fn recognize(&self, profile: &GameProfile, path: &Path) -> Option<(ModInfo, u32)> {
        let domain = &profile.nexus.as_ref()?.domain;
        let size = fs::metadata(path).ok()?.len();
        let stem = path.file_stem()?.to_str()?;
        let name = nexus_name(stem)?;
        for id in name.ids {
            let Ok(info) = self.source.mod_info(domain, id).await else {
                continue;
            };
            let same_size = || info.files.iter().filter(|f| f.size == Some(size));
            let file = same_size()
                .find(|f| uploaded_minute(f.uploaded_at) == name.uploaded)
                .or_else(|| same_size().next());
            if let Some(file_id) = file.map(|f| f.file_id) {
                return Some((info, file_id));
            }
        }
        None
    }

    /// Puts a copy of `path` in the download cache: the install takes it
    /// from there, and removes it once stored.
    async fn take_copy(
        &self,
        path: &Path,
        id: &str,
        version: &str,
        file_id: Option<u32>,
    ) -> Result<(), InstallError> {
        let archive = self.archive_path(id, version, file_id);
        let from = path.to_owned();
        // On a separate thread: an archive can weigh gigabytes.
        tokio::task::spawn_blocking(move || {
            if let Some(dir) = archive.parent() {
                fs::create_dir_all(dir)?;
            }
            fs::copy(from, archive).map(|_| ())
        })
        .await
        .map_err(|e| InstallError::Io(std::io::Error::other(e)))??;
        Ok(())
    }
}

/// An archive that isn't on Nexus, tracked under its own name.
fn unknown_file(path: &Path) -> LocalFile {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy())
        .unwrap_or_default();
    let name = nexus_name(&stem).map_or(without_copy_mark(&stem), |n| n.name);
    let name = name.replace('_', " ").trim().to_owned();
    let slug: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    LocalFile {
        mod_id: format!("file-{slug}"),
        name,
    }
}

/// `Cool HUD (1)`: the browser's mark on a second download of a file.
fn without_copy_mark(stem: &str) -> &str {
    stem.strip_suffix(')')
        .and_then(|s| s.rsplit_once(" ("))
        .filter(|(_, mark)| mark.chars().all(|c| c.is_ascii_digit()))
        .map_or(stem, |(name, _)| name)
}

/// `2026-11-03T21-03`: the upload date of a file, as its name writes it.
fn uploaded_minute(secs: i64) -> String {
    iso_date(secs)[..16].replace(':', "-")
}

fn number(word: &str) -> Option<u32> {
    word.parse().ok().filter(|_| word.chars().all(|c| c.is_ascii_digit()))
}

fn nexus_name(stem: &str) -> Option<NexusName<'_>> {
    let stem = without_copy_mark(stem);
    spaced_name(stem).or_else(|| dashed_name(stem))
}

/// Reads `<name> <mod id> <version> <upload date> <code>`. The name can
/// hold numbers too: the last ones are all candidates.
fn spaced_name(stem: &str) -> Option<NexusName<'_>> {
    let (rest, _code) = stem.rsplit_once(' ')?;
    let (rest, date) = rest.rsplit_once(' ')?;
    let uploaded = date.strip_suffix('Z').filter(|d| d.len() == 16 && d.as_bytes()[10] == b'T')?;
    let (mut rest, _version) = rest.rsplit_once(' ')?;
    let mut name = None;
    let mut ids = Vec::new();
    while ids.len() < ID_CANDIDATES
        && let Some((before, word)) = rest.rsplit_once(' ')
    {
        if let Some(id) = number(word) {
            name.get_or_insert(before);
            ids.push(id);
        }
        rest = before;
    }
    Some(NexusName {
        name: name?,
        ids,
        uploaded: uploaded.to_owned(),
    })
}

/// Reads `<name>-<mod id>-<version>-<upload date>`. The name can hold
/// numbers too: the first ones are all candidates.
fn dashed_name(stem: &str) -> Option<NexusName<'_>> {
    let (rest, date) = stem.rsplit_once('-')?;
    if date.len() != 10 {
        return None;
    }
    let uploaded = uploaded_minute(date.parse().ok()?);
    let mut parts = rest.split('-').enumerate().skip(1);
    let (index, _) = parts.clone().find(|(_, part)| number(part).is_some())?;
    let name_end = rest.split('-').take(index).map(|p| p.len() + 1).sum::<usize>() - 1;
    let ids = parts
        .by_ref()
        .filter_map(|(_, part)| number(part))
        .take(ID_CANDIDATES)
        .collect();
    Some(NexusName {
        name: &rest[..name_end],
        ids,
        uploaded,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nexus_file_name_gives_the_mod_and_the_date() {
        let name = nexus_name("Auto Collect Loot 70 1.6.0 2026-11-03T21-03Z 8TxGunSsn").unwrap();
        assert_eq!(name.name, "Auto Collect Loot");
        assert_eq!(name.ids, [70]);
        // Its upload date on Nexus, 2026-11-03 at 21:03:51.
        assert_eq!(name.uploaded, uploaded_minute(1_793_739_831));
    }

    #[test]
    fn a_version_in_a_mod_name_is_not_its_id() {
        let name = nexus_name("Uemm Phantom Blade Zero 1.4.1 2 1.4.1 2026-11-01T18-14Z l9gt3h8tQ").unwrap();
        assert_eq!(name.name, "Uemm Phantom Blade Zero 1.4.1");
        assert_eq!(name.ids, [2]);
    }

    #[test]
    fn a_number_as_version_is_not_the_mods_id() {
        let name = nexus_name("Better HUD 2 5000 3 2026-11-03T21-03Z abc").unwrap();
        assert_eq!(name.name, "Better HUD 2");
        assert_eq!(name.ids, [5000, 2]);
    }

    #[test]
    fn the_older_nexus_file_name_works_too() {
        let name = nexus_name("Cool HUD-5000-1-2-1790000000").unwrap();
        assert_eq!(name.name, "Cool HUD");
        assert_eq!(name.ids, [5000, 1]);
        assert_eq!(name.uploaded, uploaded_minute(1_790_000_000));
    }

    #[test]
    fn numbers_in_a_mod_name_are_candidates_too() {
        let name = nexus_name("Better-HUD-2-5000-1-0-1790000000").unwrap();
        assert_eq!(name.name, "Better-HUD");
        assert_eq!(name.ids, [2, 5000]);
    }

    #[test]
    fn a_second_download_of_a_file_keeps_its_name() {
        let name = nexus_name("Cool HUD-5000-1-0-1790000000 (1)").unwrap();
        assert_eq!(name.ids, [5000, 1]);
    }

    #[test]
    fn other_names_are_not_nexus_files() {
        assert_eq!(nexus_name("Cool HUD"), None);
        assert_eq!(nexus_name("Cool HUD-1.2"), None);
        assert_eq!(nexus_name("Cool-HUD-1790000000"), None);
        assert_eq!(nexus_name("Cool HUD 1.2 final version"), None);
    }

    #[test]
    fn a_nexus_file_nexus_can_t_confirm_keeps_the_mods_name() {
        let file = unknown_file(Path::new("Auto Collect Loot 70 1.6.0 2026-11-03T21-03Z 8TxGunSsn.zip"));
        assert_eq!(file.name, "Auto Collect Loot");
        assert_eq!(file.mod_id, "file-auto-collect-loot");
    }

    #[test]
    fn an_unknown_file_keeps_its_name() {
        let file = unknown_file(Path::new("C:/Downloads/Cool_HUD v2 (3).zip"));
        assert_eq!(file.name, "Cool HUD v2");
        assert_eq!(file.mod_id, "file-cool-hud-v2");
    }
}

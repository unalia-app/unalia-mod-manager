//! Install from Nexus Mods, frameworks included (REDengine games).
//!
//! 1. The requested mod comes from Nexus: the site's `nxm://` link (free
//!    account) or a direct download (Premium).
//! 2. Its files say which frameworks it needs (`.reds` means redscript…),
//!    Nexus declares the rest in its requirements.
//! 3. Missing frameworks come from GitHub, their authors' source, without
//!    any click: they go before the mod, like dependencies.
//!
//! The whole batch is transactional, like a Thunderstore install.
//!
//! A mod can have several files (color variants, options): the user picks
//! them, they are placed in order, each over the previous one, and form a
//! single mod.
//!
//! A game that stayed on an old version (`before`, release date of the next
//! one) gets the files and releases published before that date.

use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Maximum GitHub repos tried for a mod (each one costs a request).
const MIRROR_CANDIDATES: usize = 4;

use serde::Serialize;

use super::{
    Done, Fetched, InstallError, Installer, Package, Progress, Target, deploy_package, files_of,
    keep_replaced, replaced, rollback,
};
use crate::download;
use crate::games::GameProfile;
use crate::loaders::bethesda::{self, Bethesda};
use crate::loaders::framework::{Framework, needed_frameworks};
use crate::loaders::redengine::RedEngine;
use crate::loaders::unreal::{Unreal, find_binaries, find_project};
use crate::loaders::{Loader, PackageKind, Placement};
use crate::sources::SourceError;
use crate::sources::github::{GitHub, repos_in};
use crate::sources::nexus::{ModFile, ModInfo, Nexus, NxmLink, Requirement, mod_key};
use crate::state::{InstalledMod, State};
use crate::store::Manifest;

/// What the user asked for.
#[derive(Debug, Clone, Copy)]
pub enum NexusFile<'a> {
    /// The mod's main file (Install button, updates).
    Main,
    /// The file picked on the site (`nxm://` link).
    Link(&'a NxmLink),
    /// Files picked in Unalia, placed in this order. Free account: `link`
    /// is the one that just came from the site, the others must already be
    /// downloaded (else `InstallError::NexusWebsite`).
    Files {
        ids: &'a [u32],
        link: Option<&'a NxmLink>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusInstall {
    /// The game's mods after the install.
    pub mods: Vec<InstalledMod>,
    /// Requirements declared on Nexus that Modalia couldn't install by
    /// itself (other Nexus mods: one click on the site for a free account).
    pub missing: Vec<Requirement>,
}

impl Installer<'_, Nexus> {
    /// Installs a Nexus mod and the frameworks it needs.
    #[allow(clippy::too_many_arguments)]
    pub async fn install_nexus(
        &self,
        state: &mut State,
        target: &Target<'_>,
        profile: &GameProfile,
        github: &GitHub,
        mod_id: u32,
        file: NexusFile<'_>,
        before: Option<&str>,
        mut on_progress: impl FnMut(Progress),
    ) -> Result<NexusInstall, InstallError> {
        on_progress(Progress::Resolving);
        let domain = &profile
            .nexus
            .as_ref()
            .ok_or(InstallError::NotInstalled)?
            .domain;
        let existing = state.list(target.game)?;
        let adapter = Adapter::for_game(profile, target.game_dir)?;
        // Framework paths for this game (`{project}` and `{binaries}` of
        // Unreal games).
        let frameworks: Vec<Framework> = profile
            .frameworks
            .iter()
            .map(|f| f.resolve(adapter.project(), adapter.binaries()))
            .collect();

        // The requested mod. A framework without a precise file comes from
        // GitHub: no click on Nexus, even for a free account.
        let framework = frameworks.iter().find(|f| f.nexus_id == Some(mod_id));
        // If GitHub doesn't provide it (release without the expected file),
        // it goes through Nexus like any mod.
        let from_github = match (framework, file) {
            (Some(framework), NexusFile::Main) => {
                self.github_package(github, framework, before).await.ok()
            }
            _ => None,
        };
        let (parts, whole, requirements) = match from_github {
            Some(package) => (vec![package.clone()], package, Vec::new()),
            None => {
                let info = self.source.mod_info(domain, mod_id).await?;
                let id = mod_key(mod_id);
                let previous = existing.iter().find(|m| m.id == id);
                let (ids, link) = match file {
                    NexusFile::Link(link) => (vec![link.file_id], Some(link)),
                    NexusFile::Files { ids, link } => (ids.to_vec(), link),
                    NexusFile::Main => {
                        // Reinstall, update: the files picked last time, in
                        // their current version.
                        let mut kept: Vec<u32> = Vec::new();
                        for old in previous.map(|p| p.files.as_slice()).unwrap_or_default() {
                            if let Some(file) = info.successor(*old, before)
                                && !kept.contains(&file.file_id)
                            {
                                kept.push(file.file_id);
                            }
                        }
                        if kept.is_empty() {
                            let mut choices = info.choices(before);
                            recommend(&mut choices, &frameworks);
                            if choices.len() > 1 && previous.is_none() {
                                return Err(InstallError::ChooseFiles(
                                    choices.into_iter().cloned().collect(),
                                ));
                            }
                            let main = info
                                .file_for(before)
                                .ok_or_else(|| InstallError::NoFile(info.name.clone()))?;
                            kept.push(main.file_id);
                        }
                        (kept, None)
                    }
                };
                let single = ids.len() == 1;
                let mut parts = Vec::new();
                // Free account, several files: the ones that are here are
                // downloaded, the site is asked for the first of the others.
                // The next link picks up from there.
                let mut website = None;
                for (index, file_id) in ids.iter().enumerate() {
                    let file = info
                        .files
                        .iter()
                        .find(|f| f.file_id == *file_id)
                        .cloned()
                        .ok_or_else(|| InstallError::NoFile(info.name.clone()))?;
                    let version = file_version(&info, &file, before);
                    let link = link.filter(|l| l.file_id == *file_id);
                    // Free account (site link): a single connection, like
                    // Vortex. Several would bypass the throttling.
                    let mut connections = link.map(|_| 1);
                    let archive = self.archive_path(&id, &version, Some(*file_id));
                    let cached = self.cached(target.game, &id, &version, Some(*file_id));
                    // Already downloaded (previous file of a multiple
                    // choice, free account): nothing to ask again.
                    let url = if cached {
                        String::new()
                    } else {
                        match self
                            .source
                            .download_url(domain, mod_id, *file_id, link)
                            .await
                        {
                            Ok(url) => url,
                            // Free account: before sending to the site, look
                            // for the same file from the author, elsewhere.
                            Err(SourceError::WebsiteOnly { .. }) if link.is_none() => {
                                let mirror = if single {
                                    self.github_mirror(
                                        github,
                                        profile,
                                        domain,
                                        &info,
                                        &file,
                                        &archive,
                                        before,
                                        &mut on_progress,
                                    )
                                    .await
                                } else {
                                    None
                                };
                                match mirror {
                                    // GitHub: no throttling to respect.
                                    Some(url) => {
                                        connections = None;
                                        url
                                    }
                                    None => {
                                        website.get_or_insert(*file_id);
                                        continue;
                                    }
                                }
                            }
                            Err(error) => return Err(error.into()),
                        }
                    };
                    let part = Package {
                        connections,
                        id: id.clone(),
                        // Several files: the progress says which one.
                        name: if single {
                            info.name.clone()
                        } else {
                            file.name.clone()
                        },
                        author: info.author.clone(),
                        version,
                        icon_url: info.thumbnail_url.clone(),
                        download_url: url,
                        sha256: None,
                        file_id: Some(*file_id),
                    };
                    if !single && !cached {
                        self.fetch_package(target, &part, index, ids.len(), &mut on_progress)
                            .await?;
                    }
                    parts.push(part);
                }
                if let Some(file_id) = website {
                    return Err(InstallError::NexusWebsite { file_id });
                }
                let whole = Package {
                    name: info.name.clone(),
                    file_id: None,
                    ..parts[0].clone()
                };
                (parts, whole, info.requirements)
            }
        };

        // Download the mod, then the missing frameworks (and the ones those
        // frameworks need in turn).
        let main_id = whole.id.clone();
        let chosen: Vec<u32> = parts.iter().filter_map(|p| p.file_id).collect();
        let mut fetched = vec![
            self.fetch_parts(target, &adapter, parts, whole, &mut on_progress)
                .await?,
        ];
        let mut planned: HashSet<String> = HashSet::from([main_id]);
        let mut queue: Vec<&Framework> = Vec::new();
        let declared = frameworks.iter().filter(|f| {
            f.nexus_id
                .is_some_and(|id| requirements.iter().any(|r| r.mod_id == Some(id)))
        });
        queue.extend(declared);
        queue.extend(needed_frameworks(&frameworks, &fetched[0].placements));
        while let Some(framework) = queue.pop() {
            let present = framework.installed_in(target.game_dir)
                || existing.iter().any(|m| m.id == framework.key());
            if present || !planned.insert(framework.key()) {
                continue;
            }
            let package = self.github_package(github, framework, before).await?;
            let index = fetched.len();
            let item = self
                .fetch(
                    target,
                    &adapter,
                    package,
                    index,
                    index + 1,
                    &mut on_progress,
                )
                .await?;
            queue.extend(needed_frameworks(&frameworks, &item.placements));
            fetched.push(item);
        }

        // Last chance to cancel: after this, the game is modified.
        self.check_cancel()?;
        // Place: frameworks first (the last discovered are the deepest),
        // the requested mod last.
        let requested = mod_key(mod_id);
        let total = fetched.len();
        let store = self.paths.store();
        let mut done: Vec<Done> = Vec::new();
        for (index, item) in fetched.into_iter().rev().enumerate() {
            on_progress(Progress::Installing {
                index,
                total,
                name: item.package.name.clone(),
            });
            let mut previous = existing.iter().find(|m| m.id == item.package.id).cloned();
            match deploy_package(&store, target, item, previous.as_mut(), &existing) {
                Ok(record) => done.push(Done { record, previous }),
                Err(error) => {
                    rollback(&store, target.game_dir, &done);
                    return Err(error);
                }
            }
        }
        let replaced = replaced(&done);
        let records: Vec<InstalledMod> = done
            .into_iter()
            .map(|d| {
                let mut record = d.record;
                if record.id == requested {
                    record.files = chosen.clone();
                }
                record.explicit = match &d.previous {
                    Some(previous) => previous.explicit || record.id == requested,
                    None => record.id == requested,
                };
                record
            })
            .collect();
        state.save_all(&records)?;
        keep_replaced(state, self.paths, &replaced)?;

        let mods = state.list(target.game)?;
        let missing = requirements
            .into_iter()
            .filter(|r| match r.mod_id {
                Some(id) => {
                    !frameworks.iter().any(|f| f.nexus_id == Some(id))
                        && !mods.iter().any(|m| m.id == mod_key(id))
                }
                None => r.url.is_some(),
            })
            .collect();
        Ok(NexusInstall { mods, missing })
    }

    /// Source ladder: is the mod's main file published as is on a GitHub
    /// repo mentioned in its description? We don't take it on trust: the
    /// candidate file (same exact size) is downloaded, and Nexus must
    /// recognize its hash as this exact file of this mod. Kept in the
    /// cache, it is not downloaded twice.
    #[allow(clippy::too_many_arguments)]
    async fn github_mirror(
        &self,
        github: &GitHub,
        profile: &GameProfile,
        domain: &str,
        info: &ModInfo,
        file: &ModFile,
        archive: &Path,
        before: Option<&str>,
        on_progress: &mut impl FnMut(Progress),
    ) -> Option<String> {
        let size = file.size?;
        // Framework repos are mentioned as dependencies, not as the mod's
        // source.
        let frameworks: Vec<String> = profile
            .frameworks
            .iter()
            .map(|f| f.github.to_ascii_lowercase())
            .collect();
        // The game profile first, then the repos mentioned by the description.
        let listed = profile
            .mirrors
            .iter()
            .filter(|m| m.nexus_id == info.mod_id)
            .map(|m| m.github.clone());
        let mut repos: Vec<String> = Vec::new();
        for repo in listed.chain(repos_in(&info.description)) {
            let lower = repo.to_ascii_lowercase();
            if !frameworks.contains(&lower) && !repos.iter().any(|r| r.eq_ignore_ascii_case(&repo))
            {
                repos.push(repo);
            }
        }
        let repos = repos.into_iter().take(MIRROR_CANDIDATES);
        for repo in repos {
            let release = match before {
                Some(before) => github.release_before(&repo, before).await,
                None => github.latest_release(&repo).await,
            };
            let Ok(release) = release else {
                continue;
            };
            for asset in release.assets.iter().filter(|a| a.size == size) {
                let _ = std::fs::remove_file(archive);
                let options = download::Options {
                    max_bytes: size,
                    cancel: self.cancel.cloned(),
                    ..download::Options::default()
                };
                let fetched = download::download(
                    self.client,
                    &asset.url,
                    archive,
                    &options,
                    |received, total| {
                        on_progress(Progress::Downloading {
                            index: 0,
                            total: 1,
                            name: info.name.clone(),
                            received,
                            size: total,
                        });
                    },
                )
                .await;
                if fetched.is_ok()
                    && let Ok(md5) = download::md5_file(archive)
                    && let Ok(matches) = self.source.md5_search(domain, &md5).await
                    && matches.contains(&(info.mod_id, file.file_id))
                {
                    return Some(asset.url.clone());
                }
                let _ = std::fs::remove_file(archive);
                super::discard_partial(archive);
            }
        }
        None
    }

    async fn github_package(
        &self,
        github: &GitHub,
        framework: &Framework,
        before: Option<&str>,
    ) -> Result<Package, InstallError> {
        let release = framework_release(github, framework, before).await?;
        let (asset, version) = release.asset_and_version(&framework.asset).ok_or_else(|| {
            InstallError::NoReleaseFile {
                name: framework.name.clone(),
                version: release.version.clone(),
            }
        })?;
        Ok(Package {
            connections: None,
            id: framework.key(),
            name: framework.name.clone(),
            author: framework
                .github
                .split_once('/')
                .map(|(owner, _)| owner.to_owned())
                .unwrap_or_default(),
            version,
            icon_url: None,
            download_url: asset.url.clone(),
            sha256: None,
            file_id: None,
        })
    }

    /// A mod's picked files, each placed on its own then laid one over the
    /// other (a color variant replaces the main file's archive): a single
    /// package, `whole`.
    async fn fetch_parts(
        &self,
        target: &Target<'_>,
        adapter: &Adapter,
        mut parts: Vec<Package>,
        whole: Package,
        on_progress: &mut impl FnMut(Progress),
    ) -> Result<Fetched, InstallError> {
        if parts.len() == 1 {
            return self
                .fetch(target, adapter, parts.remove(0), 0, 1, on_progress)
                .await;
        }
        let total = parts.len();
        let mut placed = Vec::new();
        for (index, part) in parts.iter().enumerate() {
            let (_, manifest) = self
                .fetch_package(target, part, index, total, on_progress)
                .await?;
            let placements = adapter.place(&part.name, &files_of(&manifest))?;
            placed.push((manifest, placements));
        }
        let ids: Vec<String> = parts
            .iter()
            .filter_map(|p| p.file_id)
            .map(|f| f.to_string())
            .collect();
        let key = format!(
            "{}/{}+{}",
            super::mod_prefix(target.game, &whole.id),
            super::path_segment(&whole.version),
            ids.join("+")
        );
        // The combined package mirrors the game: each file in its place.
        // No file copied: it reuses the packages' content.
        let manifest = combine(&placed);
        self.paths.store().save_manifest(&key, &manifest)?;
        let placements = files_of(&manifest)
            .into_iter()
            .map(|path| Placement {
                source: path.clone(),
                target: path,
            })
            .collect();
        Ok(Fetched {
            package: whole,
            key,
            manifest,
            placements,
            loader_root: None,
        })
    }

    async fn fetch(
        &self,
        target: &Target<'_>,
        adapter: &Adapter,
        package: Package,
        index: usize,
        total: usize,
        on_progress: &mut impl FnMut(Progress),
    ) -> Result<Fetched, InstallError> {
        let (key, manifest) = self
            .fetch_package(target, &package, index, total, on_progress)
            .await?;
        let placements = adapter.place(&package.name, &files_of(&manifest))?;
        Ok(Fetched {
            package,
            key,
            manifest,
            placements,
            loader_root: None,
        })
    }
}

/// Combines a mod's files, laid out as in the game: a file placed later
/// replaces the one at the same path (case-insensitive, like Windows). The
/// hash is the first archive's.
fn combine(parts: &[(Manifest, Vec<Placement>)]) -> Manifest {
    let mut combined = Manifest {
        sha256: parts
            .first()
            .map(|(m, _)| m.sha256.clone())
            .unwrap_or_default(),
        files: Default::default(),
    };
    let mut seen: HashMap<String, String> = HashMap::new();
    for (manifest, placements) in parts {
        for p in placements {
            let Some(entry) = manifest.files.get(&p.source) else {
                continue;
            };
            if let Some(earlier) = seen.insert(p.target.to_ascii_lowercase(), p.target.clone()) {
                combined.files.remove(&earlier);
            }
            combined.files.insert(p.target.clone(), entry.clone());
        }
    }
    combined
}

/// Several main files: they are versions of the same mod, only one to
/// install. First (picked by default): the one that mentions a framework
/// Modalia installs ("UE4SS_…"): it works as is, the other may depend on
/// a missing loader. Otherwise the order doesn't change.
fn recommend(choices: &mut [&ModFile], frameworks: &[Framework]) {
    if choices.iter().filter(|f| f.category == "MAIN").count() < 2 {
        return;
    }
    let mentions = |file: &ModFile| {
        let text = format!("{} {}", file.name, file.description).to_lowercase();
        frameworks
            .iter()
            .any(|f| text.contains(&f.name.to_lowercase()))
    };
    choices.sort_by_key(|f| !(f.category == "MAIN" && mentions(f)));
}

/// Release of a framework: its tag's if it has one, else the latest stable
/// one (from before `before` for an old version of the game).
async fn framework_release(
    github: &GitHub,
    framework: &Framework,
    before: Option<&str>,
) -> Result<crate::sources::github::Release, SourceError> {
    match (framework.release.as_deref(), before) {
        (None, Some(before)) => github.release_before(&framework.github, before).await,
        (tag, _) => github.release(&framework.github, tag).await,
    }
}

/// Version of a Nexus file. Without a number, the mod's, except for an old
/// file: its date, which can't be confused with the current version.
fn file_version(info: &ModInfo, file: &ModFile, before: Option<&str>) -> String {
    if !file.version.is_empty() {
        file.version.clone()
    } else if before.is_some() && info.main_file() != Some(file) {
        crate::sources::nexus::iso_date(file.uploaded_at)[..10].to_owned()
    } else {
        info.version.clone()
    }
}

/// Adapter of a Nexus game, from its profile.
enum Adapter {
    Bethesda(Bethesda),
    RedEngine(RedEngine),
    Unreal(Unreal),
}

impl Adapter {
    fn for_game(profile: &GameProfile, game_dir: &Path) -> Result<Self, InstallError> {
        match profile.loader.as_str() {
            "unreal" => {
                let project = find_project(game_dir).ok_or(InstallError::NoGameFiles)?;
                Ok(Self::Unreal(Unreal {
                    binaries: find_binaries(game_dir, &project),
                    project,
                }))
            }
            "bethesda" => Ok(Self::Bethesda(Bethesda)),
            _ => Ok(Self::RedEngine(RedEngine)),
        }
    }

    fn loader(&self) -> &dyn Loader {
        match self {
            Self::Bethesda(loader) => loader,
            Self::RedEngine(loader) => loader,
            Self::Unreal(loader) => loader,
        }
    }

    fn project(&self) -> Option<&str> {
        match self {
            Self::Unreal(loader) => Some(&loader.project),
            Self::Bethesda(_) | Self::RedEngine(_) => None,
        }
    }

    fn binaries(&self) -> Option<&str> {
        match self {
            Self::Unreal(loader) => Some(&loader.binaries),
            Self::Bethesda(_) | Self::RedEngine(_) => None,
        }
    }

    /// Destination of the files of mod `name`. Nothing to place: an error
    /// that says why.
    fn place(&self, name: &str, files: &[String]) -> Result<Vec<Placement>, InstallError> {
        let placements = self.loader().place(name, &PackageKind::Mod, files);
        if !placements.is_empty() {
            Ok(placements)
        } else if matches!(self, Self::Bethesda(_)) && bethesda::has_fomod(files) {
            Err(InstallError::Fomod(name.to_owned()))
        } else {
            Err(InstallError::NothingToInstall(name.to_owned()))
        }
    }
}

/// Updates of a Nexus game's mods: frameworks from their latest GitHub
/// release (where Modalia takes them), the others from Nexus, in one
/// request. An unreachable source doesn't block the other. For an old
/// version of the game (`before`), only what came out before the next
/// one: mod by mod, from its files.
pub async fn available_updates_nexus(
    nexus: &Nexus,
    github: &GitHub,
    profile: &GameProfile,
    installed: &[InstalledMod],
    before: Option<&str>,
) -> Vec<super::Update> {
    use crate::resolver::compare_versions;
    use crate::sources::nexus::parse_mod_key;

    // A build version (`3.0.1-1181-g83330635`) that changes while the
    // number stays the same also counts as an update.
    // For an old version of the game, a mod installed for a newer one is
    // offered too: the "update" goes back.
    let newer = |m: &InstalledMod, latest: &str| {
        let order = compare_versions(latest, &m.version);
        let changed = order == std::cmp::Ordering::Greater
            || ((order == std::cmp::Ordering::Equal || before.is_some()) && latest != m.version);
        changed.then(|| super::Update {
            mod_id: m.id.clone(),
            current: m.version.clone(),
            latest: latest.to_owned(),
        })
    };
    let mut updates = Vec::new();
    let mut from_nexus = Vec::new();
    for m in installed {
        if let Some(framework) = profile.frameworks.iter().find(|f| f.key() == m.id) {
            if let Ok(release) = framework_release(github, framework, before).await
                && let Some((_, version)) = release.asset_and_version(&framework.asset)
            {
                updates.extend(newer(m, &version));
            }
        } else if let Some(id) = parse_mod_key(&m.id) {
            from_nexus.push(id);
        }
    }
    let Some(domain) = profile.nexus.as_ref().map(|n| &n.domain) else {
        return updates;
    };
    if let Some(before) = before {
        for m in installed {
            let Some(id) = parse_mod_key(&m.id).filter(|id| from_nexus.contains(id)) else {
                continue;
            };
            let file_of = |info: &'_ ModInfo| match m.files.first() {
                Some(first) => info.successor(*first, Some(before)).cloned(),
                None => info.file_for(Some(before)).cloned(),
            };
            if let Ok(info) = nexus.mod_info(domain, id).await
                && let Some(file) = file_of(&info)
            {
                updates.extend(newer(m, &file_version(&info, &file, Some(before))));
            }
        }
    } else if let Ok(latest) = nexus.latest_versions(domain, &from_nexus).await {
        for m in installed {
            if let Some(version) = parse_mod_key(&m.id).and_then(|id| latest.get(&id)) {
                updates.extend(newer(m, version));
            }
        }
    }
    updates
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(id: u32, category: &str, name: &str, description: &str) -> ModFile {
        ModFile {
            file_id: id,
            name: name.into(),
            version: "1".into(),
            category: category.into(),
            primary: false,
            size: None,
            uploaded_at: 0,
            description: description.into(),
        }
    }

    fn ue4ss() -> Framework {
        Framework {
            name: "UE4SS".into(),
            nexus_id: None,
            github: "UE4SS-RE/RE-UE4SS".into(),
            release: None,
            asset: "UE4SS_v*.zip".into(),
            marker: "ue4ss/UE4SS.dll".into(),
            needed_by_prefix: vec![],
            needed_by_ext: vec![],
        }
    }

    #[test]
    fn the_main_file_that_names_a_framework_comes_first() {
        // Skip Start Screen (Hogwarts Legacy): two versions of the same mod.
        let apparate = file(
            3280,
            "MAIN",
            "Apparate_SkipStartScreen",
            "Blueprint Apparate Modloader Version",
        );
        let lua = file(3279, "MAIN", "UE4SS_SkipStartScreen", "UE4SS Version");
        let extra = file(3281, "OPTIONAL", "Extra", "");
        let mut choices = vec![&apparate, &extra, &lua];
        recommend(&mut choices, &[ue4ss()]);
        let ids: Vec<u32> = choices.iter().map(|f| f.file_id).collect();
        assert_eq!(ids, [3279, 3280, 3281]);

        // A single main file: the order doesn't change.
        let mut choices = vec![&apparate, &extra];
        recommend(&mut choices, &[ue4ss()]);
        assert_eq!(choices[0].file_id, 3280);
    }
}

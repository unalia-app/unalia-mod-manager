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

/// Where ReShade comes from: its installer, for the game's executable.
const RESHADE_URL: &str = "https://reshade.me";

use serde::Serialize;

use super::{
    variants,
    Done, Fetched, InstallError, Installer, NotAMod, Package, Progress, Target, deploy_package,
    files_of, holders,
    keep_replaced, rank_new, replaced, rollback, settle,
};
use crate::download;
use crate::games::GameProfile;
use crate::loaders::bethesda::{self, Bethesda};
use crate::loaders::framework::{Framework, needed_frameworks};
use crate::loaders::redengine::RedEngine;
use crate::loaders::unreal::{
    Unreal, find_binaries, find_project, is_reshade_preset, reshade_installed, user_settings,
};
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
        variant: Option<&str>,
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
                    NexusFile::Link(link) => (linked_files(&info, previous, link.file_id), Some(link)),
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
        // One folder per variant in the archive: the picked one, else the
        // one installed before, else the user picks (the archive stays
        // cached for that).
        if let Some(variants) = variants::variants_of(&fetched[0].placements) {
            let previous = existing.iter().find(|m| m.id == fetched[0].package.id);
            let pick = variant
                .filter(|v| variants.names.iter().any(|n| n == v))
                .map(str::to_owned)
                .or_else(|| previous.and_then(|p| variants.used_by(&p.placements)));
            match pick {
                Some(pick) => variants.keep(&mut fetched[0].placements, &pick),
                None => return Err(InstallError::ChooseVariant(variants.names)),
            }
        }
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
        let mut held = holders(&existing);
        let mut done: Vec<Done> = Vec::new();
        for (index, item) in fetched.into_iter().rev().enumerate() {
            on_progress(Progress::Installing {
                index,
                total,
                name: item.package.name.clone(),
            });
            let mut previous = existing.iter().find(|m| m.id == item.package.id).cloned();
            match deploy_package(&store, target, item, previous.as_mut(), &mut held) {
                Ok(record) => done.push(Done { record, previous }),
                Err(error) => {
                    rollback(&store, target.game_dir, &done);
                    return Err(error);
                }
            }
        }
        let replaced = replaced(&done);
        let mut records: Vec<InstalledMod> = done
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
        rank_new(state, &mut records, &existing)?;
        state.save_all(&records)?;
        keep_replaced(state, self.paths, &replaced)?;
        settle(state, self.paths, target.game, target.game_dir)?;

        let mods = state.list(target.game)?;
        let required: Vec<Requirement> = requirements
            .into_iter()
            .filter(|r| !r.optional())
            .filter(|r| match r.mod_id {
                Some(id) => !frameworks.iter().any(|f| f.nexus_id == Some(id)),
                None => r.url.is_some(),
            })
            .collect();
        let installed = |r: &Requirement| r.mod_id.is_some_and(|id| mods.iter().any(|m| m.id == mod_key(id)));
        let mut missing: Vec<Requirement> = required.iter().filter(|r| !installed(r)).cloned().collect();
        let present: Vec<Requirement> = required.iter().filter(|r| installed(r)).cloned().collect();
        // A ReShade preset: ReShade comes from its website, not from Unalia.
        let preset = mods.iter().filter(|m| m.id == requested).any(|m| {
            m.placements
                .iter()
                .any(|p| is_reshade_preset(p.target.rsplit('/').next().unwrap_or_default()))
        });
        if let Some(binaries) = adapter.binaries().filter(|_| preset) {
            let declared = |r: &Requirement| r.name.to_ascii_lowercase().contains("reshade");
            if reshade_installed(target.game_dir, binaries) {
                missing.retain(|r| !declared(r));
            } else if !missing.iter().any(declared) {
                missing.push(Requirement {
                    mod_id: None,
                    name: "ReShade".into(),
                    url: Some(RESHADE_URL.into()),
                    notes: String::new(),
                });
            }
        }
        // A REDmod mod: the game loads it with a free DLC, not from Unalia.
        if let Some(modded) = &profile.modded
            && let Some(tool) = &modded.tool
            && mods.iter().filter(|m| m.id == requested).any(|m| {
                m.deployment.files.iter().any(|f| modded.needed_by(f))
            })
        {
            let word = tool.name.split(' ').next().unwrap_or_default().to_ascii_lowercase();
            let declared = |r: &Requirement| r.name.to_ascii_lowercase().contains(&word);
            if target.game_dir.join(&tool.path).is_file() {
                missing.retain(|r| !declared(r));
            } else if !missing.iter().any(declared) {
                missing.push(Requirement {
                    mod_id: None,
                    name: tool.name.clone(),
                    url: Some(tool.url.clone()),
                    notes: String::new(),
                });
            }
        }
        // Kept with the mod, installed or not: the UI shows what's missing
        // as long as it is, and what comes later goes under this mod.
        if let Some(mut record) = state.get(target.game, &requested)? {
            record.needs = missing.iter().chain(&present).cloned().collect();
            state.save(&record)?;
        }
        let mods = state.list(target.game)?;
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
                    settings: user_settings(&project),
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
        } else if let Some(kind) = NotAMod::of(files) {
            Err(InstallError::NotAMod {
                name: name.to_owned(),
                kind,
            })
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
    use crate::sources::nexus::parse_mod_key;

    let newer = |m: &InstalledMod, latest: &str| {
        is_update(&m.version, latest, before.is_some()).then(|| super::Update {
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
            let Some(id) = parse_mod_key(&m.id) else {
                continue;
            };
            let Some(update) = latest.get(&id).and_then(|version| newer(m, version)) else {
                continue;
            };
            // The page's version is the main file's. Installed without it,
            // a mod follows its own file: that is what an update installs.
            let own = match m.files.first() {
                Some(first) => nexus.mod_info(domain, id).await.ok().and_then(|info| {
                    let file = info.successor(*first, None)?;
                    (file.category != "MAIN").then(|| file_version(&info, file, None))
                }),
                None => None,
            };
            match own {
                Some(version) => updates.extend(newer(m, &version)),
                None => updates.push(update),
            }
        }
    }
    updates
}

/// What a site link installs. An optional file is an add-on (a patch for
/// another mod): it joins the files already installed, and a main file
/// keeps them, instead of replacing the mod. Another version of the same
/// file still replaces it. The main file comes first, its version is the
/// mod's.
fn linked_files(info: &ModInfo, previous: Option<&InstalledMod>, linked: u32) -> Vec<u32> {
    let category = |id: u32| info.files.iter().find(|f| f.file_id == id).map(|f| f.category.as_str());
    let add_on = |id: u32| matches!(category(id), Some("OPTIONAL" | "MISCELLANEOUS"));
    let joins = add_on(linked);
    let kept = previous
        .map(|p| p.files.as_slice())
        .unwrap_or_default()
        .iter()
        .copied()
        .filter(|old| category(*old).is_some() && !info.same_file(*old, linked))
        .filter(|old| joins || add_on(*old));
    if joins {
        kept.chain([linked]).collect()
    } else {
        [linked].into_iter().chain(kept).collect()
    }
}

/// Whether `latest` replaces the installed `current`. A build version
/// (`3.0.1-1181-g83330635`) that changes while the number stays the same
/// also counts. For an old version of the game (`going_back`), a mod
/// installed for a newer one is offered too: the "update" goes back.
fn is_update(current: &str, latest: &str, going_back: bool) -> bool {
    use crate::resolver::compare_versions;
    use std::cmp::Ordering;

    // `1.0` on the mod page and `1.0.0` on its file are the same version:
    // only a text that isn't plain numbers (a build) is compared as is.
    let numeric = |v: &str| v.split('.').all(|p| p.parse::<u64>().is_ok());
    let same = current == latest
        || (numeric(current)
            && numeric(latest)
            && compare_versions(current, latest) == Ordering::Equal);
    let order = compare_versions(latest, current);
    order == Ordering::Greater || ((order == Ordering::Equal || going_back) && !same)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_number_written_differently_is_no_update() {
        assert!(!is_update("1.0.0", "1.0", false));
        assert!(!is_update("1", "1.0.0", true));
        assert!(is_update("1.0.0", "1.0.1", false));
        assert!(!is_update("1.0.1", "1.0", false));
        assert!(is_update("1.0.1", "1.0", true));
        assert!(is_update("3.0.1-1181-g83330635", "3.0.1-1183-gcdbc0b66", false));
        assert!(!is_update("3.0.1-1183-gcdbc0b66", "3.0.1-1183-gcdbc0b66", false));
    }

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

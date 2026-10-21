//! Installation depuis Nexus Mods, frameworks compris (jeux REDengine).
//!
//! 1. Le mod demandé vient de Nexus : lien `nxm://` du site (compte
//!    gratuit) ou téléchargement direct (Premium).
//! 2. Ses fichiers disent de quels frameworks il a besoin (`.reds` →
//!    redscript…), Nexus déclare le reste dans ses prérequis.
//! 3. Les frameworks absents viennent de GitHub, la source de leurs auteurs,
//!    sans aucun clic : ils passent avant le mod, comme des dépendances.
//!
//! Tout le lot est transactionnel, comme une installation Thunderstore.

use std::collections::HashSet;

use serde::Serialize;

use super::{
    Done, Fetched, InstallError, Installer, Package, Progress, Target, deploy_package, list_files,
    rollback,
};
use crate::games::GameProfile;
use crate::loaders::redengine::{Framework, RedEngine, needed_frameworks};
use crate::loaders::{Loader, PackageKind};
use crate::sources::github::GitHub;
use crate::sources::nexus::{Nexus, NxmLink, Requirement, mod_key};
use crate::state::{InstalledMod, State};

/// Ce que l'utilisateur a demandé.
#[derive(Debug, Clone, Copy)]
pub enum NexusFile<'a> {
    /// Le fichier principal du mod (bouton Install, mises à jour).
    Main,
    /// Le fichier choisi sur le site (lien `nxm://`).
    Link(&'a NxmLink),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusInstall {
    /// Mods du jeu après l'installation.
    pub mods: Vec<InstalledMod>,
    /// Prérequis déclarés sur Nexus que Modalia n'a pas pu installer seul
    /// (autres mods Nexus : un clic sur le site pour un compte gratuit).
    pub missing: Vec<Requirement>,
}

impl Installer<'_, Nexus> {
    /// Installe un mod Nexus et les frameworks dont il a besoin.
    #[allow(clippy::too_many_arguments)]
    pub async fn install_nexus(
        &self,
        state: &mut State,
        target: &Target<'_>,
        profile: &GameProfile,
        github: &GitHub,
        mod_id: u32,
        file: NexusFile<'_>,
        mut on_progress: impl FnMut(Progress),
    ) -> Result<NexusInstall, InstallError> {
        on_progress(Progress::Resolving);
        let domain = &profile
            .nexus
            .as_ref()
            .ok_or(InstallError::NotInstalled)?
            .domain;
        let existing = state.list(target.game)?;

        // Le mod demandé. Un framework sans fichier précis vient de GitHub :
        // pas de clic sur Nexus, même pour un compte gratuit.
        let framework = profile.frameworks.iter().find(|f| f.nexus_id == mod_id);
        let (main, requirements) = match (framework, file) {
            (Some(framework), NexusFile::Main) => {
                (self.github_package(github, framework).await?, Vec::new())
            }
            _ => {
                let info = self.source.mod_info(domain, mod_id).await?;
                let link = match file {
                    NexusFile::Link(link) => Some(link),
                    NexusFile::Main => None,
                };
                let file = match link {
                    Some(link) => info.files.iter().find(|f| f.file_id == link.file_id),
                    None => info.main_file(),
                }
                .cloned()
                .ok_or_else(|| InstallError::NoFile(info.name.clone()))?;
                let url = self
                    .source
                    .download_url(domain, mod_id, file.file_id, link)
                    .await?;
                let version = if file.version.is_empty() {
                    info.version.clone()
                } else {
                    file.version.clone()
                };
                let package = Package {
                    id: mod_key(mod_id),
                    name: info.name.clone(),
                    author: info.author.clone(),
                    version,
                    icon_url: info.thumbnail_url.clone(),
                    download_url: url,
                    sha256: None,
                };
                (package, info.requirements)
            }
        };

        // Télécharger le mod, puis les frameworks qui manquent (et ceux
        // dont ces frameworks ont besoin à leur tour).
        let mut fetched = vec![self.fetch(target, main, 0, 1, &mut on_progress).await?];
        let mut planned: HashSet<u32> = HashSet::from([mod_id]);
        let mut queue: Vec<&Framework> = Vec::new();
        let declared = profile
            .frameworks
            .iter()
            .filter(|f| requirements.iter().any(|r| r.mod_id == Some(f.nexus_id)));
        queue.extend(declared);
        queue.extend(needed_frameworks(
            &profile.frameworks,
            &fetched[0].placements,
        ));
        while let Some(framework) = queue.pop() {
            let present = framework.installed_in(target.game_dir)
                || existing.iter().any(|m| m.id == mod_key(framework.nexus_id));
            if present || !planned.insert(framework.nexus_id) {
                continue;
            }
            let package = self.github_package(github, framework).await?;
            let index = fetched.len();
            let item = self
                .fetch(target, package, index, index + 1, &mut on_progress)
                .await?;
            queue.extend(needed_frameworks(&profile.frameworks, &item.placements));
            fetched.push(item);
        }

        // Poser : frameworks d'abord (les derniers découverts sont les plus
        // profonds), le mod demandé en dernier.
        let requested = mod_key(mod_id);
        let total = fetched.len();
        let mut done: Vec<Done> = Vec::new();
        for (index, item) in fetched.into_iter().rev().enumerate() {
            on_progress(Progress::Installing {
                index,
                total,
                name: item.package.name.clone(),
            });
            let previous = existing.iter().find(|m| m.id == item.package.id).cloned();
            match deploy_package(target, item, previous.as_ref(), &existing) {
                Ok(record) => done.push(Done { record, previous }),
                Err(error) => {
                    rollback(target.game_dir, &done);
                    return Err(error);
                }
            }
        }
        let records: Vec<InstalledMod> = done
            .into_iter()
            .map(|d| {
                let mut record = d.record;
                record.explicit = match &d.previous {
                    Some(previous) => previous.explicit || record.id == requested,
                    None => record.id == requested,
                };
                record
            })
            .collect();
        state.save_all(&records)?;

        let mods = state.list(target.game)?;
        let missing = requirements
            .into_iter()
            .filter(|r| match r.mod_id {
                Some(id) => {
                    !profile.frameworks.iter().any(|f| f.nexus_id == id)
                        && !mods.iter().any(|m| m.id == mod_key(id))
                }
                None => r.url.is_some(),
            })
            .collect();
        Ok(NexusInstall { mods, missing })
    }

    async fn github_package(
        &self,
        github: &GitHub,
        framework: &Framework,
    ) -> Result<Package, InstallError> {
        let release = github.latest_release(&framework.github).await?;
        let asset = release
            .asset(&framework.asset)
            .ok_or_else(|| InstallError::NoFile(framework.name.clone()))?;
        Ok(Package {
            id: mod_key(framework.nexus_id),
            name: framework.name.clone(),
            author: framework
                .github
                .split_once('/')
                .map(|(owner, _)| owner.to_owned())
                .unwrap_or_default(),
            version: release.version.clone(),
            icon_url: None,
            download_url: asset.url.clone(),
            sha256: None,
        })
    }

    async fn fetch(
        &self,
        target: &Target<'_>,
        package: Package,
        index: usize,
        total: usize,
        on_progress: &mut impl FnMut(Progress),
    ) -> Result<Fetched, InstallError> {
        let (store_dir, sha256) = self
            .fetch_package(target, &package, index, total, on_progress)
            .await?;
        let placements =
            RedEngine.place(&package.name, &PackageKind::Mod, &list_files(&store_dir)?);
        if placements.is_empty() {
            return Err(InstallError::NothingToInstall(package.name));
        }
        Ok(Fetched {
            package,
            store_dir,
            sha256,
            placements,
            loader_root: None,
        })
    }
}

/// Mises à jour des mods d'un jeu Nexus : les frameworks d'après leur
/// dernière release GitHub (là où Modalia les prend), les autres d'après
/// Nexus, en une requête. Une source injoignable n'empêche pas l'autre.
pub async fn available_updates_nexus(
    nexus: &Nexus,
    github: &GitHub,
    profile: &GameProfile,
    installed: &[InstalledMod],
) -> Vec<super::Update> {
    use crate::resolver::compare_versions;
    use crate::sources::nexus::parse_mod_key;

    let newer = |m: &InstalledMod, latest: &str| {
        (compare_versions(latest, &m.version) == std::cmp::Ordering::Greater).then(|| {
            super::Update {
                mod_id: m.id.clone(),
                current: m.version.clone(),
                latest: latest.to_owned(),
            }
        })
    };
    let mut updates = Vec::new();
    let mut from_nexus = Vec::new();
    for m in installed {
        let Some(id) = parse_mod_key(&m.id) else {
            continue;
        };
        match profile.frameworks.iter().find(|f| f.nexus_id == id) {
            Some(framework) => {
                if let Ok(release) = github.latest_release(&framework.github).await {
                    updates.extend(newer(m, &release.version));
                }
            }
            None => from_nexus.push(id),
        }
    }
    if let Some(domain) = profile.nexus.as_ref().map(|n| &n.domain)
        && let Ok(latest) = nexus.latest_versions(domain, &from_nexus).await
    {
        for m in installed {
            if let Some(version) = parse_mod_key(&m.id).and_then(|id| latest.get(&id)) {
                updates.extend(newer(m, version));
            }
        }
    }
    updates
}

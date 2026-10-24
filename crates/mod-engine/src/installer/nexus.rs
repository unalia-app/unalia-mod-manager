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
//!
//! Un mod peut avoir plusieurs fichiers (variantes de couleur, options) :
//! l'utilisateur les choisit, ils sont posés dans l'ordre, chacun par-dessus
//! le précédent, et forment un seul mod.
//!
//! Un jeu resté sur une ancienne version (`before`, date de sortie de la
//! suivante) reçoit les fichiers et les releases publiés avant cette date.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Dépôts GitHub essayés au plus pour un mod (chacun coûte une requête).
const MIRROR_CANDIDATES: usize = 4;

use serde::Serialize;

use super::{
    Done, Fetched, InstallError, Installer, Package, Progress, Target, deploy_package, list_files,
    rollback,
};
use crate::download;
use crate::games::GameProfile;
use crate::loaders::framework::{Framework, needed_frameworks};
use crate::loaders::redengine::RedEngine;
use crate::loaders::unreal::{Unreal, find_project};
use crate::loaders::{Loader, PackageKind, Placement};
use crate::sources::SourceError;
use crate::sources::github::{GitHub, repos_in};
use crate::sources::nexus::{ModFile, ModInfo, Nexus, NxmLink, Requirement, mod_key};
use crate::state::{InstalledMod, State};

/// Ce que l'utilisateur a demandé.
#[derive(Debug, Clone, Copy)]
pub enum NexusFile<'a> {
    /// Le fichier principal du mod (bouton Install, mises à jour).
    Main,
    /// Le fichier choisi sur le site (lien `nxm://`).
    Link(&'a NxmLink),
    /// Fichiers choisis dans Unalia, posés dans cet ordre. Compte gratuit :
    /// `link` est celui qui vient d'arriver du site, les autres doivent déjà
    /// être téléchargés (sinon `InstallError::NexusWebsite`).
    Files {
        ids: &'a [u32],
        link: Option<&'a NxmLink>,
    },
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
        // Chemins des frameworks pour ce jeu (`{project}` des jeux Unreal).
        let frameworks: Vec<Framework> = profile
            .frameworks
            .iter()
            .map(|f| f.resolve(adapter.project()))
            .collect();

        // Le mod demandé. Un framework sans fichier précis vient de GitHub :
        // pas de clic sur Nexus, même pour un compte gratuit.
        let framework = frameworks.iter().find(|f| f.nexus_id == Some(mod_id));
        // Si GitHub ne le fournit pas (release sans le fichier attendu), il
        // passe par Nexus comme n'importe quel mod.
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
                        // Réinstallation, mise à jour : les fichiers choisis
                        // la dernière fois, dans leur version actuelle.
                        let mut kept: Vec<u32> = Vec::new();
                        for old in previous.map(|p| p.files.as_slice()).unwrap_or_default() {
                            if let Some(file) = info.successor(*old, before)
                                && !kept.contains(&file.file_id)
                            {
                                kept.push(file.file_id);
                            }
                        }
                        if kept.is_empty() {
                            let choices = info.choices(before);
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
                // Compte gratuit, plusieurs fichiers : ceux qui viennent
                // sont téléchargés, le site est demandé pour le premier des
                // autres. Le lien suivant reprend là.
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
                    // Compte gratuit (lien du site) : une seule connexion,
                    // comme Vortex. Plusieurs contourneraient le bridage.
                    let mut connections = link.map(|_| 1);
                    let archive = self.archive_path(&id, &version, Some(*file_id));
                    // Déjà téléchargé (fichier précédent d'un choix
                    // multiple, compte gratuit) : rien à redemander.
                    let url = if archive.is_file() {
                        String::new()
                    } else {
                        match self
                            .source
                            .download_url(domain, mod_id, *file_id, link)
                            .await
                        {
                            Ok(url) => url,
                            // Compte gratuit : avant d'envoyer sur le site, on
                            // cherche le même fichier chez l'auteur, ailleurs.
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
                                    // GitHub : pas de bridage à respecter.
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
                        // Plusieurs fichiers : la progression dit lequel.
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
                    if !single && !archive.is_file() {
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

        // Télécharger le mod, puis les frameworks qui manquent (et ceux
        // dont ces frameworks ont besoin à leur tour).
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

        // Dernière occasion d'annuler : ensuite, le jeu est modifié.
        self.check_cancel()?;
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

    /// Échelle de sources : le fichier principal du mod est-il publié tel
    /// quel sur un dépôt GitHub cité dans sa description ? On ne le croit
    /// pas sur parole : le fichier candidat (même taille exacte) est
    /// téléchargé, et Nexus doit reconnaître son empreinte comme ce fichier
    /// précis de ce mod. Gardé en cache, il n'est pas téléchargé deux fois.
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
        // Les dépôts des frameworks sont cités comme dépendances, pas comme
        // source du mod.
        let frameworks: Vec<String> = profile
            .frameworks
            .iter()
            .map(|f| f.github.to_ascii_lowercase())
            .collect();
        // Le profil du jeu d'abord, puis les dépôts cités par la description.
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

    /// Les fichiers choisis d'un mod, chacun placé à part puis posés l'un
    /// sur l'autre (une variante de couleur remplace l'archive du fichier
    /// principal) : un seul paquet, `whole`.
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
        let mut sha256 = String::new();
        for (index, part) in parts.iter().enumerate() {
            let (dir, sum) = self
                .fetch_package(target, part, index, total, on_progress)
                .await?;
            let placements =
                adapter
                    .loader()
                    .place(&part.name, &PackageKind::Mod, &list_files(&dir)?);
            if placements.is_empty() {
                return Err(InstallError::NothingToInstall(part.name.clone()));
            }
            if index == 0 {
                sha256 = sum;
            }
            placed.push((dir, placements));
        }
        let ids: Vec<String> = parts
            .iter()
            .filter_map(|p| p.file_id)
            .map(|f| f.to_string())
            .collect();
        let parent = placed[0]
            .0
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let combined = parent.join(format!(
            "{}+{}",
            super::path_segment(&whole.version),
            ids.join("+")
        ));
        // Déjà composé (même version, mêmes fichiers) : réutilisé tel quel.
        if !combined.is_dir() {
            combine(&placed, &combined)?;
        }
        // Le dossier composé reproduit le jeu : chaque fichier à sa place.
        let placements = list_files(&combined)?
            .into_iter()
            .map(|path| Placement {
                source: path.clone(),
                target: path,
            })
            .collect();
        Ok(Fetched {
            package: whole,
            store_dir: combined,
            sha256,
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
        let (store_dir, sha256) = self
            .fetch_package(target, &package, index, total, on_progress)
            .await?;
        let placements =
            adapter
                .loader()
                .place(&package.name, &PackageKind::Mod, &list_files(&store_dir)?);
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

/// Compose les fichiers d'un mod dans `into`, rangés comme dans le jeu : un
/// fichier posé plus tard remplace celui du même chemin (sans tenir compte
/// de la casse, comme Windows). Liens physiques quand c'est possible.
fn combine(parts: &[(PathBuf, Vec<Placement>)], into: &Path) -> std::io::Result<()> {
    let mut chosen: Vec<(String, PathBuf)> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for (dir, placements) in parts {
        for p in placements {
            let source = dir.join(&p.source);
            match seen.get(&p.target.to_ascii_lowercase()) {
                Some(&at) => chosen[at] = (p.target.clone(), source),
                None => {
                    seen.insert(p.target.to_ascii_lowercase(), chosen.len());
                    chosen.push((p.target.clone(), source));
                }
            }
        }
    }
    let tmp = into.with_extension("partial");
    let _ = fs::remove_dir_all(&tmp);
    for (target, source) in &chosen {
        let dest = tmp.join(target);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        if fs::hard_link(source, &dest).is_err() {
            fs::copy(source, &dest)?;
        }
    }
    fs::rename(&tmp, into)
}

/// Release d'un framework : celle de son tag s'il en a un, sinon la
/// dernière stable (d'avant `before` pour une ancienne version du jeu).
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

/// Version d'un fichier Nexus. Sans numéro, celle du mod, sauf pour un
/// ancien fichier : sa date, qui ne se confond pas avec la version actuelle.
fn file_version(info: &ModInfo, file: &ModFile, before: Option<&str>) -> String {
    if !file.version.is_empty() {
        file.version.clone()
    } else if before.is_some() && info.main_file() != Some(file) {
        crate::sources::nexus::iso_date(file.uploaded_at)[..10].to_owned()
    } else {
        info.version.clone()
    }
}

/// Adaptateur d'un jeu Nexus, d'après son profil.
enum Adapter {
    RedEngine(RedEngine),
    Unreal(Unreal),
}

impl Adapter {
    fn for_game(profile: &GameProfile, game_dir: &Path) -> Result<Self, InstallError> {
        match profile.loader.as_str() {
            "unreal" => {
                let project = find_project(game_dir).ok_or(InstallError::NoGameFiles)?;
                Ok(Self::Unreal(Unreal { project }))
            }
            _ => Ok(Self::RedEngine(RedEngine)),
        }
    }

    fn loader(&self) -> &dyn Loader {
        match self {
            Self::RedEngine(loader) => loader,
            Self::Unreal(loader) => loader,
        }
    }

    fn project(&self) -> Option<&str> {
        match self {
            Self::Unreal(loader) => Some(&loader.project),
            Self::RedEngine(_) => None,
        }
    }
}

/// Mises à jour des mods d'un jeu Nexus : les frameworks d'après leur
/// dernière release GitHub (là où Modalia les prend), les autres d'après
/// Nexus, en une requête. Une source injoignable n'empêche pas l'autre.
/// Pour une ancienne version du jeu (`before`), seulement ce qui est sorti
/// avant la suivante : mod par mod, d'après ses fichiers.
pub async fn available_updates_nexus(
    nexus: &Nexus,
    github: &GitHub,
    profile: &GameProfile,
    installed: &[InstalledMod],
    before: Option<&str>,
) -> Vec<super::Update> {
    use crate::resolver::compare_versions;
    use crate::sources::nexus::parse_mod_key;

    // Une version de build (`3.0.1-1181-g83330635`) qui change sans que le
    // numéro bouge compte aussi comme une mise à jour.
    // Pour une ancienne version du jeu, un mod installé pour une plus
    // récente est proposé aussi : la « mise à jour » revient en arrière.
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

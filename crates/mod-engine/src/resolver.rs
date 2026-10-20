//! Brique **resolver** : de « je veux ce mod » à la liste ordonnée de ce
//! qu'il faut installer.
//!
//! - dépendances d'abord, le mod demandé en dernier ;
//! - chaque paquet une seule fois, même s'il est requis par plusieurs mods ;
//! - ce qui est déjà installé dans une version suffisante est sauté ;
//! - **un seul loader** : certains jeux ont plusieurs paquets loader
//!   concurrents (Silksong : `BepInEx-BepInExPack_Silksong` et
//!   `silksong_modding-BepInExPack_Silksong`). Le premier rencontré (ou
//!   celui déjà installé) satisfait toutes les demandes de loader ;
//! - les cycles et les arbres démesurés sont refusés.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::future::Future;

use crate::sources::{Mod, ModVersion, SourceError};

/// Au-delà, l'arbre est anormal (ou malveillant) : on arrête.
const MAX_PACKAGES: usize = 250;

#[derive(Debug, Clone, PartialEq)]
pub struct PlanStep {
    pub id: String,
    pub name: String,
    pub version: ModVersion,
    /// Paquet loader (s'installe à la racine du jeu).
    pub loader: bool,
    /// Déjà installé dans une version plus ancienne : mise à jour.
    pub replaces: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct InstallPlan {
    /// Dans l'ordre d'installation.
    pub steps: Vec<PlanStep>,
}

impl InstallPlan {
    /// Ce que l'utilisateur voit comme « + N dépendances ».
    pub fn dependency_count(&self) -> usize {
        self.steps.len().saturating_sub(1)
    }
}

/// Ce qui est déjà dans le jeu : identifiant → version installée.
#[derive(Debug, Clone, Default)]
pub struct Installed {
    pub versions: HashMap<String, String>,
    /// Paquet loader déjà installé, s'il y en a un.
    pub loader: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("couldn't get {id}")]
    Source {
        id: String,
        #[source]
        source: SourceError,
    },
    #[error("{0} has no downloadable version")]
    NoVersion(String),
    #[error("{id} needs {dependency} {required} or newer, but the newest is {available}")]
    TooOld {
        id: String,
        dependency: String,
        required: String,
        available: String,
    },
    #[error("circular dependency through {0}")]
    Cycle(String),
    #[error("too many dependencies (more than {MAX_PACKAGES})")]
    TooMany,
}

/// Calcule le plan d'installation de `root`. `fetch` donne la fiche d'un
/// paquet (source de mods), `is_loader` dit si un paquet est un loader
/// (catalogue de jeux).
pub async fn resolve<F, Fut>(
    root: &str,
    installed: &Installed,
    fetch: F,
    is_loader: impl Fn(&str) -> bool,
) -> Result<InstallPlan, ResolveError>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Result<Mod, SourceError>>,
{
    let mut state = State {
        installed,
        is_loader: &is_loader,
        loader: installed.loader.clone(),
        planned: HashSet::new(),
        visiting: Vec::new(),
        cache: HashMap::new(),
        steps: Vec::new(),
    };
    state.visit(root.to_owned(), None, &fetch).await?;
    Ok(InstallPlan { steps: state.steps })
}

struct State<'a, L> {
    installed: &'a Installed,
    is_loader: &'a L,
    loader: Option<String>,
    planned: HashSet<String>,
    /// Pile du parcours en cours, pour détecter les cycles.
    visiting: Vec<String>,
    cache: HashMap<String, Mod>,
    steps: Vec<PlanStep>,
}

impl<L: Fn(&str) -> bool> State<'_, L> {
    /// Parcours en profondeur, post-ordre : les dépendances d'un paquet sont
    /// ajoutées avant lui. `required` : version minimale demandée par le
    /// parent (`None` pour le mod demandé).
    async fn visit<F, Fut>(
        &mut self,
        id: String,
        required: Option<(String, String)>,
        fetch: &F,
    ) -> Result<(), ResolveError>
    where
        F: Fn(String) -> Fut,
        Fut: Future<Output = Result<Mod, SourceError>>,
    {
        let loader = (self.is_loader)(&id);
        if loader && self.loader.as_ref().is_some_and(|chosen| chosen != &id) {
            // Un autre loader est déjà là ou prévu : il fait l'affaire.
            return Ok(());
        }
        if self.planned.contains(&id) {
            return Ok(());
        }
        if self.visiting.contains(&id) {
            return Err(ResolveError::Cycle(id));
        }
        if let (Some(have), Some((_, min))) = (self.installed.versions.get(&id), &required)
            && compare_versions(have, min) != Ordering::Less
        {
            return Ok(());
        }
        if self.planned.len() + self.visiting.len() >= MAX_PACKAGES {
            return Err(ResolveError::TooMany);
        }

        let package = match self.cache.get(&id) {
            Some(p) => p.clone(),
            None => {
                let p = fetch(id.clone())
                    .await
                    .map_err(|source| ResolveError::Source {
                        id: id.clone(),
                        source,
                    })?;
                self.cache.insert(id.clone(), p.clone());
                p
            }
        };
        let version = package
            .latest()
            .cloned()
            .ok_or_else(|| ResolveError::NoVersion(id.clone()))?;
        if let Some((parent, min)) = &required
            && compare_versions(&version.number, min) == Ordering::Less
        {
            return Err(ResolveError::TooOld {
                id: parent.clone(),
                dependency: id,
                required: min.clone(),
                available: version.number,
            });
        }
        // Le mod demandé déjà installé à jour : rien à faire.
        if required.is_none()
            && self
                .installed
                .versions
                .get(&id)
                .is_some_and(|have| compare_versions(have, &version.number) != Ordering::Less)
        {
            return Ok(());
        }
        if loader {
            self.loader = Some(id.clone());
        }

        self.visiting.push(id.clone());
        for dep in &version.dependencies {
            Box::pin(self.visit(
                dep.id.clone(),
                Some((id.clone(), dep.min_version.clone())),
                fetch,
            ))
            .await?;
        }
        self.visiting.pop();

        self.planned.insert(id.clone());
        self.steps.push(PlanStep {
            replaces: self.installed.versions.get(&id).cloned(),
            name: package.name,
            id,
            version,
            loader,
        });
        Ok(())
    }
}

/// Compare deux numéros `1.2.3` composante par composante (numérique),
/// ce que fait Thunderstore. Ce qui n'est pas un nombre compte pour 0.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let parse = |v: &str| -> Vec<u64> { v.split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    let (a, b) = (parse(a), parse(b));
    for i in 0..a.len().max(b.len()) {
        match a.get(i).unwrap_or(&0).cmp(b.get(i).unwrap_or(&0)) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    Ordering::Equal
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert_eq!(compare_versions("1.10.0", "1.9.9"), Ordering::Greater);
        assert_eq!(compare_versions("5.4.2304", "5.4.2304"), Ordering::Equal);
        assert_eq!(compare_versions("1.0", "1.0.0"), Ordering::Equal);
        assert_eq!(compare_versions("0.6.7", "1.0.0"), Ordering::Less);
    }
}

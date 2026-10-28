//! **resolver** block: from "I want this mod" to the ordered list of what
//! needs to be installed.
//!
//! - dependencies first, the requested mod last;
//! - each package once, even if several mods require it;
//! - what is already installed in a recent enough version is skipped;
//! - **a single loader**: some games have several competing loader packages
//!   (Silksong: `BepInEx-BepInExPack_Silksong` and
//!   `silksong_modding-BepInExPack_Silksong`). The first one met (or the one
//!   already installed) satisfies every loader request;
//! - cycles and oversized trees are refused.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::future::Future;

use crate::sources::{Mod, ModVersion, SourceError};

/// Beyond this, the tree is abnormal (or malicious): we stop.
const MAX_PACKAGES: usize = 250;

#[derive(Debug, Clone, PartialEq)]
pub struct PlanStep {
    pub id: String,
    pub name: String,
    pub version: ModVersion,
    /// Loader package (installs at the game root).
    pub loader: bool,
    /// Already installed in an older version: update.
    pub replaces: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct InstallPlan {
    /// In install order.
    pub steps: Vec<PlanStep>,
}

impl InstallPlan {
    /// What the user sees as "+ N dependencies".
    pub fn dependency_count(&self) -> usize {
        self.steps.len().saturating_sub(1)
    }
}

/// What is already in the game: id to installed version.
#[derive(Debug, Clone, Default)]
pub struct Installed {
    pub versions: HashMap<String, String>,
    /// Loader package already installed, if any.
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

/// Computes the install plan of `root`. `fetch` gives a package's page (mod
/// source), `is_loader` says whether a package is a loader (game
/// catalog).
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
    /// Stack of the current walk, to detect cycles.
    visiting: Vec<String>,
    cache: HashMap<String, Mod>,
    steps: Vec<PlanStep>,
}

impl<L: Fn(&str) -> bool> State<'_, L> {
    /// Depth-first walk, post-order: a package's dependencies are added
    /// before it. `required`: minimum version asked by the parent (`None`
    /// for the requested mod).
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
            // Another loader is already there or planned: it will do.
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
        // The requested mod is already installed and up to date: nothing to do.
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

/// Compares two `1.2.3` numbers component by component (numerically), as
/// Thunderstore does. Anything that isn't a number counts as 0.
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

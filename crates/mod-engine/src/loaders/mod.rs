//! **Loaders** block: how a mod installs into a game.
//!
//! A loader decides where each file of a package goes. It is code, kept to a
//! few loaders, reviewed by the maintainer with extra care.

pub mod bepinex;
pub mod bethesda;
pub mod framework;
pub mod redengine;
pub mod rules;
pub mod unreal;

use std::path::Path;

use serde::{Deserialize, Serialize};

/// A file of the extracted package, and its destination in the game folder.
/// Relative paths with `/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placement {
    pub source: String,
    pub target: String,
}

/// What we install: a mod, or the loader itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageKind {
    Mod,
    /// Loader package: the content of `root_folder` goes to the game root.
    Loader {
        root_folder: String,
    },
}

pub trait Loader {
    fn id(&self) -> &'static str;

    /// Is the loader already in the game, installed outside Modalia?
    fn installed_manually(&self, game_dir: &Path) -> bool;

    /// Destination of each file of the package `package_id` (e.g.
    /// `Author-Name`). Files without a destination are skipped.
    fn place(&self, package_id: &str, kind: &PackageKind, files: &[String]) -> Vec<Placement>;
}

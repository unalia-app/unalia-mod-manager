//! Brique **loaders** : comment un mod s'installe dans un jeu.
//!
//! Un loader décide où va chaque fichier d'un paquet. C'est du code, peu
//! nombreux, relu par le mainteneur (SPEC §6.3, §7 : revue renforcée).

pub mod bepinex;
pub mod rules;

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Un fichier du paquet extrait, et sa destination dans le dossier du jeu.
/// Chemins relatifs avec des `/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placement {
    pub source: String,
    pub target: String,
}

/// Ce que l'on installe : un mod, ou le loader lui-même.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageKind {
    Mod,
    /// Paquet loader : le contenu de `root_folder` va à la racine du jeu.
    Loader {
        root_folder: String,
    },
}

pub trait Loader {
    fn id(&self) -> &'static str;

    /// Le loader est-il déjà présent dans le jeu, installé hors de Modalia ?
    fn installed_manually(&self, game_dir: &Path) -> bool;

    /// Destination de chaque fichier du paquet `package_id` (ex.
    /// `Author-Name`). Les fichiers sans destination sont ignorés.
    fn place(&self, package_id: &str, kind: &PackageKind, files: &[String]) -> Vec<Placement>;
}

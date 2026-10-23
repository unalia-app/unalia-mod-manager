//! Brique **games** : ce que Modalia sait de chaque jeu moddable.
//!
//! La base vient du schéma d'écosystème de Thunderstore (celui d'r2modman) :
//! pour chaque jeu, ses identifiants Steam, sa communauté Thunderstore, son
//! loader, ses exécutables. C'est de la donnée pure, jamais du code (SPEC §7).
//!
//! Le schéma est mis en cache sur disque : l'appli démarre hors ligne avec la
//! dernière version connue.

mod builtin;
mod schema;

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;

use crate::loaders::redengine::Framework;
use crate::loaders::rules::InstallRules;

pub use schema::SchemaError;

pub const SCHEMA_URL: &str = "https://thunderstore.io/api/experimental/schema/dev/latest/";

/// Loaders que Modalia sait installer, avec les règles de placement du
/// schéma (`loaders::rules`).
pub const SUPPORTED_LOADERS: [&str; 7] = [
    "bepinex",
    "bepisloader",
    "melonloader",
    "recursive-melonloader",
    "shimloader",
    "umm",
    "northstar",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameProfile {
    /// Identifiant du jeu dans le schéma, ex. `hollow-knight-silksong`.
    pub id: String,
    pub name: String,
    /// Communauté Thunderstore où chercher les mods.
    pub community: String,
    pub steam_app_ids: Vec<u32>,
    /// Loader attendu par les mods de ce jeu, ex. `bepinex`, `melonloader`.
    pub loader: String,
    /// `true` si Modalia sait installer ce loader.
    pub supported: bool,
    pub exe_names: Vec<String>,
    /// Dossier de données Unity, ex. `Hollow Knight Silksong_Data`.
    pub data_folder_name: Option<String>,
    /// Jaquette 360×480 fournie par Thunderstore pour les gestionnaires de mods.
    pub cover_url: Option<String>,
    /// Bannière 1920×620.
    pub hero_url: Option<String>,
    /// Jeu dont les mods viennent de Nexus Mods (profils `games/*.toml`).
    pub nexus: Option<NexusGame>,
    /// Où vont les fichiers des mods de ce jeu.
    #[serde(skip)]
    pub install_rules: InstallRules,
    /// Frameworks que Modalia installe tout seul quand un mod en a besoin.
    #[serde(skip)]
    pub frameworks: Vec<Framework>,
    /// Dépôts GitHub d'auteurs que la description de leur mod ne cite pas.
    #[serde(skip)]
    pub mirrors: Vec<Mirror>,
    /// Launchers qui s'ouvrent avant le jeu (noms d'exécutables) : ouverts,
    /// le jeu « démarre » ; ils ne comptent pas comme le jeu lui-même.
    #[serde(skip)]
    pub launchers: Vec<String>,
}

/// Le même mod publié par son auteur sur GitHub (profil du jeu). Seulement
/// un candidat : l'empreinte du fichier est toujours vérifiée auprès de
/// Nexus avant de s'en servir.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mirror {
    pub name: String,
    pub nexus_id: u32,
    pub github: String,
}

/// Le jeu chez Nexus Mods.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct NexusGame {
    /// Ex. `cyberpunk2077`, dans les URL et les liens `nxm://`.
    pub domain: String,
    /// Identifiant numérique (API GraphQL), ex. 3333.
    pub game_id: u32,
}

/// Paquet qui *est* un loader (ex. `silksong_modding-BepInExPack_Silksong`) :
/// il s'installe à la racine du jeu au lieu du dossier des plugins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderPackage {
    pub package_id: String,
    pub loader: String,
    /// Dossier de l'archive dont le contenu va à la racine du jeu.
    pub root_folder: String,
}

#[derive(Debug, Default)]
pub struct GameCatalog {
    games: Vec<GameProfile>,
    by_steam_app_id: HashMap<u32, usize>,
    loader_packages: HashMap<String, LoaderPackage>,
}

impl GameCatalog {
    pub fn from_schema_json(json: &str) -> Result<Self, SchemaError> {
        let (mut games, loaders) = schema::parse(json)?;
        // Profils écrits à la main : ils priment sur le schéma pour le même
        // jeu Steam.
        let builtin = builtin::profiles();
        games.retain(|g| {
            !builtin.iter().any(|b| {
                b.steam_app_ids
                    .iter()
                    .any(|id| g.steam_app_ids.contains(id))
            })
        });
        games.extend(builtin);
        let mut by_steam_app_id = HashMap::new();
        for (i, game) in games.iter().enumerate() {
            for &app_id in &game.steam_app_ids {
                by_steam_app_id.entry(app_id).or_insert(i);
            }
        }
        Ok(Self {
            games,
            by_steam_app_id,
            loader_packages: loaders
                .into_iter()
                .map(|l| (l.package_id.clone(), l))
                .collect(),
        })
    }

    pub fn games(&self) -> &[GameProfile] {
        &self.games
    }

    pub fn by_steam_app_id(&self, app_id: u32) -> Option<&GameProfile> {
        self.by_steam_app_id.get(&app_id).map(|&i| &self.games[i])
    }

    /// Jeu d'une communauté Thunderstore.
    pub fn by_community(&self, community: &str) -> Option<&GameProfile> {
        self.games.iter().find(|g| g.community == community)
    }

    pub fn by_id(&self, id: &str) -> Option<&GameProfile> {
        self.games.iter().find(|g| g.id == id)
    }

    /// Le paquet `namespace-name` est-il un loader ?
    pub fn loader_package(&self, package_id: &str) -> Option<&LoaderPackage> {
        self.loader_packages.get(package_id)
    }

    /// Reconnaît un jeu installé hors de Steam à partir de son dossier : le
    /// dossier de données Unity d'abord (quasi unique), puis les
    /// exécutables. Une correspondance ambiguë ne renvoie rien plutôt que de
    /// deviner.
    pub fn identify_folder(&self, dir: &Path) -> Option<&GameProfile> {
        let names: Vec<String> = fs::read_dir(dir)
            .ok()?
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_lowercase())
            .collect();
        let has = |name: &str| names.iter().any(|n| n == &name.to_lowercase());

        let by_data = self
            .games
            .iter()
            .filter(|g| g.data_folder_name.as_deref().is_some_and(has))
            .collect::<Vec<_>>();
        if !by_data.is_empty() {
            return single(by_data);
        }
        // Un exécutable dans un sous-dossier (`bin/x64/…`) se vérifie sur
        // le disque.
        let exe_found = |exe: &String| {
            if exe.contains('/') {
                dir.join(exe).is_file()
            } else {
                has(exe)
            }
        };
        single(
            self.games
                .iter()
                .filter(|g| g.exe_names.iter().any(exe_found))
                .collect(),
        )
    }
}

impl GameCatalog {
    /// Comme `identify_folder`, en tolérant le dossier parent : on choisit
    /// souvent `D:/Games` au lieu de `D:/Games/Silksong`. Renvoie le profil
    /// et le vrai dossier du jeu.
    pub fn locate_game(&self, dir: &Path) -> Option<(&GameProfile, PathBuf)> {
        if let Some(profile) = self.identify_folder(dir) {
            return Some((profile, dir.to_path_buf()));
        }
        let found: Vec<_> = fs::read_dir(dir)
            .ok()?
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|e| Some((self.identify_folder(&e.path())?, e.path())))
            .collect();
        single(found)
    }
}

/// Identifiant d'un jeu ajouté à la main : son identifiant Steam s'il en a
/// un (images et état partagés avec la version Steam), sinon un identifiant
/// stable dérivé du profil, bit de poids fort à 1 pour ne jamais croiser un
/// identifiant Steam réel.
pub fn manual_app_id(profile: &GameProfile) -> u32 {
    if let Some(&id) = profile.steam_app_ids.first() {
        return id;
    }
    // FNV-1a : stable d'une version de Rust à l'autre, contrairement au
    // hachage de la bibliothèque standard.
    let hash = profile.id.bytes().fold(0x811c_9dc5u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    });
    hash | 0x8000_0000
}

fn single<T>(items: Vec<T>) -> Option<T> {
    let mut items = items.into_iter();
    let first = items.next()?;
    items.next().is_none().then_some(first)
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("couldn't download the Thunderstore game list")]
    Download(#[source] reqwest::Error),
    #[error("the Thunderstore game list is unreadable")]
    Schema(#[from] SchemaError),
    #[error("couldn't write the game list cache")]
    Cache(#[source] io::Error),
}

/// Charge le catalogue : cache s'il a moins de `max_age`, sinon
/// téléchargement (et mise à jour du cache). Hors ligne, un cache périmé
/// vaut mieux que rien.
pub async fn load_catalog(
    client: &reqwest::Client,
    url: &str,
    cache_file: &Path,
    max_age: Duration,
) -> Result<GameCatalog, CatalogError> {
    let cached = fs::read_to_string(cache_file).ok();
    let fresh = fs::metadata(cache_file)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < max_age);

    if fresh && let Some(Ok(catalog)) = cached.as_deref().map(GameCatalog::from_schema_json) {
        return Ok(catalog);
    }

    match download_schema(client, url).await {
        Ok(json) => {
            // On valide avant d'écraser un cache qui fonctionne.
            let catalog = GameCatalog::from_schema_json(&json)?;
            write_atomically(cache_file, &json).map_err(CatalogError::Cache)?;
            Ok(catalog)
        }
        Err(error) => cached
            .as_deref()
            .and_then(|json| GameCatalog::from_schema_json(json).ok())
            .ok_or(CatalogError::Download(error)),
    }
}

async fn download_schema(client: &reqwest::Client, url: &str) -> Result<String, reqwest::Error> {
    client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await
}

fn write_atomically(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, contents)?;
    fs::rename(tmp, path)
}

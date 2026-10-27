//! Fichiers de mods posés **hors d'Unalia** : à la main, par Vortex, par
//! Mod Organizer… Ils bloquent une installation propre (`winhttp.dll`
//! d'un ancien BepInEx, par exemple) et l'utilisateur ne sait pas quoi
//! effacer.
//!
//! On ne devine pas : seuls les emplacements connus des loaders du jeu sont
//! regardés (son « empreinte »), jamais les fichiers du jeu lui-même. Ce qui
//! s'y trouve sans venir d'Unalia part à la corbeille, d'où l'on peut le
//! récupérer, et seulement ce que l'utilisateur a coché.
//!
//! Un nom de DLL commun (`winhttp.dll`, `version.dll`…) peut aussi venir du
//! jeu ou d'un programme dont il a besoin : il n'est `sure` qu'à côté du
//! loader qui l'utilise, sinon sa case reste décochée.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;

use serde::Serialize;

use crate::loaders::unreal::find_project;

/// Emplacement de l'empreinte trouvé dans le jeu, avec ce qui n'y vient
/// pas d'Unalia.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Foreign {
    /// Chemin relatif au jeu, avec des `/`.
    pub path: String,
    pub dir: bool,
    /// Fichiers qui ne viennent pas d'Unalia (0 : dossier vide laissé là).
    pub files: usize,
    pub bytes: u64,
    /// Reste d'un loader à coup sûr : coché d'office dans « Reset mods ».
    pub sure: bool,
}

/// Emplacement de l'empreinte. `proof` : chemins dont un seul suffit à
/// prouver le loader quand le nom ne le prouve pas (vide : le nom suffit).
struct Spot {
    path: String,
    proof: Vec<String>,
}

fn spots(list: &[(&str, &[&str])], prefix: &str) -> Vec<Spot> {
    list.iter()
        .map(|(path, proof)| Spot {
            path: format!("{prefix}{path}"),
            proof: proof.iter().map(|p| format!("{prefix}{p}")).collect(),
        })
        .collect()
}

/// Posés par Doorstop (BepInEx) à côté de son `winhttp.dll`.
const DOORSTOP: &[&str] = &["doorstop_config.ini", ".doorstop_version"];
const CET: &[&str] = &["bin/x64/plugins/cyber_engine_tweaks"];
const UE4SS: &[&str] = &["Binaries/Win64/ue4ss", "Binaries/Win64/UE4SS.dll"];

/// Dossiers de MelonLoader aux noms trop communs pour être retirés sans
/// preuve : seulement vides, ou à côté de `MelonLoader/`.
const MELON_DIRS: [&str; 4] = ["Mods", "Plugins", "UserData", "UserLibs"];

/// Emplacements où les loaders de ce jeu (`loader` du profil) posent leurs
/// fichiers et ceux de leurs mods, relatifs au jeu.
fn footprint(loader: &str, game_dir: &Path) -> Vec<Spot> {
    let mut paths: Vec<Spot> = match loader {
        // BepInEx et MelonLoader : un reste de l'un bloque aussi l'autre.
        "bepinex" | "bepisloader" | "melonloader" | "recursive-melonloader" => spots(
            &[
                ("BepInEx", &[]),
                ("winhttp.dll", DOORSTOP),
                ("doorstop_config.ini", &[]),
                (".doorstop_version", &[]),
                ("doorstop_libs", &[]),
                ("dotnet", DOORSTOP),
                ("MelonLoader", &[]),
                ("version.dll", &["MelonLoader"]),
            ],
            "",
        )
        .into_iter()
        .chain(
            MELON_DIRS
                .iter()
                .filter(|d| game_dir.join("MelonLoader").is_dir() || is_empty_dir(&game_dir.join(d)))
                .map(|d| Spot { path: d.to_string(), proof: Vec::new() }),
        )
        .collect(),
        "redengine" => spots(
            &[
                ("archive/pc/mod", &[]),
                ("bin/x64/plugins", &[]),
                ("bin/x64/version.dll", CET),
                ("bin/x64/winmm.dll", &["red4ext"]),
                ("bin/x64/global.ini", CET),
                ("mods", &[]),
                ("r6/scripts", &[]),
                ("r6/tweaks", &[]),
                ("r6/cache/modded", &[]),
                ("red4ext", &[]),
            ],
            "",
        ),
        "unreal" => match find_project(game_dir) {
            Some(project) => spots(
                &[
                    ("Binaries/Win64/ue4ss", &[]),
                    ("Binaries/Win64/Mods", &[]),
                    ("Binaries/Win64/dwmapi.dll", UE4SS),
                    ("Binaries/Win64/UE4SS.dll", &[]),
                    ("Binaries/Win64/UE4SS-settings.ini", &[]),
                    ("Content/Paks/~mods", &[]),
                    ("Content/Paks/LogicMods", &[]),
                ],
                &format!("{project}/"),
            ),
            None => Vec::new(),
        },
        _ => Vec::new(),
    };
    // Traces de Vortex à la racine : son manifeste de déploiement et sa
    // marque de dossier géré.
    if let Ok(entries) = fs::read_dir(game_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let lower = name.to_ascii_lowercase();
            if (lower.starts_with("vortex.deployment") && lower.ends_with(".json"))
                || lower == "__folder_managed_by_vortex"
            {
                paths.push(Spot { path: name, proof: Vec::new() });
            }
        }
    }
    paths
}

/// Ce que l'empreinte contient sans venir d'Unalia. `ours` : fichiers posés
/// par Unalia ; `our_dirs` : dossiers qu'il a créés (chemins relatifs, `/`).
pub fn scan(
    loader: &str,
    game_dir: &Path,
    ours: &HashSet<String>,
    our_dirs: &HashSet<String>,
) -> Vec<Foreign> {
    let ours: HashSet<String> = ours.iter().map(|p| p.to_ascii_lowercase()).collect();
    let our_dirs: HashSet<String> = our_dirs.iter().map(|p| p.to_ascii_lowercase()).collect();
    footprint(loader, game_dir)
        .into_iter()
        .filter_map(|Spot { path, proof }| {
            let full = game_dir.join(&path);
            let meta = fs::symlink_metadata(&full).ok()?;
            // Une preuve posée par Unalia ne prouve pas l'autre outil.
            let sure = proof.is_empty()
                || proof.iter().any(|p| {
                    let lower = p.to_ascii_lowercase();
                    !ours.contains(&lower)
                        && !our_dirs.contains(&lower)
                        && fs::symlink_metadata(game_dir.join(p)).is_ok()
                });
            if !meta.is_dir() {
                return (!ours.contains(&path.to_ascii_lowercase())).then_some(Foreign {
                    path,
                    dir: false,
                    files: 1,
                    bytes: meta.len(),
                    sure,
                });
            }
            let (mut files, mut bytes, mut any) = (0, 0, false);
            for (relative, size) in files_under(&full, &path) {
                any = true;
                if !ours.contains(&relative.to_ascii_lowercase()) {
                    files += 1;
                    bytes += size;
                }
            }
            // Dossier vide qu'Unalia n'a pas créé : reste d'un autre outil.
            let leftover = !any && !our_dirs.contains(&path.to_ascii_lowercase());
            (files > 0 || leftover).then_some(Foreign {
                path,
                dir: true,
                files,
                bytes,
                sure,
            })
        })
        .collect()
}

/// Retire ces emplacements du jeu avec `discard` (la corbeille,
/// [`to_recycle_bin`], hors tests). Renvoie ceux qui n'ont pas pu partir
/// (fichier ouvert par le jeu, droits…).
pub fn remove(game_dir: &Path, found: &[Foreign], discard: impl Fn(&Path) -> io::Result<()>) -> Vec<String> {
    found
        .iter()
        .filter(|f| {
            let full = game_dir.join(&f.path);
            fs::symlink_metadata(&full).is_ok() && discard(&full).is_err()
        })
        .map(|f| f.path.clone())
        .collect()
}

/// Corbeille du système : l'utilisateur peut y récupérer un fichier.
pub fn to_recycle_bin(path: &Path) -> io::Result<()> {
    trash::delete(path).map_err(io::Error::other)
}

fn is_empty_dir(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_none())
}

/// Fichiers sous `dir` (récursivement), chemin relatif au jeu et taille.
fn files_under(dir: &Path, prefix: &str) -> Vec<(String, u64)> {
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<(String, u64)>) -> io::Result<()> {
        for entry in fs::read_dir(dir)?.flatten() {
            let name = format!("{prefix}/{}", entry.file_name().to_string_lossy());
            let meta = entry.metadata()?;
            // Une jonction se compte comme un fichier : on ne la suit pas.
            if meta.is_dir() && !meta.is_symlink() {
                walk(&entry.path(), &name, out)?;
            } else {
                out.push((name, meta.len()));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    let _ = walk(dir, prefix, &mut out);
    out
}

//! Adaptateur **Bethesda** (Creation Engine : Skyrim Special Edition,
//! Fallout 4), pour les jeux dont les mods viennent de Nexus.
//!
//! Tout mod va dans `Data/`. D'après le contenu de l'archive, sans deviner
//! au hasard :
//! - `Data/…` se pose tel quel ;
//! - un dossier de `Data` (`meshes/`, `textures/`, `SKSE/`…) va sous `Data/` ;
//! - un plugin (`.esp`, `.esm`, `.esl`) ou une archive de ressources
//!   (`.bsa`, `.ba2`) à la racine du paquet va dans `Data/`.
//!
//! Le reste (readme, captures, dossiers d'options) n'est pas installé. Une
//! archive avec un installateur FOMOD (`fomod/ModuleConfig.xml`) propose
//! des choix : elle n'est pas posée à l'aveugle (`has_fomod`).
//!
//! Le jeu ne charge que les plugins listés dans son `plugins.txt` :
//! `sync_plugins` le tient à jour d'après ce que Modalia a posé.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Read};
use std::path::Path;

use super::{Loader, PackageKind, Placement};

/// Dossiers que le jeu lit dans `Data/`.
const DATA_FOLDERS: [&str; 20] = [
    "f4se",
    "grass",
    "interface",
    "lodsettings",
    "materials",
    "mcm",
    "meshes",
    "music",
    "scripts",
    "seq",
    "shadersfx",
    "skse",
    "sound",
    "source",
    "strings",
    "terrain",
    "textures",
    "tools",
    "video",
    "vis",
];

/// Plugins : chargés seulement s'ils sont listés dans `plugins.txt`.
const PLUGINS: [&str; 3] = [".esp", ".esm", ".esl"];

/// Archives de ressources, chargées avec le plugin du même nom.
const ARCHIVES: [&str; 2] = [".bsa", ".ba2"];

pub struct Bethesda;

impl Loader for Bethesda {
    fn id(&self) -> &'static str {
        "bethesda"
    }

    fn installed_manually(&self, _game_dir: &Path) -> bool {
        // Pas de loader unique : le script extender est un framework.
        false
    }

    fn place(&self, _package_id: &str, _kind: &PackageKind, files: &[String]) -> Vec<Placement> {
        if has_fomod(files) {
            return Vec::new();
        }
        let skip = wrapper_depth(files);
        files
            .iter()
            .filter_map(|file| {
                let parts: Vec<&str> = file.split('/').skip(skip).collect();
                Some(Placement {
                    source: file.clone(),
                    target: target(&parts)?,
                })
            })
            .collect()
    }
}

/// L'archive est-elle un installateur FOMOD, avec des choix à faire ? Un
/// dossier `fomod/` qui ne contient que `info.xml` (la fiche du mod) ne
/// compte pas.
pub fn has_fomod(files: &[String]) -> bool {
    files.iter().any(|f| {
        let lower = f.to_ascii_lowercase();
        lower == "fomod/moduleconfig.xml" || lower.ends_with("/fomod/moduleconfig.xml")
    })
}

/// Dossiers d'emballage à ignorer : `Mon Mod v1.2/meshes/…`. On descend
/// tant que tout le paquet est dans un seul dossier qui n'est ni `Data`,
/// ni un de ses dossiers.
fn wrapper_depth(files: &[String]) -> usize {
    let mut depth = 0;
    loop {
        let mut first = None;
        for file in files {
            let parts: Vec<&str> = file.split('/').skip(depth).collect();
            if parts.len() < 2 {
                return depth;
            }
            match first {
                None => first = Some(parts[0]),
                Some(f) if f != parts[0] => return depth,
                Some(_) => {}
            }
        }
        match first {
            Some(f) if !is_data(f) && !is_data_folder(f) => depth += 1,
            _ => return depth,
        }
    }
}

fn is_data(segment: &str) -> bool {
    segment.eq_ignore_ascii_case("data")
}

fn is_data_folder(segment: &str) -> bool {
    DATA_FOLDERS.iter().any(|d| d.eq_ignore_ascii_case(segment))
}

fn has_ext(name: &str, exts: &[&str]) -> bool {
    let lower = name.to_ascii_lowercase();
    exts.iter().any(|e| lower.ends_with(e))
}

fn target(parts: &[&str]) -> Option<String> {
    match parts {
        [data, rest @ ..] if is_data(data) && !rest.is_empty() => {
            Some(format!("Data/{}", rest.join("/")))
        }
        [folder, rest @ ..] if is_data_folder(folder) && !rest.is_empty() => {
            Some(format!("Data/{}", parts.join("/")))
        }
        [name] if has_ext(name, &PLUGINS) || has_ext(name, &ARCHIVES) => {
            Some(format!("Data/{name}"))
        }
        _ => None,
    }
}

/// Plugin posé dans le jeu : son nom, s'il est directement dans `Data/`.
fn plugin_name(target: &str) -> Option<&str> {
    let (dir, name) = target.split_once('/')?;
    (is_data(dir) && !name.contains('/') && has_ext(name, &PLUGINS)).then_some(name)
}

/// Une ligne de `plugins.txt` : `*Mod.esp` (actif) ou `Mod.esp`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    name: String,
    active: bool,
}

/// Met `plugins.txt` (`list`) d'accord avec les plugins du jeu. Les
/// plugins que Modalia a posés (`deployed`, chemins relatifs au jeu) sont
/// ajoutés, actifs ; les lignes dont le fichier n'est plus dans `Data/`
/// partent (le jeu les ignorait déjà). Le reste, posé à la main ou par un
/// autre outil, garde sa ligne et son état. Chaque plugin passe après ses
/// masters, sans bouger sinon. Renvoie `true` si le fichier a changé.
pub fn sync_plugins(game_dir: &Path, list: &Path, deployed: &[String]) -> io::Result<bool> {
    let data = game_dir.join("Data");
    let present: HashSet<String> = match fs::read_dir(&data) {
        Ok(entries) => entries
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_ascii_lowercase())
            .collect(),
        // Jeu déplacé ou disque débranché : on ne touche à rien.
        Err(_) => return Ok(false),
    };

    let before = match fs::read(list) {
        Ok(bytes) => latin1(&bytes),
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let mut header = Vec::new();
    let mut entries: Vec<Entry> = Vec::new();
    for line in before.lines().map(|l| l.trim_end_matches('\r')) {
        if line.starts_with('#') {
            header.push(line.to_owned());
            continue;
        }
        let (name, active) = match line.strip_prefix('*') {
            Some(name) => (name.trim(), true),
            None => (line.trim(), false),
        };
        if name.is_empty() || !present.contains(&name.to_ascii_lowercase()) {
            continue;
        }
        if entries.iter().any(|e| e.name.eq_ignore_ascii_case(name)) {
            continue;
        }
        entries.push(Entry {
            name: name.to_owned(),
            active,
        });
    }
    for name in deployed.iter().filter_map(|t| plugin_name(t)) {
        let listed = entries.iter().any(|e| e.name.eq_ignore_ascii_case(name));
        // Nom hors de l'encodage du fichier : impossible à écrire.
        if !listed
            && present.contains(&name.to_ascii_lowercase())
            && name.chars().all(|c| u32::from(c) <= 0xFF)
        {
            entries.push(Entry {
                name: name.to_owned(),
                active: true,
            });
        }
    }

    let entries = masters_first(entries, |name| masters(&data.join(name)));
    let mut after = String::new();
    for line in &header {
        after.push_str(line);
        after.push_str("\r\n");
    }
    for entry in &entries {
        if entry.active {
            after.push('*');
        }
        after.push_str(&entry.name);
        after.push_str("\r\n");
    }
    if after == before || (before.is_empty() && entries.is_empty()) {
        return Ok(false);
    }
    if let Some(dir) = list.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = list.with_extension("txt.tmp");
    fs::write(&tmp, after.chars().map(|c| c as u8).collect::<Vec<u8>>())?;
    fs::rename(&tmp, list)?;
    Ok(true)
}

/// `plugins.txt` est en Windows-1252 : lu octet par octet, il se réécrit à
/// l'identique.
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

/// Ordre de chargement où chaque plugin suit ses masters listés. Un master
/// placé trop bas remonte juste avant le premier plugin qui en a besoin ;
/// rien d'autre ne bouge.
fn masters_first(entries: Vec<Entry>, masters_of: impl Fn(&str) -> Vec<String>) -> Vec<Entry> {
    let index: HashMap<String, usize> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| (e.name.to_ascii_lowercase(), i))
        .collect();
    let mut seen = vec![false; entries.len()];
    let mut order = Vec::with_capacity(entries.len());
    fn visit(
        i: usize,
        entries: &[Entry],
        index: &HashMap<String, usize>,
        masters_of: &dyn Fn(&str) -> Vec<String>,
        seen: &mut [bool],
        order: &mut Vec<usize>,
    ) {
        if seen[i] {
            return;
        }
        seen[i] = true;
        for master in masters_of(&entries[i].name) {
            if let Some(&m) = index.get(&master.to_ascii_lowercase()) {
                visit(m, entries, index, masters_of, seen, order);
            }
        }
        order.push(i);
    }
    for i in 0..entries.len() {
        visit(i, &entries, &index, &masters_of, &mut seen, &mut order);
    }
    order.into_iter().map(|i| entries[i].clone()).collect()
}

/// Taille maximale de l'en-tête lu : bien au-delà d'une vraie liste de
/// masters, une protection contre un fichier abîmé.
const MAX_HEADER: u32 = 1 << 20;

/// Masters d'un plugin (sous-enregistrements `MAST` de son en-tête `TES4`).
/// Illisible : aucun, l'ordre reste celui du fichier.
fn masters(plugin: &Path) -> Vec<String> {
    read_masters(plugin).unwrap_or_default()
}

fn read_masters(plugin: &Path) -> io::Result<Vec<String>> {
    let mut file = fs::File::open(plugin)?;
    // En-tête d'enregistrement de Skyrim SE et Fallout 4 : 24 octets.
    let mut head = [0u8; 24];
    file.read_exact(&mut head)?;
    if &head[..4] != b"TES4" {
        return Ok(Vec::new());
    }
    let size = u32::from_le_bytes([head[4], head[5], head[6], head[7]]).min(MAX_HEADER);
    let mut body = vec![0u8; size as usize];
    file.read_exact(&mut body)?;

    let mut found = Vec::new();
    let mut at = 0;
    while at + 6 <= body.len() {
        let kind = &body[at..at + 4];
        let len = usize::from(u16::from_le_bytes([body[at + 4], body[at + 5]]));
        let start = at + 6;
        let Some(data) = body.get(start..start + len) else {
            break;
        };
        if kind == b"MAST" {
            let name = data.split(|&b| b == 0).next().unwrap_or_default();
            found.push(latin1(name));
        }
        at = start + len;
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(files: &[&str]) -> Vec<(String, String)> {
        let files: Vec<String> = files.iter().map(|f| f.to_string()).collect();
        let mut out: Vec<_> = Bethesda
            .place("Cool Mod", &PackageKind::Mod, &files)
            .into_iter()
            .map(|p| (p.source, p.target))
            .collect();
        out.sort();
        out
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        let mut v: Vec<_> = list
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        v.sort();
        v
    }

    #[test]
    fn data_layouts_go_under_data() {
        assert_eq!(
            place(&[
                "Cool.esp",
                "Cool - Textures.bsa",
                "meshes/cool/sword.nif",
                "SKSE/Plugins/Cool.dll",
                "readme.txt",
            ]),
            pairs(&[
                ("Cool.esp", "Data/Cool.esp"),
                ("Cool - Textures.bsa", "Data/Cool - Textures.bsa"),
                ("meshes/cool/sword.nif", "Data/meshes/cool/sword.nif"),
                ("SKSE/Plugins/Cool.dll", "Data/SKSE/Plugins/Cool.dll"),
            ])
        );
    }

    #[test]
    fn data_folder_and_wrappers_are_unwrapped() {
        assert_eq!(
            place(&[
                "Cool Mod 1.2/Data/Cool.esp",
                "Cool Mod 1.2/Data/textures/a.dds"
            ]),
            pairs(&[
                ("Cool Mod 1.2/Data/Cool.esp", "Data/Cool.esp"),
                ("Cool Mod 1.2/Data/textures/a.dds", "Data/textures/a.dds"),
            ])
        );
        assert_eq!(
            place(&["Cool Mod/Cool.esp", "Cool Mod/scripts/cool.pex"]),
            pairs(&[
                ("Cool Mod/Cool.esp", "Data/Cool.esp"),
                ("Cool Mod/scripts/cool.pex", "Data/scripts/cool.pex"),
            ])
        );
    }

    #[test]
    fn fomod_installers_are_not_placed_blindly() {
        assert!(
            place(&[
                "fomod/ModuleConfig.xml",
                "00 Core/Cool.esp",
                "01 Option/Cool.esp"
            ])
            .is_empty()
        );
        assert!(has_fomod(&["Cool/fomod/ModuleConfig.xml".into()]));
        // La fiche seule ne propose aucun choix.
        assert_eq!(
            place(&["fomod/info.xml", "Cool.esp"]),
            pairs(&[("Cool.esp", "Data/Cool.esp")])
        );
    }

    #[test]
    fn masters_load_before_their_plugins() {
        let entry = |name: &str| Entry {
            name: name.into(),
            active: true,
        };
        let masters_of = |name: &str| match name {
            "Patch.esp" => vec!["Skyrim.esm".to_owned(), "Lib.esm".to_owned()],
            _ => Vec::new(),
        };
        let sorted = masters_first(
            vec![
                entry("A.esp"),
                entry("Patch.esp"),
                entry("B.esp"),
                entry("Lib.esm"),
            ],
            masters_of,
        );
        let names: Vec<&str> = sorted.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["A.esp", "Lib.esm", "Patch.esp", "B.esp"]);
    }
}

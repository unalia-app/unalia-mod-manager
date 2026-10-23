//! Jeux hors Steam ajoutés à Steam (« Ajouter un jeu non-Steam »), lus dans
//! `userdata/<compte>/config/shortcuts.vdf`.
//!
//! Lancer un tel jeu par Steam lui donne l'overlay de Steam (compteur FPS,
//! captures, chat). Modalia lit ce fichier, et sait y ajouter un jeu à la
//! demande (`add_shortcut`), Steam fermé, sans toucher aux autres.
//!
//! Le fichier est un VDF binaire : un octet de type, une clé terminée par un
//! zéro, puis la valeur (`0x00` objet, `0x01` texte, `0x02` entier 32 bits,
//! `0x08` fin d'objet).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Raccourci d'un jeu hors Steam.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    /// Identifiant que Steam donne au raccourci.
    pub app_id: u32,
    pub name: String,
    pub exe: PathBuf,
    pub start_dir: PathBuf,
}

impl Shortcut {
    /// Adresse qui lance le raccourci par Steam.
    pub fn launch_url(&self) -> String {
        let game_id = (u64::from(self.app_id) << 32) | 0x0200_0000;
        format!("steam://rungameid/{game_id}")
    }

    /// Le raccourci lance-t-il un exécutable de ce dossier ?
    pub fn points_into(&self, game_dir: &Path) -> bool {
        let dir = normalize(game_dir);
        [&self.exe, &self.start_dir].iter().any(|path| {
            let path = normalize(path);
            path == dir
                || path
                    .strip_prefix(&dir)
                    .is_some_and(|rest| rest.starts_with('/'))
        })
    }
}

/// Raccourcis de tous les comptes Steam de la machine.
pub fn steam_shortcuts(steam_root: &Path) -> Vec<Shortcut> {
    let Ok(users) = fs::read_dir(steam_root.join("userdata")) else {
        return Vec::new();
    };
    users
        .flatten()
        .filter_map(|user| fs::read(user.path().join("config").join("shortcuts.vdf")).ok())
        .flat_map(|bytes| parse(&bytes))
        .collect()
}

/// Raccourci qui pointe dans le dossier du jeu, s'il y en a un.
pub fn shortcut_for<'a>(shortcuts: &'a [Shortcut], game_dir: &Path) -> Option<&'a Shortcut> {
    shortcuts.iter().find(|s| s.points_into(game_dir))
}

/// Lit un `shortcuts.vdf`. Un fichier abîmé donne ce qui a pu être lu.
pub fn parse(bytes: &[u8]) -> Vec<Shortcut> {
    let mut reader = Reader { bytes, at: 0 };
    let root = reader.object_partial();
    root.iter()
        .filter_map(|(_, node)| match node {
            Node::Map(entries) => Some(entries),
            _ => None,
        })
        .flatten()
        .filter_map(|(_, node)| match node {
            Node::Map(fields) => shortcut(fields),
            _ => None,
        })
        .collect()
}

/// Un nœud du VDF binaire, gardé tel quel : réécrire le fichier ne doit rien
/// perdre des raccourcis existants (étiquettes, options de lancement…).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    Map(Vec<(String, Node)>),
    Text(String),
    Int(u32),
    /// Autres types (flottant, 64 bits…) : octet de type et valeur brute.
    Raw(u8, Vec<u8>),
}

fn shortcut(fields: &[(String, Node)]) -> Option<Shortcut> {
    let text = |key: &str| {
        fields.iter().find_map(|(k, v)| match v {
            Node::Text(t) if k.eq_ignore_ascii_case(key) => Some(t.clone()),
            _ => None,
        })
    };
    let exe = text("exe")?;
    let name = text("appname").unwrap_or_default();
    // Les raccourcis récents portent leur identifiant ; les anciens se
    // calculent comme le fait Steam, à partir de l'exécutable et du nom.
    let app_id = fields
        .iter()
        .find_map(|(k, v)| match v {
            Node::Int(id) if k.eq_ignore_ascii_case("appid") => Some(*id),
            _ => None,
        })
        .unwrap_or_else(|| legacy_app_id(&exe, &name));
    Some(Shortcut {
        app_id,
        name,
        exe: PathBuf::from(unquote(&exe)),
        start_dir: PathBuf::from(unquote(&text("startdir").unwrap_or_default())),
    })
}

fn legacy_app_id(exe: &str, name: &str) -> u32 {
    crc32(format!("{exe}{name}").as_bytes()) | 0x8000_0000
}

fn unquote(path: &str) -> &str {
    path.trim().trim_matches('"')
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Option<u8> {
        let b = *self.bytes.get(self.at)?;
        self.at += 1;
        Some(b)
    }

    fn string(&mut self) -> Option<String> {
        let len = self.bytes.get(self.at..)?.iter().position(|&b| b == 0)?;
        let text = String::from_utf8_lossy(&self.bytes[self.at..self.at + len]).into_owned();
        self.at += len + 1;
        Some(text)
    }

    fn take(&mut self, n: usize) -> Option<Vec<u8>> {
        let bytes = self.bytes.get(self.at..self.at + n)?.to_vec();
        self.at += n;
        Some(bytes)
    }

    fn node(&mut self, kind: u8) -> Option<Node> {
        Some(match kind {
            0x00 => Node::Map(self.object()?),
            0x01 => Node::Text(self.string()?),
            0x02 => Node::Int(u32::from_le_bytes(self.take(4)?.try_into().ok()?)),
            // 0x03 flottant, 0x04 pointeur, 0x06 couleur : 4 octets.
            0x03 | 0x04 | 0x06 => Node::Raw(kind, self.take(4)?),
            // 0x07 entier 64 bits.
            0x07 => Node::Raw(kind, self.take(8)?),
            _ => return None,
        })
    }

    /// Champs d'un objet jusqu'à sa fin.
    fn object(&mut self) -> Option<Vec<(String, Node)>> {
        let mut fields = Vec::new();
        loop {
            let kind = self.byte()?;
            if kind == 0x08 {
                return Some(fields);
            }
            let key = self.string()?;
            fields.push((key, self.node(kind)?));
        }
    }

    /// Comme `object`, mais garde ce qui a pu être lu d'un fichier abîmé.
    fn object_partial(&mut self) -> Vec<(String, Node)> {
        let mut fields = Vec::new();
        while let Some(kind) = self.byte() {
            if kind == 0x08 {
                break;
            }
            let Some(key) = self.string() else { break };
            match kind {
                0x00 => {
                    let start = self.at;
                    match self.object() {
                        Some(inner) => fields.push((key, Node::Map(inner))),
                        None => {
                            // Objet tronqué : ses entrées complètes comptent.
                            self.at = start;
                            fields.push((key, Node::Map(self.object_partial())));
                            break;
                        }
                    }
                }
                _ => match self.node(kind) {
                    Some(node) => fields.push((key, node)),
                    None => break,
                },
            }
        }
        fields
    }
}

fn write_fields(out: &mut Vec<u8>, fields: &[(String, Node)]) {
    for (key, node) in fields {
        let kind = match node {
            Node::Map(_) => 0x00,
            Node::Text(_) => 0x01,
            Node::Int(_) => 0x02,
            Node::Raw(kind, _) => *kind,
        };
        out.push(kind);
        out.extend(key.as_bytes());
        out.push(0);
        match node {
            Node::Map(inner) => {
                write_fields(out, inner);
                out.push(0x08);
            }
            Node::Text(text) => {
                out.extend(text.as_bytes());
                out.push(0);
            }
            Node::Int(n) => out.extend(n.to_le_bytes()),
            Node::Raw(_, bytes) => out.extend(bytes),
        }
    }
}

/// Raccourci à créer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewShortcut {
    pub name: String,
    pub exe: PathBuf,
    pub start_dir: PathBuf,
}

/// Ajoute un jeu non-Steam au fichier `shortcuts.vdf` du compte (créé s'il
/// n'existe pas), comme le fait « Ajouter un jeu non-Steam » de Steam.
/// Steam doit être fermé : il garde ce fichier en mémoire et le réécrit en
/// quittant. Les raccourcis existants sont gardés tels quels, et l'ancien
/// fichier est sauvegardé à côté (`.unalia-backup`). Déjà présent pour cet
/// exécutable : rien n'est écrit.
pub fn add_shortcut(file: &Path, new: &NewShortcut) -> io::Result<Shortcut> {
    let existing = fs::read(file).ok();
    let mut root = existing
        .as_deref()
        .map(|bytes| Reader { bytes, at: 0 }.object_partial())
        .unwrap_or_default();
    if let Some(found) = parse(existing.as_deref().unwrap_or_default())
        .into_iter()
        .find(|s| normalize(&s.exe) == normalize(&new.exe))
    {
        return Ok(found);
    }

    let quoted = |p: &Path| format!("\"{}\"", p.display());
    let exe = quoted(&new.exe);
    let app_id = legacy_app_id(&exe, &new.name);
    // Les champs que Steam écrit lui-même pour un jeu non-Steam.
    let entry = Node::Map(vec![
        ("appid".into(), Node::Int(app_id)),
        ("AppName".into(), Node::Text(new.name.clone())),
        ("Exe".into(), Node::Text(exe)),
        ("StartDir".into(), Node::Text(quoted(&new.start_dir))),
        ("icon".into(), Node::Text(new.exe.display().to_string())),
        ("ShortcutPath".into(), Node::Text(String::new())),
        ("LaunchOptions".into(), Node::Text(String::new())),
        ("IsHidden".into(), Node::Int(0)),
        ("AllowDesktopConfig".into(), Node::Int(1)),
        ("AllowOverlay".into(), Node::Int(1)),
        ("OpenVR".into(), Node::Int(0)),
        ("Devkit".into(), Node::Int(0)),
        ("DevkitGameID".into(), Node::Text(String::new())),
        ("DevkitOverrideAppID".into(), Node::Int(0)),
        ("LastPlayTime".into(), Node::Int(0)),
        ("FlatpakAppID".into(), Node::Text(String::new())),
        ("tags".into(), Node::Map(Vec::new())),
    ]);
    match root
        .iter_mut()
        .find(|(k, _)| k.eq_ignore_ascii_case("shortcuts"))
    {
        Some((_, Node::Map(entries))) => {
            // Clés « 0 », « 1 »… : la suivante.
            let next = entries
                .iter()
                .filter_map(|(k, _)| k.parse::<u32>().ok())
                .max()
                .map_or(0, |n| n + 1);
            entries.push((next.to_string(), entry));
        }
        _ => root = vec![("shortcuts".into(), Node::Map(vec![("0".into(), entry)]))],
    }

    let mut out = Vec::new();
    write_fields(&mut out, &root);
    out.push(0x08);
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    if existing.is_some() {
        fs::copy(file, file.with_extension("vdf.unalia-backup"))?;
    }
    let tmp = file.with_extension("vdf.tmp");
    fs::write(&tmp, &out)?;
    fs::rename(&tmp, file)?;
    Ok(Shortcut {
        app_id,
        name: new.name.clone(),
        exe: new.exe.clone(),
        start_dir: new.start_dir.clone(),
    })
}

/// Dossier `config` du compte Steam à utiliser, de la source la plus sûre à
/// la moins sûre :
/// 1. sous Windows, le compte connecté que Steam ouvert écrit dans le
///    registre (`ActiveProcess\ActiveUser`) ;
/// 2. `loginusers.vdf` : le compte marqué le plus récent, sinon celui de la
///    dernière connexion ;
/// 3. le compte dont Steam a modifié les réglages le plus récemment.
pub fn current_user_config(steam_root: &Path) -> Option<PathBuf> {
    let userdata = steam_root.join("userdata");
    let config = |account: u64| userdata.join(account.to_string()).join("config");
    let exists = |account: &u64| *account != 0 && userdata.join(account.to_string()).is_dir();

    if let Some(account) = active_user().filter(exists) {
        return Some(config(account));
    }
    if let Some(account) = login_users(steam_root).filter(exists) {
        return Some(config(account));
    }
    fs::read_dir(&userdata)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let account: u64 = e.file_name().to_str()?.parse().ok()?;
            let touched = fs::metadata(e.path().join("config").join("localconfig.vdf"))
                .and_then(|m| m.modified())
                .ok()?;
            exists(&account).then_some((touched, account))
        })
        .max()
        .map(|(_, account)| config(account))
}

/// Compte connecté à Steam en ce moment (Windows seulement).
#[cfg(windows)]
fn active_user() -> Option<u64> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Valve\Steam\ActiveProcess")
        .ok()?;
    let id: u32 = key.get_value("ActiveUser").ok()?;
    (id != 0).then_some(u64::from(id))
}

#[cfg(not(windows))]
fn active_user() -> Option<u64> {
    None
}

/// Compte le plus récent de `loginusers.vdf`, en identifiant de compte (le
/// nom du dossier de `userdata`).
fn login_users(steam_root: &Path) -> Option<u64> {
    const STEAM_ID_BASE: u64 = 76_561_197_960_265_728;
    let text = fs::read_to_string(steam_root.join("config").join("loginusers.vdf")).ok()?;
    let doc = super::vdf::parse(&text).ok()?;
    let users: Vec<(u64, bool, u64)> = doc
        .obj("users")?
        .iter()
        .filter_map(|(steam_id, value)| {
            let super::vdf::Value::Obj(user) = value else {
                return None;
            };
            let account = steam_id.parse::<u64>().ok()?.checked_sub(STEAM_ID_BASE)?;
            let recent = user.str("MostRecent") == Some("1");
            let at = user
                .str("Timestamp")
                .and_then(|t| t.parse().ok())
                .unwrap_or(0);
            Some((account, recent, at))
        })
        .collect();
    users
        .iter()
        .find(|(_, recent, _)| *recent)
        .or_else(|| users.iter().max_by_key(|(_, _, at)| *at))
        .map(|(account, _, _)| *account)
}

/// Image de la bibliothèque Steam d'un raccourci : `kind` vaut `p`
/// (jaquette), `_hero` (bannière), `_logo` ou `` (en-tête).
pub fn grid_image(config: &Path, app_id: u32, kind: &str, extension: &str) -> PathBuf {
    config
        .join("grid")
        .join(format!("{app_id}{kind}.{extension}"))
}

/// CRC-32 (polynôme de zlib), comme Steam pour les anciens raccourcis.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Comparaison de chemins tolérante : séparateurs unifiés, casse ignorée
/// sous Windows.
fn normalize(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let s = s.trim_end_matches('/');
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Construit un `shortcuts.vdf` comme Steam l'écrit.
    fn vdf(entries: &[&[(&str, Field)]]) -> Vec<u8> {
        let mut out = vec![0x00];
        out.extend(b"shortcuts\0");
        for (i, fields) in entries.iter().enumerate() {
            out.push(0x00);
            out.extend(format!("{i}\0").as_bytes());
            for (key, value) in fields.iter() {
                match value {
                    Field::Text(t) => {
                        out.push(0x01);
                        out.extend(format!("{key}\0{t}\0").as_bytes());
                    }
                    Field::Int(n) => {
                        out.push(0x02);
                        out.extend(format!("{key}\0").as_bytes());
                        out.extend(n.to_le_bytes());
                    }
                    Field::Tags => {
                        out.push(0x00);
                        out.extend(format!("{key}\0").as_bytes());
                        out.push(0x01);
                        out.extend(b"0\0favorite\0");
                        out.push(0x08);
                    }
                }
            }
            out.push(0x08);
        }
        out.extend([0x08, 0x08]);
        out
    }

    enum Field {
        Text(&'static str),
        Int(u32),
        Tags,
    }

    #[test]
    fn reads_shortcuts_as_steam_writes_them() {
        let bytes = vdf(&[
            &[
                ("appid", Field::Int(0xC234_5678)),
                ("AppName", Field::Text("Cyberpunk 2077 (GOG)")),
                (
                    "Exe",
                    Field::Text("\"D:\\GOG Games\\Cyberpunk 2077\\bin\\x64\\Cyberpunk2077.exe\""),
                ),
                (
                    "StartDir",
                    Field::Text("\"D:\\GOG Games\\Cyberpunk 2077\\bin\\x64\\\""),
                ),
                ("IsHidden", Field::Int(0)),
                ("tags", Field::Tags),
            ],
            &[
                ("appname", Field::Text("Old shortcut")),
                ("exe", Field::Text("\"C:\\Games\\Old\\old.exe\"")),
            ],
        ]);
        let shortcuts = parse(&bytes);
        assert_eq!(shortcuts.len(), 2);
        assert_eq!(shortcuts[0].app_id, 0xC234_5678);
        assert_eq!(shortcuts[0].name, "Cyberpunk 2077 (GOG)");
        assert_eq!(
            shortcuts[0].launch_url(),
            format!("steam://rungameid/{}", (0xC234_5678u64 << 32) | 0x0200_0000)
        );
        // Sans identifiant : calculé comme Steam.
        assert_eq!(
            shortcuts[1].app_id,
            crc32(b"\"C:\\Games\\Old\\old.exe\"Old shortcut") | 0x8000_0000
        );
    }

    #[test]
    fn finds_the_shortcut_of_a_game_folder() {
        let shortcuts = parse(&vdf(&[&[
            ("appid", Field::Int(42)),
            (
                "Exe",
                Field::Text("\"/games/Cyberpunk 2077/bin/x64/Cyberpunk2077.exe\""),
            ),
            (
                "StartDir",
                Field::Text("\"/games/Cyberpunk 2077/bin/x64/\""),
            ),
        ]]));
        assert!(shortcut_for(&shortcuts, Path::new("/games/Cyberpunk 2077")).is_some());
        assert!(shortcut_for(&shortcuts, Path::new("/games/Cyberpunk")).is_none());
        assert!(shortcut_for(&shortcuts, Path::new("/games/Other")).is_none());
    }

    #[test]
    fn adding_a_shortcut_keeps_the_others_intact() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config/shortcuts.vdf");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let before = vdf(&[&[
            ("appid", Field::Int(7)),
            ("AppName", Field::Text("Old game")),
            ("Exe", Field::Text("\"C:\\Old\\old.exe\"")),
            ("LastPlayTime", Field::Int(123)),
            ("tags", Field::Tags),
        ]]);
        std::fs::write(&file, &before).unwrap();

        let new = NewShortcut {
            name: "Cyberpunk 2077".into(),
            exe: PathBuf::from("D:/GOG/Cyberpunk 2077/bin/x64/Cyberpunk2077.exe"),
            start_dir: PathBuf::from("D:/GOG/Cyberpunk 2077/bin/x64"),
        };
        let added = add_shortcut(&file, &new).unwrap();
        assert!(added.app_id & 0x8000_0000 != 0);

        let bytes = std::fs::read(&file).unwrap();
        let all = parse(&bytes);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].name, "Old game");
        assert_eq!(all[1].name, "Cyberpunk 2077");
        assert_eq!(all[1].app_id, added.app_id);
        // L'ancien raccourci est réécrit à l'identique, étiquettes comprises.
        assert!(
            bytes
                .windows(before.len() - 2)
                .any(|w| w == &before[..before.len() - 2])
        );
        assert_eq!(
            std::fs::read(file.with_extension("vdf.unalia-backup")).unwrap(),
            before
        );

        // Une seconde fois : rien de plus.
        assert_eq!(add_shortcut(&file, &new).unwrap(), added);
        assert_eq!(parse(&std::fs::read(&file).unwrap()).len(), 2);
    }

    #[test]
    fn a_first_shortcut_creates_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config/shortcuts.vdf");
        let new = NewShortcut {
            name: "Game".into(),
            exe: PathBuf::from("/g/game.exe"),
            start_dir: PathBuf::from("/g"),
        };
        add_shortcut(&file, &new).unwrap();
        let all = parse(&std::fs::read(&file).unwrap());
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].exe, PathBuf::from("/g/game.exe"));
    }

    #[test]
    fn finds_the_steam_account_to_use() {
        let root = tempfile::tempdir().unwrap();
        let base = 76_561_197_960_265_728u64;
        std::fs::create_dir_all(root.path().join("config")).unwrap();
        for account in ["0", "111", "222"] {
            std::fs::create_dir_all(root.path().join("userdata").join(account).join("config"))
                .unwrap();
        }
        // Rien d'autre : le compte dont les réglages ont bougé en dernier.
        std::fs::write(
            root.path().join("userdata/111/config/localconfig.vdf"),
            "old",
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(
            root.path().join("userdata/222/config/localconfig.vdf"),
            "new",
        )
        .unwrap();
        assert_eq!(
            current_user_config(root.path()),
            Some(root.path().join("userdata/222/config"))
        );

        // Sans « MostRecent » : la dernière connexion.
        let users = |a: &str, b: &str| {
            format!(
                "\"users\"\n{{\n\"{}\"\n{{\n{a}\n}}\n\"{}\"\n{{\n{b}\n}}\n}}\n",
                base + 111,
                base + 222
            )
        };
        let login = root.path().join("config/loginusers.vdf");
        std::fs::write(
            &login,
            users("\"Timestamp\" \"900\"", "\"Timestamp\" \"100\""),
        )
        .unwrap();
        assert_eq!(
            current_user_config(root.path()),
            Some(root.path().join("userdata/111/config"))
        );
        // « mostrecent » en minuscules, comme l'écrivent certaines versions.
        std::fs::write(
            &login,
            users("\"Timestamp\" \"900\"", "\"mostrecent\" \"1\""),
        )
        .unwrap();
        assert_eq!(
            current_user_config(root.path()),
            Some(root.path().join("userdata/222/config"))
        );
    }

    #[test]
    fn damaged_files_give_what_could_be_read() {
        let mut bytes = vdf(&[&[("appid", Field::Int(1)), ("Exe", Field::Text("a.exe"))]]);
        bytes.truncate(bytes.len() - 6);
        assert!(parse(&bytes).len() <= 1);
        assert!(parse(b"garbage").is_empty());
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }
}

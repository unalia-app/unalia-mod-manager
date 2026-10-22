//! Jeux hors Steam ajoutés à Steam (« Ajouter un jeu non-Steam »), lus dans
//! `userdata/<compte>/config/shortcuts.vdf`.
//!
//! Lancer un tel jeu par Steam lui donne l'overlay de Steam (compteur FPS,
//! captures, chat). Modalia ne fait que lire ce fichier : il ne crée ni ne
//! modifie jamais de raccourci.
//!
//! Le fichier est un VDF binaire : un octet de type, une clé terminée par un
//! zéro, puis la valeur (`0x00` objet, `0x01` texte, `0x02` entier 32 bits,
//! `0x08` fin d'objet).

use std::fs;
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
    let mut shortcuts = Vec::new();
    // Racine : un objet « shortcuts » dont chaque entrée est un raccourci.
    if reader.byte() != Some(0x00) || reader.string().is_none() {
        return shortcuts;
    }
    while let Some(kind) = reader.byte() {
        if kind != 0x00 {
            break;
        }
        if reader.string().is_none() {
            break;
        }
        let Some(fields) = reader.object() else {
            break;
        };
        if let Some(shortcut) = shortcut(&fields) {
            shortcuts.push(shortcut);
        }
    }
    shortcuts
}

enum Value {
    Text(String),
    Int(u32),
    Other,
}

fn shortcut(fields: &[(String, Value)]) -> Option<Shortcut> {
    let text = |key: &str| {
        fields.iter().find_map(|(k, v)| match v {
            Value::Text(t) if k.eq_ignore_ascii_case(key) => Some(t.clone()),
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
            Value::Int(id) if k.eq_ignore_ascii_case("appid") => Some(*id),
            _ => None,
        })
        .unwrap_or_else(|| crc32(format!("{exe}{name}").as_bytes()) | 0x8000_0000);
    Some(Shortcut {
        app_id,
        name,
        exe: PathBuf::from(unquote(&exe)),
        start_dir: PathBuf::from(unquote(&text("startdir").unwrap_or_default())),
    })
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
        let len = self.bytes[self.at..].iter().position(|&b| b == 0)?;
        let text = String::from_utf8_lossy(&self.bytes[self.at..self.at + len]).into_owned();
        self.at += len + 1;
        Some(text)
    }

    fn int(&mut self) -> Option<u32> {
        let bytes = self.bytes.get(self.at..self.at + 4)?;
        self.at += 4;
        Some(u32::from_le_bytes(bytes.try_into().ok()?))
    }

    /// Champs d'un objet jusqu'à sa fin ; les objets imbriqués (tags) sont
    /// lus pour être sautés.
    fn object(&mut self) -> Option<Vec<(String, Value)>> {
        let mut fields = Vec::new();
        loop {
            let kind = self.byte()?;
            if kind == 0x08 {
                return Some(fields);
            }
            let key = self.string()?;
            let value = match kind {
                0x00 => {
                    self.object()?;
                    Value::Other
                }
                0x01 => Value::Text(self.string()?),
                0x02 => Value::Int(self.int()?),
                // 0x03 flottant, 0x04 pointeur, 0x06 couleur : 4 octets.
                0x03 | 0x04 | 0x06 => {
                    self.int()?;
                    Value::Other
                }
                // 0x07 entier 64 bits.
                0x07 => {
                    self.int()?;
                    self.int()?;
                    Value::Other
                }
                _ => return None,
            };
            fields.push((key, value));
        }
    }
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
    fn damaged_files_give_what_could_be_read() {
        let mut bytes = vdf(&[&[("appid", Field::Int(1)), ("Exe", Field::Text("a.exe"))]]);
        bytes.truncate(bytes.len() - 6);
        assert!(parse(&bytes).len() <= 1);
        assert!(parse(b"garbage").is_empty());
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }
}

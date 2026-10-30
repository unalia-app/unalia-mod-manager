//! **ini** block: merge a mod's settings into a game's settings file.
//!
//! Unreal games read their settings (`Engine.ini`…) from the player's
//! folder, a file the game and the player also write. A mod's `Engine.ini`
//! can't replace it: its keys are merged in, each change recorded with the
//! value it replaced, so that removing the mod puts that value back.
//!
//! A key the player or another mod changed since is left alone. Array lines
//! (`+Key=…`, `-Key=…`) are added once and removed only if we added them.

use serde::{Deserialize, Serialize};

/// A key a mod set: in `section`, `key` now holds `value`, which replaced
/// `previous` (none: the key was added).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub section: String,
    pub key: String,
    pub value: String,
    pub previous: Option<String>,
}

/// A settings file, line by line, as it was written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ini {
    lines: Vec<String>,
    crlf: bool,
    bom: bool,
}

impl Ini {
    pub fn parse(text: &str) -> Self {
        let bom = text.starts_with('\u{feff}');
        let text = text.trim_start_matches('\u{feff}');
        Self {
            lines: text.lines().map(str::to_owned).collect(),
            // A new file follows Windows, where the games read it.
            crlf: text.contains("\r\n") || text.is_empty(),
            bom,
        }
    }

    /// Sets the keys of `settings` (another file's text) and returns what
    /// changed. Keys outside a section mean nothing to the game: skipped.
    pub fn merge(&mut self, settings: &str) -> Vec<Entry> {
        let mut changes = Vec::new();
        let mut section = None;
        for line in settings.trim_start_matches('\u{feff}').lines() {
            let line = line.trim();
            if let Some(name) = header(line) {
                section = Some(name.to_owned());
                continue;
            }
            let (Some(section), Some((key, value))) = (&section, key_value(line)) else {
                continue;
            };
            if let Some(entry) = self.set(section, key, value) {
                changes.push(entry);
            }
        }
        changes
    }

    /// Undoes `changes` (from `merge`), latest first. Sections left without
    /// any key are removed.
    pub fn revert(&mut self, changes: &[Entry]) {
        for change in changes.iter().rev() {
            let Some(at) = self.find(&change.section, &change.key, Some(&change.value)) else {
                continue;
            };
            match &change.previous {
                Some(previous) => {
                    self.lines[at] = format!("{}={previous}", key_of(&self.lines[at]));
                }
                None => {
                    self.lines.remove(at);
                }
            }
        }
        for change in changes {
            self.drop_if_empty(&change.section);
        }
    }

    /// No key left: the file can go if we created it.
    pub fn is_empty(&self) -> bool {
        self.lines.iter().all(|l| key_value(l.trim()).is_none())
    }

    fn set(&mut self, section: &str, key: &str, value: &str) -> Option<Entry> {
        let array = key.starts_with(['+', '-', '.', '!']);
        let previous = if array {
            // Already there: not ours to remove later.
            if self.find(section, key, Some(value)).is_some() {
                return None;
            }
            None
        } else {
            match self.find(section, key, None) {
                Some(at) => {
                    let (_, old) = key_value(self.lines[at].trim())?;
                    if old == value {
                        return None;
                    }
                    let old = old.to_owned();
                    self.lines[at] = format!("{}={value}", key_of(&self.lines[at]));
                    return Some(Entry {
                        section: section.to_owned(),
                        key: key.to_owned(),
                        value: value.to_owned(),
                        previous: Some(old),
                    });
                }
                None => None,
            }
        };
        let at = match self.section(section) {
            Some((start, end)) => self.last_content(start, end) + 1,
            None => {
                if self.lines.last().is_some_and(|l| !l.trim().is_empty()) {
                    self.lines.push(String::new());
                }
                self.lines.push(format!("[{section}]"));
                self.lines.len()
            }
        };
        self.lines.insert(at, format!("{key}={value}"));
        Some(Entry {
            section: section.to_owned(),
            key: key.to_owned(),
            value: value.to_owned(),
            previous,
        })
    }

    /// Lines of `name`: its header, and the end (next header or end of
    /// file). Section and key names ignore case, as in the engine.
    fn section(&self, name: &str) -> Option<(usize, usize)> {
        let start = self
            .lines
            .iter()
            .position(|l| header(l.trim()).is_some_and(|h| h.eq_ignore_ascii_case(name)))?;
        let end = self.lines[start + 1..]
            .iter()
            .position(|l| header(l.trim()).is_some())
            .map_or(self.lines.len(), |i| start + 1 + i);
        Some((start, end))
    }

    /// Last non-blank line of a section: new keys go after it.
    fn last_content(&self, start: usize, end: usize) -> usize {
        (start..end)
            .rev()
            .find(|&i| !self.lines[i].trim().is_empty())
            .unwrap_or(start)
    }

    /// The line of `key` in `section`, holding `value` if given.
    fn find(&self, section: &str, key: &str, value: Option<&str>) -> Option<usize> {
        let (start, end) = self.section(section)?;
        (start + 1..end).rev().find(|&i| {
            key_value(self.lines[i].trim()).is_some_and(|(k, v)| {
                k.eq_ignore_ascii_case(key) && value.is_none_or(|value| v == value)
            })
        })
    }

    fn drop_if_empty(&mut self, name: &str) {
        let Some((start, end)) = self.section(name) else {
            return;
        };
        if (start + 1..end).any(|i| !self.lines[i].trim().is_empty()) {
            return;
        }
        self.lines.drain(start..end);
        // The blank line that separated it from the previous section.
        if start > 0 && start == self.lines.len() && self.lines[start - 1].trim().is_empty() {
            self.lines.pop();
        }
    }
}

impl std::fmt::Display for Ini {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let newline = if self.crlf { "\r\n" } else { "\n" };
        if self.bom {
            f.write_str("\u{feff}")?;
        }
        for line in &self.lines {
            f.write_str(line)?;
            f.write_str(newline)?;
        }
        Ok(())
    }
}

fn header(line: &str) -> Option<&str> {
    line.strip_prefix('[')?.strip_suffix(']')
}

/// `key=value`, without comments (`;`, `#`).
fn key_value(line: &str) -> Option<(&str, &str)> {
    if line.starts_with([';', '#']) {
        return None;
    }
    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    (!key.is_empty()).then_some((key, value.trim()))
}

/// The key as the file spells it, to keep it when the value changes.
fn key_of(line: &str) -> &str {
    line.split_once('=').map_or(line, |(k, _)| k.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FOG: &str = "[SystemSettings]\nr.Fog=0\nr.VolumetricFog=0\n";

    #[test]
    fn a_new_file_gets_the_keys_and_loses_them_on_revert() {
        let mut ini = Ini::parse("");
        let changes = ini.merge(FOG);
        assert_eq!(ini.to_string(), "[SystemSettings]\r\nr.Fog=0\r\nr.VolumetricFog=0\r\n");
        ini.revert(&changes);
        assert!(ini.is_empty());
        assert_eq!(ini.to_string(), "");
    }

    #[test]
    fn existing_keys_are_replaced_then_put_back() {
        const ENGINE: &str = "[/Script/Engine.Engine]\nbSmoothFrameRate=True\n\n";
        let before = format!("{ENGINE}[SystemSettings]\nR.FOG=1\n");
        let mut ini = Ini::parse(&before);
        let changes = ini.merge(FOG);
        assert_eq!(
            ini.to_string(),
            format!("{ENGINE}[SystemSettings]\nR.FOG=0\nr.VolumetricFog=0\n")
        );
        assert_eq!(changes[0].previous.as_deref(), Some("1"));
        ini.revert(&changes);
        assert_eq!(ini.to_string(), before);
    }

    #[test]
    fn a_key_changed_since_is_left_alone() {
        let mut ini = Ini::parse("");
        let changes = ini.merge(FOG);
        ini.merge("[SystemSettings]\nr.Fog=1\n");
        ini.revert(&changes);
        assert_eq!(ini.to_string(), "[SystemSettings]\r\nr.Fog=1\r\n");
    }

    #[test]
    fn array_lines_are_added_once_and_removed_only_if_ours() {
        let before = "[Core.Log]\n+Paths=A\n";
        let mut ini = Ini::parse(before);
        let changes = ini.merge("[Core.Log]\n+Paths=A\n+Paths=B\n; comment=no\nstray\n");
        assert_eq!(ini.to_string(), "[Core.Log]\n+Paths=A\n+Paths=B\n");
        assert_eq!(changes.len(), 1);
        ini.revert(&changes);
        assert_eq!(ini.to_string(), before);
    }

    #[test]
    fn the_byte_order_mark_and_line_endings_are_kept() {
        let before = "\u{feff}[A]\r\nx=1\r\n";
        let mut ini = Ini::parse(before);
        let changes = ini.merge("[A]\nx=2\n");
        assert_eq!(ini.to_string(), "\u{feff}[A]\r\nx=2\r\n");
        ini.revert(&changes);
        assert_eq!(ini.to_string(), before);
    }
}

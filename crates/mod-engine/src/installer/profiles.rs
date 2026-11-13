//! Mod profiles: several lists of mods for one game, to go from one to
//! another without removing anything. A mod out of the profile in use is
//! parked: hidden, its files out of the game, kept in the storage for the
//! profiles that have it. The loader too: a profile without mods plays the
//! game as it shipped. Switching downloads nothing again.
//!
//! The active profile is the live state: the mods not parked, and their
//! switches. A mod installed while it is active belongs to it only. The
//! others keep their list as it was when they were left.

use std::collections::HashSet;
use std::path::Path;

use super::{InstallError, Paths, now, purge, toggle};
use crate::state::{InstalledMod, ModProfile, State};

/// The profile every game starts with, before a second one is made.
pub const DEFAULT_PROFILE: &str = "Default";

/// Longest profile name, in characters.
pub(super) const MAX_NAME: usize = 40;

/// The game's profiles, by creation, the active one as it is now.
pub fn profiles(state: &State, game: &str) -> Result<Vec<ModProfile>, InstallError> {
    let mut list = state.mod_profiles(game)?;
    if list.is_empty() {
        list.push(ModProfile {
            name: DEFAULT_PROFILE.to_owned(),
            mods: Vec::new(),
            off: Vec::new(),
            active: true,
            created_at: 0,
        });
    }
    let mods = state.list(game)?;
    for p in &mut list {
        if p.active {
            let present = mods.iter().filter(|m| !m.parked);
            p.mods = present.clone().map(|m| m.id.clone()).collect();
            p.off = present.filter(|m| !m.enabled).map(|m| m.id.clone()).collect();
        } else {
            // Removed since: nothing left to bring back.
            p.mods.retain(|id| mods.iter().any(|m| &m.id == id));
            p.off.retain(|id| p.mods.contains(id));
        }
    }
    Ok(list)
}

/// Makes a profile and switches to it. `copy`: with the mods of the profile
/// in use, as they are, so nothing changes in the game; otherwise empty,
/// the game without mods.
pub fn create_profile(
    state: &mut State,
    paths: &Paths,
    game: &str,
    game_dir: &Path,
    name: &str,
    copy: bool,
) -> Result<Vec<ModProfile>, InstallError> {
    let mut list = profiles(state, game)?;
    let name = valid_name(name, &list, None)?;
    let (mods, off) = match list.iter().find(|p| p.active) {
        Some(active) if copy => (active.mods.clone(), active.off.clone()),
        _ => (Vec::new(), Vec::new()),
    };
    list.push(ModProfile {
        name: name.clone(),
        mods,
        off,
        active: false,
        created_at: now(),
    });
    state.set_mod_profiles(game, &list)?;
    switch_profile(state, paths, game, game_dir, &name)
}

/// Puts the mods of profile `name` in the game, as it left them, and parks
/// the others. All or nothing: on failure, the game is back as it was.
pub fn switch_profile(
    state: &mut State,
    paths: &Paths,
    game: &str,
    game_dir: &Path,
    name: &str,
) -> Result<Vec<ModProfile>, InstallError> {
    // The mods are out of the game but on in their records: turning them
    // on or off now would place them during the session.
    if state.is_vanilla(game)? {
        return Err(InstallError::Vanilla);
    }
    let mut list = profiles(state, game)?;
    let target = list.iter().find(|p| p.name == name).ok_or(InstallError::NoProfile)?;
    if target.active {
        return Ok(list);
    }
    let members: HashSet<&str> = target.mods.iter().map(String::as_str).collect();
    let off: HashSet<&str> = target.off.iter().map(String::as_str).collect();
    let wanted = |m: &InstalledMod| members.contains(m.id.as_str()) && !off.contains(m.id.as_str());

    let mut changes: Vec<(String, bool, bool)> = state
        .list(game)?
        .into_iter()
        .filter(|m| m.enabled != wanted(m))
        .map(|m| (m.id.clone(), !m.enabled, m.is_loader()))
        .collect();
    // Off first, the loader after its mods: the files they free go to the
    // mods coming on. Then the loader first, before the mods it loads.
    changes.sort_by_key(|&(_, enabled, loader)| (enabled, loader != enabled));

    for (index, (id, enabled, _)) in changes.iter().enumerate() {
        if let Err(error) = turn(state, paths, game, game_dir, id, *enabled) {
            for (id, enabled, _) in changes[..index].iter().rev() {
                let _ = turn(state, paths, game, game_dir, id, !enabled);
            }
            return Err(error);
        }
    }

    let mods: Vec<InstalledMod> = state
        .list(game)?
        .into_iter()
        .map(|m| InstalledMod {
            parked: !members.contains(m.id.as_str()),
            ..m
        })
        .collect();
    state.save_all(&mods)?;
    for p in &mut list {
        p.active = p.name == name;
    }
    state.set_mod_profiles(game, &list)?;
    profiles(state, game)
}

pub fn rename_profile(state: &mut State, game: &str, from: &str, to: &str) -> Result<Vec<ModProfile>, InstallError> {
    let mut list = profiles(state, game)?;
    let index = list.iter().position(|p| p.name == from).ok_or(InstallError::NoProfile)?;
    list[index].name = valid_name(to, &list, Some(index))?;
    state.set_mod_profiles(game, &list)?;
    profiles(state, game)
}

/// Forgets a profile. Its mods no other profile has are uninstalled, and
/// their storage freed: they are parked, nothing of them is in the game.
pub fn delete_profile(
    state: &mut State,
    paths: &Paths,
    game: &str,
    name: &str,
) -> Result<Vec<ModProfile>, InstallError> {
    let mut list = profiles(state, game)?;
    let index = list.iter().position(|p| p.name == name).ok_or(InstallError::NoProfile)?;
    if list[index].active {
        return Err(InstallError::ProfileInUse);
    }
    list.remove(index);
    state.set_mod_profiles(game, &list)?;

    let kept: HashSet<&str> = list.iter().flat_map(|p| p.mods.iter().map(String::as_str)).collect();
    let mut orphans: Vec<InstalledMod> = state
        .list(game)?
        .into_iter()
        .filter(|m| m.parked && !kept.contains(m.id.as_str()))
        .collect();
    // The loader last, like any uninstall.
    orphans.sort_by_key(InstalledMod::is_loader);
    for m in &orphans {
        state.remove(game, &m.id)?;
        purge(state, paths, game, &m.id)?;
    }
    profiles(state, game)
}

/// Is the mod in a profile other than the one in use?
pub(super) fn kept_elsewhere(state: &State, game: &str, id: &str) -> Result<bool, InstallError> {
    Ok(state
        .mod_profiles(game)?
        .iter()
        .any(|p| !p.active && p.mods.iter().any(|m| m == id)))
}

fn turn(state: &mut State, paths: &Paths, game: &str, game_dir: &Path, id: &str, enabled: bool) -> Result<(), InstallError> {
    let m = state.get(game, id)?.ok_or(InstallError::NotInstalled)?;
    toggle(state, paths, game, game_dir, m, enabled).map(drop)
}

/// `name` trimmed, if no other profile has it (case aside). `own`: the
/// profile being renamed.
fn valid_name(name: &str, list: &[ModProfile], own: Option<usize>) -> Result<String, InstallError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return Err(InstallError::ProfileName);
    }
    let lower = name.to_lowercase();
    let taken = list
        .iter()
        .enumerate()
        .any(|(i, p)| Some(i) != own && p.name.to_lowercase() == lower);
    if taken {
        return Err(InstallError::ProfileExists(name.to_owned()));
    }
    Ok(name.to_owned())
}

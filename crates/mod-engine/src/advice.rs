//! Ce que l'utilisateur peut faire après une erreur (DESIGN, « Textes » :
//! ce qui s'est passé, puis quoi faire).
//!
//! Une erreur arrive souvent emballée : « couldn't download X », avec la vraie
//! raison (« not enough disk space ») plus bas dans la chaîne. On remonte
//! cette raison dans la phrase, et on y ajoute une action.

use std::error::Error;

use crate::archive::ArchiveError;
use crate::deploy::DeployError;
use crate::download::DownloadError;
use crate::installer::InstallError;
use crate::resolver::ResolveError;
use crate::sources::SourceError;
use crate::state::StateError;

const CONNECTION: &str = "Check your connection and try again.";
const DISK: &str = "Make sure the drive isn't full or read-only, then try again.";
const BROKEN_FILE: &str = "Try again. If it keeps happening, the file on the source is broken: tell the mod author.";
const BLOCKED: &str = "Unalia blocked it to protect your PC. Tell the mod author.";
const REPORT: &str = "Restart Unalia. If it keeps happening, copy a report from Settings > Support.";
const LATER: &str = "Try again in a few minutes.";

/// Message d'une ligne : l'erreur, sa raison utile si elle est plus bas
/// dans la chaîne, et l'action conseillée.
pub fn explain(error: &(dyn Error + 'static)) -> String {
    let mut message = error.to_string();
    let mut action = None;
    let mut cause_shown = false;
    let mut current = Some(error);
    let mut outer = true;
    while let Some(e) = current {
        let advice = advise(e);
        if !outer && !cause_shown && advice.show_cause {
            message = format!("{message}: {e}");
            cause_shown = true;
        }
        // L'action la plus précise est la plus profonde.
        if advice.action.is_some() {
            action = advice.action;
        }
        outer = false;
        current = e.source();
    }
    match action {
        Some(action) => format!("{message}. {action}"),
        None => message,
    }
}

#[derive(Default)]
struct Advice {
    /// Le message de cette erreur dit à lui seul ce qui bloque.
    show_cause: bool,
    action: Option<String>,
}

fn advice(show_cause: bool, action: &str) -> Advice {
    Advice {
        show_cause,
        action: Some(action.to_owned()),
    }
}

fn advise(e: &(dyn Error + 'static)) -> Advice {
    if let Some(e) = e.downcast_ref::<InstallError>() {
        return install(e);
    }
    if let Some(e) = e.downcast_ref::<ResolveError>() {
        return resolve(e);
    }
    if let Some(e) = e.downcast_ref::<SourceError>() {
        return source(e);
    }
    if let Some(e) = e.downcast_ref::<DownloadError>() {
        return download(e);
    }
    if let Some(e) = e.downcast_ref::<ArchiveError>() {
        return archive(e);
    }
    if let Some(e) = e.downcast_ref::<DeployError>() {
        return deploy(e);
    }
    if e.downcast_ref::<StateError>().is_some() {
        return advice(true, REPORT);
    }
    Advice::default()
}

fn install(e: &InstallError) -> Advice {
    match e {
        // Variantes transparentes : le message est déjà celui de la source.
        InstallError::Resolve(e) => resolve(e),
        InstallError::Source(e) => source(e),
        InstallError::State(_) => advice(true, REPORT),
        InstallError::Conflict { other, .. } => Advice {
            show_cause: true,
            action: Some(format!("Turn off or remove {other} first.")),
        },
        InstallError::NothingToInstall(_) | InstallError::NoFile(_) => advice(
            true,
            "This mod's layout isn't supported yet: copy a report from Settings > Support so it can be.",
        ),
        InstallError::Fomod(_) => advice(true, "Pick another file of this mod if it has one, or check back soon."),
        InstallError::NoGameFiles => advice(
            true,
            "In Steam, verify the game's files (Properties > Installed Files), then try again.",
        ),
        InstallError::NoReleaseFile { .. } => advice(true, "The author may still be uploading it: try again later."),
        InstallError::LoaderPresent => advice(
            true,
            "Click Reset mods under Play to clear it, then install again: Unalia will manage it.",
        ),
        InstallError::Io(_) => advice(true, DISK),
        _ => Advice::default(),
    }
}

fn resolve(e: &ResolveError) -> Advice {
    match e {
        ResolveError::TooOld { .. } => advice(true, "Wait for the dependency to be updated, or tell the mod author."),
        ResolveError::NoVersion(_) | ResolveError::Cycle(_) | ResolveError::TooMany => {
            advice(true, "Tell the mod author: Unalia can't install it as published.")
        }
        ResolveError::Source { .. } => Advice::default(),
    }
}

fn source(e: &SourceError) -> Advice {
    match e {
        SourceError::Network { .. } => advice(true, CONNECTION),
        SourceError::NotConnected { .. } => advice(true, "Link it in Settings > Connections."),
        SourceError::Unauthorized { .. } => advice(true, "Link your account again in Settings > Connections."),
        SourceError::Unexpected { .. } => advice(true, LATER),
        // Le message dit déjà quoi faire, ou il n'y a rien à faire.
        SourceError::NotFound { .. } | SourceError::RateLimited { .. } | SourceError::WebsiteOnly { .. } => Advice {
            show_cause: true,
            action: None,
        },
    }
}

fn download(e: &DownloadError) -> Advice {
    match e {
        // « the download failed » n'apprend rien : l'action suffit.
        DownloadError::Network(_) => Advice {
            show_cause: false,
            action: Some(CONNECTION.into()),
        },
        DownloadError::NoSpace { .. } => advice(true, "Free up some space, then try again."),
        DownloadError::HashMismatch { .. } => advice(true, BROKEN_FILE),
        DownloadError::Protocol(_) => advice(true, LATER),
        DownloadError::Io(_) => advice(false, DISK),
        DownloadError::TooLarge => Advice {
            show_cause: true,
            action: None,
        },
        DownloadError::Cancelled => Advice::default(),
    }
}

fn archive(e: &ArchiveError) -> Advice {
    match e {
        ArchiveError::UnsafePath(_)
        | ArchiveError::Link(_)
        | ArchiveError::TooLarge
        | ArchiveError::TooManyEntries
        | ArchiveError::SuspiciousRatio(_) => advice(true, BLOCKED),
        ArchiveError::Corrupt(_) | ArchiveError::SevenZip(_) | ArchiveError::Rar(_) => advice(true, BROKEN_FILE),
        ArchiveError::Unsupported(_) => advice(true, "Ask the mod author for a .zip version."),
        ArchiveError::Io(_) => advice(false, DISK),
        ArchiveError::Cancelled => Advice::default(),
    }
}

fn deploy(e: &DeployError) -> Advice {
    match e {
        DeployError::Conflict(_) => advice(
            true,
            "Another program put this file there: click Reset mods under Play, then try again.",
        ),
        DeployError::UnsafeTarget(_) => advice(true, BLOCKED),
        DeployError::MissingSource(_) => advice(true, "Remove the mod, then install it again."),
        DeployError::Io { .. } => advice(true, "Close the game and any program using its folder, then try again."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brings_the_real_reason_up_with_an_action() {
        let error = InstallError::Download {
            name: "Big Mod".into(),
            source: DownloadError::NoSpace {
                needed: "2 GB".into(),
                available: "1 GB".into(),
            },
        };
        assert_eq!(
            explain(&error),
            "couldn't download Big Mod: not enough disk space: 2 GB needed, 1 GB free. Free up some space, then try again."
        );
    }

    #[test]
    fn keeps_a_clear_message_as_is() {
        let error = InstallError::Source(SourceError::Unauthorized { source_name: "Nexus Mods" });
        assert_eq!(
            explain(&error),
            "Nexus Mods refused your API key. Link your account again in Settings > Connections."
        );
        let error = InstallError::Archive {
            name: "Pack".into(),
            source: ArchiveError::Unsupported("Multi-part RAR"),
        };
        assert_eq!(
            explain(&error),
            "couldn't unpack Pack: Multi-part RAR archives aren't supported yet. Ask the mod author for a .zip version."
        );
    }

    #[test]
    fn says_nothing_more_when_there_is_nothing_to_do() {
        assert_eq!(explain(&InstallError::Cancelled), "the install was cancelled");
    }
}

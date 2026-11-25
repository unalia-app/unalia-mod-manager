//! The system's vault (Credential Manager on Windows, Keychain on macOS,
//! Secret Service on Linux): the Nexus login is kept there, never in the
//! database or a file.

use keyring::v1::{Entry, Error};

const SERVICE: &str = "app.unalia.modmanager";

fn nexus(service: &str) -> Result<Entry, Error> {
    Entry::new(service, "nexus-refresh-token")
}

pub fn load_nexus() -> Option<String> {
    load(SERVICE)
}

pub fn save_nexus(refresh_token: &str) -> Result<(), Error> {
    nexus(SERVICE)?.set_password(refresh_token)
}

/// Nothing kept is fine too.
pub fn forget_nexus() {
    forget(SERVICE);
}

fn load(service: &str) -> Option<String> {
    match nexus(service).and_then(|entry| entry.get_password()) {
        Ok(token) => Some(token),
        Err(Error::NoEntry) => None,
        Err(error) => {
            log::warn!("couldn't read the Nexus login from the system's vault: {error}");
            None
        }
    }
}

fn forget(service: &str) {
    match nexus(service).and_then(|entry| entry.delete_credential()) {
        Ok(()) | Err(Error::NoEntry) => {}
        Err(error) => log::warn!("couldn't remove the Nexus login from the system's vault: {error}"),
    }
}

// The other systems may have no vault on a build machine.
#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn a_login_is_kept_then_forgotten() {
        let service = format!("app.unalia.modmanager.test-{}", std::process::id());
        assert_eq!(load(&service), None);
        nexus(&service).unwrap().set_password("secret").unwrap();
        assert_eq!(load(&service).as_deref(), Some("secret"));
        forget(&service);
        assert_eq!(load(&service), None);
        forget(&service);
    }
}

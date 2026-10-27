use std::path::PathBuf;

use coruframe_core::private_fs;

const SERVICE: &str = "xyz.coruscnium.coruframe";
const USER: &str = "warframe.market";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreErrorKind {
    // No entry stored: migrate or report disconnected.
    Missing,
    // The Secret Service is genuinely absent; the file fallback is allowed.
    Unavailable,
    // Locked wallet, cancelled prompt, D-Bus error: never fall back silently.
    Failed,
}

#[derive(Debug, Clone)]
pub struct StoreError {
    pub kind: StoreErrorKind,
    pub message: String,
}

impl StoreError {
    fn failed(message: impl Into<String>) -> Self {
        Self {
            kind: StoreErrorKind::Failed,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Storage {
    Wallet,
    File,
}

impl Storage {
    pub fn name(self) -> &'static str {
        match self {
            Self::Wallet => "wallet",
            Self::File => "file",
        }
    }
}

pub trait CredentialStore: Send + Sync {
    fn load(&self) -> Result<Option<String>, StoreError>;
    fn save(&self, token: &str) -> Result<(), StoreError>;
    fn delete(&self) -> Result<(), StoreError>;
}

pub struct SecretServiceStore;

impl SecretServiceStore {
    fn entry() -> Result<keyring::Entry, StoreError> {
        keyring::Entry::new(SERVICE, USER).map_err(map_error)
    }
}

impl CredentialStore for SecretServiceStore {
    fn load(&self) -> Result<Option<String>, StoreError> {
        match Self::entry()?.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(map_error(error)),
        }
    }

    fn save(&self, token: &str) -> Result<(), StoreError> {
        Self::entry()?.set_password(token).map_err(map_error)
    }

    fn delete(&self) -> Result<(), StoreError> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(map_error(error)),
        }
    }
}

fn map_error(error: keyring::Error) -> StoreError {
    let kind = match error {
        keyring::Error::NoEntry => StoreErrorKind::Missing,
        keyring::Error::NoDefaultStore => StoreErrorKind::Unavailable,
        _ => StoreErrorKind::Failed,
    };
    StoreError {
        kind,
        message: error.to_string(),
    }
}

// The legacy plaintext token, kept only when the Secret Service is absent (or
// when the rollback env var pins the file store for one release).
pub struct FileStore;

pub fn legacy_token_path() -> PathBuf {
    private_fs::config_dir().join("market-token")
}

impl CredentialStore for FileStore {
    fn load(&self) -> Result<Option<String>, StoreError> {
        match std::fs::read_to_string(legacy_token_path()) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(StoreError::failed(error.to_string())),
        }
    }

    fn save(&self, token: &str) -> Result<(), StoreError> {
        private_fs::write_private(&legacy_token_path(), token.as_bytes())
            .map_err(|error| StoreError::failed(error.to_string()))
    }

    fn delete(&self) -> Result<(), StoreError> {
        let _ = std::fs::remove_file(legacy_token_path());
        Ok(())
    }
}

pub fn legacy_token_exists() -> bool {
    legacy_token_path().is_file()
}

// A file store is chosen only on an explicit request or when the Secret Service
// cannot be initialised at all. A locked or cancelled wallet is never a reason.
pub fn choose(force_file: bool) -> (Box<dyn CredentialStore>, Storage) {
    if force_file {
        return (Box::new(FileStore), Storage::File);
    }
    if matches!(keyring::Entry::store_status(), Ok(())) {
        (Box::new(SecretServiceStore), Storage::Wallet)
    } else {
        (Box::new(FileStore), Storage::File)
    }
}

pub fn force_file_store() -> bool {
    std::env::var("CORUFRAME_TOKEN_STORE")
        .map(|value| value.eq_ignore_ascii_case("file"))
        .unwrap_or(false)
}

// One prefix only, and the exact character set a JWT or the v2 token uses.
pub fn normalize(token: &str) -> String {
    let trimmed = token.trim();
    trimmed
        .strip_prefix("Bearer ")
        .or_else(|| trimmed.strip_prefix("JWT "))
        .unwrap_or(trimmed)
        .trim()
        .to_string()
}

pub fn validate(token: &str) -> Result<String, String> {
    let token = normalize(token);
    let valid = (16..=8192).contains(&token.len())
        && token.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'.' | b'_' | b'~' | b'+' | b'/' | b'=' | b'-')
        });
    if valid {
        Ok(token)
    } else {
        Err("the token is not a valid warframe.market session token".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeStore {
        value: std::sync::Mutex<Option<String>>,
        fail_save: bool,
        fail_read: bool,
    }

    impl CredentialStore for FakeStore {
        fn load(&self) -> Result<Option<String>, StoreError> {
            if self.fail_read {
                return Err(StoreError::failed("locked"));
            }
            Ok(self.value.lock().unwrap().clone())
        }

        fn save(&self, token: &str) -> Result<(), StoreError> {
            if self.fail_save {
                return Err(StoreError::failed("denied"));
            }
            *self.value.lock().unwrap() = Some(token.to_string());
            Ok(())
        }

        fn delete(&self) -> Result<(), StoreError> {
            *self.value.lock().unwrap() = None;
            Ok(())
        }
    }

    #[test]
    fn normalizes_one_prefix_and_validates_the_charset() {
        assert_eq!(normalize("Bearer abc.def"), "abc.def");
        assert_eq!(normalize("JWT abc.def"), "abc.def");
        assert_eq!(normalize("  abc.def  "), "abc.def");
        assert!(validate("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abc123").is_ok());
        assert!(validate("short").is_err());
        assert!(validate("has space in it and is long enough").is_err());
        assert!(validate("line\nbreak-token-that-is-long-enough").is_err());
        assert!(validate(&"a".repeat(8193)).is_err());
    }

    #[test]
    fn a_save_failure_does_not_report_success() {
        let store = FakeStore {
            value: std::sync::Mutex::new(None),
            fail_save: true,
            fail_read: false,
        };
        assert!(store.save("token-that-is-long-enough").is_err());
        assert!(store.load().unwrap().is_none());
    }
}

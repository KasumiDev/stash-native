use super::Error;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Private runtime settings; Debug intentionally excludes credentials and the server address.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub server_url: String,
    pub api_key: String,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            server_url: std::env::var("STASH_URL")
                .or_else(|_| std::env::var("STASH_SERVER_URL"))
                .unwrap_or_default(),
            api_key: std::env::var("STASH_API_KEY").unwrap_or_default(),
        }
    }

    pub fn endpoint(&self) -> Result<String, Error> {
        let url = self.server_url.trim().trim_end_matches('/');
        let tail = url
            .strip_prefix("http://")
            .or_else(|| url.strip_prefix("https://"))
            .ok_or(Error::Configuration)?;
        let authority = tail.split('/').next().unwrap_or_default();
        if authority.is_empty()
            || authority.contains('@')
            || url.contains(['?', '#'])
            || url.chars().any(char::is_whitespace)
            || url.contains('\0')
            || self.api_key.contains(['\r', '\n', '\0'])
        {
            return Err(Error::Configuration);
        }
        Ok(if url.ends_with("/graphql") {
            url.to_owned()
        } else {
            format!("{url}/graphql")
        })
    }

    pub fn asset_url(&self, path: &str) -> Result<String, Error> {
        if path.chars().any(|c| c.is_control() || c.is_whitespace()) || path.contains(['#', '\\']) {
            return Err(Error::Configuration);
        }
        if path.starts_with("http://") || path.starts_with("https://") {
            let authority = path
                .split_once("://")
                .unwrap()
                .1
                .split(['/', '?'])
                .next()
                .unwrap_or_default();
            if authority.is_empty() || authority.contains('@') {
                return Err(Error::Configuration);
            }
            return Ok(path.to_owned());
        }
        if !path.starts_with('/') || path.starts_with("//") {
            return Err(Error::Configuration);
        }
        let endpoint = self.endpoint()?;
        let start = endpoint.find("://").ok_or(Error::Configuration)? + 3;
        let end = endpoint[start..]
            .find('/')
            .map(|i| start + i)
            .unwrap_or(endpoint.len());
        Ok(format!("{}{path}", &endpoint[..end]))
    }

    pub fn load(path: &Path) -> Result<Self, Error> {
        use std::io::Read;
        let file = std::fs::File::open(path).map_err(|_| Error::Storage)?;
        let mut bytes = Vec::new();
        file.take(65537)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::Storage)?;
        if bytes.len() > 65536 {
            return Err(Error::Storage);
        }
        serde_json::from_slice(&bytes).map_err(|_| Error::Storage)
    }

    /// Replace settings atomically. Call on the storage worker. Unix credentials are owner-only.
    pub fn save(&self, path: &Path) -> Result<(), Error> {
        self.endpoint()?;
        let parent = path.parent().ok_or(Error::Storage)?;
        std::fs::create_dir_all(parent).map_err(|_| Error::Storage)?;
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
        let name = path.file_name().ok_or(Error::Storage)?.to_string_lossy();
        let temporary = parent.join(format!(
            ".{name}.{}.{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let bytes = serde_json::to_vec(self).map_err(|_| Error::Storage)?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        use std::io::Write;
        let mut file = options.open(&temporary).map_err(|_| Error::Storage)?;
        let result = file.write_all(&bytes).and_then(|_| file.sync_all());
        drop(file);
        let result = result.and_then(|_| std::fs::rename(&temporary, path));
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result.map_err(|_| Error::Storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_accepts_root_and_graphql() {
        let mut c = Config {
            server_url: "https://example.test/stash/".into(),
            api_key: String::new(),
        };
        assert_eq!(c.endpoint().unwrap(), "https://example.test/stash/graphql");
        c.server_url = "http://example.test/graphql".into();
        assert_eq!(c.endpoint().unwrap(), c.server_url);
        c.api_key = "secret\r\nHeader: injected".into();
        assert!(c.endpoint().is_err());
    }
    #[test]
    fn media_urls_reject_fragments_and_userinfo_before_adding_credentials() {
        let c = Config {
            server_url: "https://example.test".into(),
            api_key: String::new(),
        };
        for path in [
            "https://example.test/stream#fragment",
            "https://user@example.test/stream",
            "https://example.test/stream\r\nInjected",
            "/stream\\other",
        ] {
            assert!(c.asset_url(path).is_err());
        }
        assert_eq!(
            c.asset_url("/stream?resolution=LOW").unwrap(),
            "https://example.test/stream?resolution=LOW"
        );
    }

    #[test]
    fn settings_roundtrip_is_bounded_and_owner_only() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("stash-config-test-{}-{nonce}", std::process::id()));
        let path = dir.join("stash.json");
        let c = Config {
            server_url: "https://example.test/graphql".into(),
            api_key: "synthetic-key".into(),
        };
        c.save(&path).unwrap();
        let restored = Config::load(&path).unwrap();
        assert_eq!(restored.server_url, c.server_url);
        assert_eq!(restored.api_key, c.api_key);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::write(&path, vec![b' '; 65537]).unwrap();
        assert!(matches!(Config::load(&path), Err(Error::Storage)));
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }
}

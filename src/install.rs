//! Agent installs: bearer token + OpenCode MCP registration.

use std::path::PathBuf;

use rand::RngExt as _;

use crate::Error;

/// Default HTTP port for `serve`.
pub const DEFAULT_PORT: u16 = 3210;

fn global_dir() -> PathBuf {
    std::env::var_os("HOME").map_or_else(
        || PathBuf::from(".one-grep"),
        |home| PathBuf::from(home).join(".one-grep"),
    )
}

/// Read the bearer token, generating and storing one (mode 0600) on first use.
///
/// # Errors
///
/// Returns [`Error`] when the token cannot be read or written.
pub fn ensure_token() -> Result<String, Error> {
    ensure_token_in(&global_dir())
}

fn ensure_token_in(dir: &std::path::Path) -> Result<String, Error> {
    let path = dir.join("token");
    if path.exists() {
        return Ok(std::fs::read_to_string(&path)?.trim().to_owned());
    }
    let token: String = rand::rng()
        .sample_iter(&rand::distr::Alphanumeric)
        .take(48)
        .map(char::from)
        .collect();
    std::fs::create_dir_all(global_dir())?;
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(dir.join("token"))?;
        use std::io::Write as _;
        file.write_all(token.as_bytes())?;
    }
    Ok(token)
}

fn opencode_config_path() -> PathBuf {
    std::env::var_os("HOME").map_or_else(
        || PathBuf::from("opencode.json"),
        |home| {
            PathBuf::from(home)
                .join(".config")
                .join("opencode")
                .join("opencode.json")
        },
    )
}

/// Register one-grep in OpenCode's `mcp` section.
///
/// `http` selects the loopback+bearer entry; otherwise a stdio entry using
/// the current binary is written. Existing keys are preserved.
///
/// # Errors
///
/// Returns [`Error`] when the config cannot be read or written.
pub fn install_opencode(http: bool, port: u16) -> Result<PathBuf, Error> {
    let path = opencode_config_path();
    let mut config: serde_json::Value = if path.exists() {
        serde_json::from_slice(&std::fs::read(&path)?)?
    } else {
        serde_json::json!({})
    };
    let exe = std::env::current_exe()?.to_string_lossy().into_owned();
    let entry = if http {
        serde_json::json!({
            "type": "remote",
            "url": format!("http://127.0.0.1:{port}/mcp"),
            "headers": { "Authorization": format!("Bearer {}", ensure_token()?) },
            "enabled": true,
        })
    } else {
        serde_json::json!({
            "type": "local",
            "command": [exe, "serve", "--stdio"],
            "enabled": true,
        })
    };
    config
        .as_object_mut()
        .ok_or_else(|| Error::InvalidInput("opencode.json is not an object".to_owned()))?
        .entry("mcp")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| Error::InvalidInput("opencode.json mcp is not an object".to_owned()))?
        .insert("one-grep".to_owned(), entry);
    std::fs::write(&path, serde_json::to_string_pretty(&config)?)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_stable_and_sized() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = ensure_token_in(dir.path()).expect("generate");
        let again = ensure_token_in(dir.path()).expect("read");
        assert_eq!(first, again);
        assert_eq!(first.len(), 48);
    }
}

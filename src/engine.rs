//! one-grep engine shared bits: index location + workspace walker.
//!
//! The walker is gitignore-aware and skips the index dir, build output,
//! and lockfiles — none are worth indexing or embedding.

use std::path::Path;

/// Workspace index location: `<workspace>/.one-grep/`.
#[must_use]
pub fn index_dir(workspace: &Path) -> std::path::PathBuf {
    workspace.join(".one-grep")
}

/// Lockfiles and generated bundles: expensive, useless for retrieval.
const SKIP_FILES: &[&str] = &[
    "Cargo.lock",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "flake.lock",
    "Gemfile.lock",
    "poetry.lock",
];

/// Build-output dirs: never indexed.
const SKIP_DIRS: &[&str] = &["target", "node_modules", "dist", "build", ".git"];

/// Data/model blobs: expensive, useless for retrieval.
const SKIP_EXTS: &[&str] = &[
    "gguf",
    "jsonl",
    "safetensors",
    "onnx",
    "pt",
    "bin",
    "pyc",
    "png",
    "jpg",
    "jpeg",
    "gif",
    "pdf",
    "zip",
    "tar",
    "gz",
];

fn skipped(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if name == ".one-grep" || SKIP_FILES.contains(&name) || SKIP_DIRS.contains(&name) {
        return true;
    }
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| SKIP_EXTS.contains(&e))
}

/// Gitignore-aware walker shared by indexing, vectors, and future passes.
#[must_use]
pub fn walker(workspace: &Path) -> ignore::Walk {
    let mut builder = ignore::WalkBuilder::new(workspace);
    builder
        .hidden(true)
        .parents(true)
        .git_ignore(true)
        .require_git(false)
        .filter_entry(|e| !skipped(e.path()));
    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_dir_lives_under_workspace() {
        let dir = index_dir(Path::new("/tmp/ws"));
        assert_eq!(dir, Path::new("/tmp/ws/.one-grep"));
    }

    #[test]
    fn walker_skips_index_build_and_locks() {
        let dir = tempfile::tempdir().expect("tempdir");
        for name in [
            ".one-grep/x",
            "target/x",
            "node_modules/x",
            "Cargo.lock",
            "src/main.rs",
        ] {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(&path, "hello\n").expect("write");
        }
        let mut seen: Vec<String> = walker(dir.path())
            .filter_map(Result::ok)
            .filter(|e| e.path().is_file())
            .map(|e| {
                e.path()
                    .strip_prefix(dir.path())
                    .expect("prefix")
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        seen.sort();
        assert_eq!(seen, vec!["src/main.rs"]);
    }
}

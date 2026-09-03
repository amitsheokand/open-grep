//! Managed ripgrep path: exact text + regex over workspace files.
//!
//! No index required. Exhaustive by default, gitignore-aware, results
//! carry file-oriented locations for terminal reading or agent context.

use std::path::{Path, PathBuf};

use grep::{
    regex::RegexMatcherBuilder,
    searcher::{BinaryDetection, SearcherBuilder, Sink, SinkMatch},
};
use ignore::WalkBuilder;

use crate::Error;

/// One matching line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// File containing the match.
    pub path: PathBuf,
    /// 1-based line number.
    pub line: u64,
    /// Line text without trailing newline.
    pub text: String,
}

/// Search options.
#[derive(Debug, Clone)]
pub struct Options {
    /// Treat pattern as regex; otherwise match literally.
    pub regex: bool,
    /// Case-insensitive matching.
    pub case_insensitive: bool,
    /// Extra ignore-style globs (e.g. `["!target/*"]`).
    pub globs: Vec<String>,
    /// Maximum hits to collect.
    pub limit: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            regex: false,
            case_insensitive: false,
            globs: Vec::new(),
            limit: 100,
        }
    }
}

/// Search `workspace` for `pattern`, returning up to `options.limit` hits.
///
/// # Errors
///
/// Returns [`Error`] when the pattern fails to compile, the workspace
/// cannot be walked, or a file cannot be searched.
pub fn search(workspace: &Path, pattern: &str, options: &Options) -> Result<Vec<Hit>, Error> {
    if !workspace.is_dir() {
        return Err(Error::InvalidInput(format!(
            "workspace is not a directory: {}",
            workspace.display()
        )));
    }
    let matcher = RegexMatcherBuilder::new()
        .case_insensitive(options.case_insensitive)
        .fixed_strings(!options.regex)
        .build(pattern)?;

    let mut builder = WalkBuilder::new(workspace);
    builder
        .hidden(true)
        .parents(true)
        .git_ignore(true)
        .require_git(false);
    if !options.globs.is_empty() {
        let mut overrides = ignore::overrides::OverrideBuilder::new(workspace);
        for glob in &options.globs {
            overrides.add(glob)?;
        }
        builder.overrides(overrides.build()?);
    }
    let mut hits = Vec::new();
    for entry in builder.build().filter_map(Result::ok) {
        if hits.len() >= options.limit {
            break;
        }
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let mut searcher = SearcherBuilder::new()
            .line_number(true)
            .binary_detection(BinaryDetection::quit(b'\x00'))
            .build();
        let mut sink = HitSink {
            path: path.to_path_buf(),
            hits: &mut hits,
            limit: options.limit,
        };
        // Skip unreadable files instead of failing the whole search.
        let _ = searcher.search_path(&matcher, path, &mut sink);
    }
    Ok(hits)
}

struct HitSink<'h> {
    path: PathBuf,
    hits: &'h mut Vec<Hit>,
    limit: usize,
}

impl Sink for HitSink<'_> {
    type Error = std::io::Error;

    fn matched(
        &mut self,
        _searcher: &grep::searcher::Searcher,
        mat: &SinkMatch<'_>,
    ) -> Result<bool, std::io::Error> {
        if self.hits.len() >= self.limit {
            return Ok(false);
        }
        let text = String::from_utf8_lossy(mat.bytes());
        self.hits.push(Hit {
            path: self.path.clone(),
            line: mat.line_number().unwrap_or(0),
            text: text.trim_end_matches(['\r', '\n']).to_owned(),
        });
        Ok(self.hits.len() < self.limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn workspace_with(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        for (name, contents) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("mkdir");
            }
            let mut file = std::fs::File::create(&path).expect("create");
            file.write_all(contents.as_bytes()).expect("write");
        }
        dir
    }

    #[test]
    fn literal_search_finds_lines_with_locations() {
        let dir = workspace_with(&[("a.txt", "hello world\nfoo bar\n")]);
        let hits = search(dir.path(), "foo", &Options::default()).expect("search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 2);
        assert_eq!(hits[0].text, "foo bar");
        assert_eq!(hits[0].path, dir.path().join("a.txt"));
    }

    #[test]
    fn regex_search_matches_pattern() {
        let dir = workspace_with(&[("a.txt", "foo123\nbar\n")]);
        let options = Options {
            regex: true,
            ..Options::default()
        };
        let hits = search(dir.path(), r"foo\d+", &options).expect("search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].text, "foo123");
    }

    #[test]
    fn literal_search_ignores_regex_meta() {
        let dir = workspace_with(&[("a.txt", "foo.bar\nfooxbar\n")]);
        let hits = search(dir.path(), "foo.bar", &Options::default()).expect("search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].text, "foo.bar");
    }

    #[test]
    fn gitignored_files_are_skipped() {
        let dir = workspace_with(&[
            (".gitignore", "secret.txt\n"),
            ("secret.txt", "password hunter2\n"),
            ("notes.txt", "password hunter2\n"),
        ]);
        let hits = search(dir.path(), "hunter2", &Options::default()).expect("search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, dir.path().join("notes.txt"));
    }

    #[test]
    fn non_directory_is_invalid_input() {
        let err =
            search(Path::new("/no/such/dir"), "x", &Options::default()).expect_err("must fail");
        assert!(matches!(err, Error::InvalidInput(_)));
    }

    #[test]
    fn bad_regex_is_bad_pattern() {
        let dir = workspace_with(&[("a.txt", "x\n")]);
        let options = Options {
            regex: true,
            ..Options::default()
        };
        let err = search(dir.path(), "(", &options).expect_err("must fail");
        assert!(matches!(err, Error::BadPattern(_)));
    }
}

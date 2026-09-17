//! Tool-output detection: decides what a blob of tool output *is*.
//!
//! The order of checks is load-bearing and mirrors the RTK pipeline:
//!
//! ```text
//! git-log → git-diff → git-status → build-output → porcelain → grep → find
//!   → tree → ls → search-list → read-numbered → dedup-log → smart-truncate
//! ```
//!
//! Build output is checked **before** the porcelain check on purpose: cargo and
//! npm print `Compiling …` / `added N packages`, which the loose porcelain row
//! pattern would otherwise misread as `git status`.
//!
//! Matchers are hand-written rather than regex-based: every pattern is a fixed
//! prefix or a fixed-width column check, and the crate deliberately keeps its
//! dependency set small enough for an offline build.
//!
//! Only the first [`DETECT_WINDOW`](super::DETECT_WINDOW) characters are
//! inspected, so detection stays cheap on huge blobs.

/// The filter chosen for a blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    GitLog,
    GitDiff,
    GitStatus,
    BuildOutput,
    Grep,
    Find,
    Tree,
    Ls,
    SearchList,
    ReadNumbered,
    DedupLog,
    SmartTruncate,
}

impl FilterKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FilterKind::GitLog => "git-log",
            FilterKind::GitDiff => "git-diff",
            FilterKind::GitStatus => "git-status",
            FilterKind::BuildOutput => "build-output",
            FilterKind::Grep => "grep",
            FilterKind::Find => "find",
            FilterKind::Tree => "tree",
            FilterKind::Ls => "ls",
            FilterKind::SearchList => "search-list",
            FilterKind::ReadNumbered => "read-numbered",
            FilterKind::DedupLog => "dedup-log",
            FilterKind::SmartTruncate => "smart-truncate",
        }
    }

    /// True when the filter drops content rather than only regrouping it, so
    /// the `minimal` level can skip it.
    ///
    /// `smart-truncate` cuts a head and a tail; `dedup-log` collapses repeated
    /// lines, which is lossless in practice but is kept on the lossy side
    /// because a log's repetition count can carry meaning.
    pub fn is_lossy(self) -> bool {
        matches!(self, FilterKind::SmartTruncate | FilterKind::DedupLog)
    }
}

/// Characters git uses as the log graph gutter before `commit`.
const LOG_GUTTER: [char; 5] = ['*', '|', '/', '\\', ' '];

/// `ls -l` permission row: type char then nine permission chars.
const LS_TYPE_CHARS: [char; 7] = ['-', 'd', 'l', 'b', 'c', 'p', 's'];

/// Two-column `git status --porcelain` status chars.
const PORCELAIN_CHARS: [char; 8] = [' ', 'M', 'A', 'D', 'R', 'C', 'U', '?'];

/// Detects the filter for a blob, or `None` when nothing matches.
pub fn detect(text: &str) -> Option<FilterKind> {
    let window: String = text.chars().take(super::DETECT_WINDOW).collect();
    let window = window.as_str();

    if window.lines().any(is_git_log_line) {
        return Some(FilterKind::GitLog);
    }
    if window.lines().any(|line| line.starts_with("diff --git "))
        || window.lines().any(|line| line.starts_with("@@ "))
    {
        return Some(FilterKind::GitDiff);
    }
    if window.lines().any(is_git_status_line) {
        return Some(FilterKind::GitStatus);
    }
    // Before the porcelain check: see the module comment.
    if window.lines().any(is_build_output_line) {
        return Some(FilterKind::BuildOutput);
    }
    if is_mostly_porcelain(window) {
        return Some(FilterKind::GitStatus);
    }
    if is_grep(window) {
        return Some(FilterKind::Grep);
    }
    if is_find(window) {
        return Some(FilterKind::Find);
    }
    if window.contains("├──") || window.contains("└──") || window.contains("│  ") {
        return Some(FilterKind::Tree);
    }
    if window.lines().any(is_ls_row) || window.lines().any(is_ls_total) {
        return Some(FilterKind::Ls);
    }
    if window.trim_start().starts_with("Result of search in '") {
        return Some(FilterKind::SearchList);
    }
    if is_read_numbered(window) {
        return Some(FilterKind::ReadNumbered);
    }
    if has_consecutive_duplicates(window) {
        return Some(FilterKind::DedupLog);
    }
    if text.lines().count() > super::filters::SMART_TRUNCATE_MIN_LINES {
        return Some(FilterKind::SmartTruncate);
    }

    None
}

/// `[*|/\\ ]*commit <7-40 hex>` — a `git log` header, with or without a graph.
fn is_git_log_line(line: &str) -> bool {
    let rest = line.trim_start_matches(|c: char| LOG_GUTTER.contains(&c));
    let Some(hash) = rest.strip_prefix("commit ") else {
        return false;
    };
    let hash = hash.trim();
    (7..=40).contains(&hash.len()) && hash.chars().all(|c| c.is_ascii_hexdigit())
}

/// A line that only `git status` produces.
fn is_git_status_line(line: &str) -> bool {
    [
        "On branch ",
        "nothing to commit",
        "Changes not staged for commit:",
        "Changes to be committed:",
        "Untracked files:",
    ]
    .iter()
    .any(|prefix| line.starts_with(prefix))
}

/// A line that only a package manager or compiler produces.
fn is_build_output_line(line: &str) -> bool {
    const PREFIXES: [&str; 12] = [
        "npm warn",
        "npm error",
        "npm ERR!",
        "yarn warn",
        "yarn error",
        "[ERROR]",
        "BUILD SUCCESS",
        "BUILD FAILED",
        "Successfully installed",
        "Successfully built",
        "ERROR:",
    ];

    let trimmed = line.trim_start();
    PREFIXES.iter().any(|prefix| trimmed.starts_with(prefix))
        || line.starts_with("   Compiling ")
        || line.starts_with("   Downloading ")
        || line.starts_with("Finished ")
        || trimmed.starts_with("added ") && trimmed.contains(" package")
}

/// `git status --porcelain`: two status columns, a space, then a path.
fn is_porcelain_row(line: &str) -> bool {
    let mut chars = line.chars();
    let (Some(first), Some(second), Some(third)) = (chars.next(), chars.next(), chars.next())
    else {
        return false;
    };

    PORCELAIN_CHARS.contains(&first)
        && PORCELAIN_CHARS.contains(&second)
        && third == ' '
        && chars.next().is_some_and(|c| !c.is_whitespace())
}

fn is_mostly_porcelain(window: &str) -> bool {
    let lines: Vec<&str> = non_empty(window).collect();
    if lines.len() < 3 {
        return false;
    }

    let porcelain = lines.iter().filter(|line| is_porcelain_row(line)).count();
    porcelain * 100 / lines.len() >= 60
}

/// `grep -n` output: a line has `path:line:text`.
fn is_grep_row(line: &str) -> bool {
    let mut parts = line.splitn(3, ':');
    let (Some(path), Some(number), Some(rest)) = (parts.next(), parts.next(), parts.next())
    else {
        return false;
    };

    !path.is_empty()
        && !path.contains(' ')
        && !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
        && !rest.is_empty()
}

/// `grep` with line numbers: any of the first five lines has a match row.
fn is_grep(window: &str) -> bool {
    non_empty(window).take(5).any(is_grep_row)
}

/// `find` output: three or more lines that are all path-like.
fn is_find(window: &str) -> bool {
    let lines: Vec<&str> = non_empty(window).collect();
    if lines.len() < 3 {
        return false;
    }

    lines.iter().all(|line| looks_like_path(line))
}

/// A bare relative or absolute path, with no spaces and no prose punctuation.
fn looks_like_path(line: &str) -> bool {
    let line = line.trim();
    if line.is_empty() || line.contains(' ') || line.starts_with('#') || line.ends_with('.') {
        return false;
    }

    line.contains('/') || line.contains('.')
}

/// `ls -l` row: a type char then nine permission chars.
fn is_ls_row(line: &str) -> bool {
    let mut chars = line.chars();
    let Some(kind) = chars.next() else {
        return false;
    };
    if !LS_TYPE_CHARS.contains(&kind) {
        return false;
    }

    chars
        .take(9)
        .all(|c| matches!(c, 'r' | 'w' | 'x' | '-' | 's' | 'S' | 't' | 'T'))
}

/// The `total 42` header `ls -l` prints.
fn is_ls_total(line: &str) -> bool {
    line.strip_prefix("total ")
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

/// A `<digits>|text` or `<digits>: text` line from a numbered file read.
fn is_numbered_row(line: &str) -> bool {
    let trimmed = line.trim_start();
    let digits: String = trimmed.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return false;
    }

    trimmed[digits.len()..]
        .trim_start_matches(['|', ':'])
        .trim_start()
        .starts_with(|c: char| !c.is_whitespace())
}

/// A line-numbered file dump.
fn is_read_numbered(window: &str) -> bool {
    let lines: Vec<&str> = non_empty(window).collect();
    if lines.len() < 5 {
        return false;
    }

    let numbered = lines.iter().filter(|line| is_numbered_row(line)).count();
    // A high ratio is required so prose that happens to contain `12:` stays out.
    numbered * 10 / lines.len() >= 7
}

/// True when the same line repeats back to back, the signature of a log dump.
fn has_consecutive_duplicates(window: &str) -> bool {
    let mut previous: Option<&str> = None;
    for line in non_empty(window) {
        if previous == Some(line) {
            return true;
        }
        previous = Some(line);
    }
    false
}

fn non_empty(text: &str) -> impl Iterator<Item = &str> {
    text.lines().filter(|line| !line.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_build_output_is_not_mistaken_for_git_status() {
        // The loose porcelain pattern would match "   Compiling foo" rows; build
        // output is therefore checked first.
        let text = "   Compiling alnair-router v0.1.0\n\
                    warning: unused import\n\
                    Finished `dev` profile [unoptimized]\n";
        assert_eq!(detect(text), Some(FilterKind::BuildOutput));
    }

    #[test]
    fn npm_output_is_build_output() {
        let text = "npm warn deprecated left-pad@1.0.0\nadded 42 packages in 3s\n";
        assert_eq!(detect(text), Some(FilterKind::BuildOutput));
    }

    #[test]
    fn porcelain_status_is_detected() {
        let text = " M src/main.rs\n?? src/new.rs\nA  src/added.rs\n";
        assert_eq!(detect(text), Some(FilterKind::GitStatus));
    }

    #[test]
    fn human_git_status_is_detected() {
        let text = "On branch main\n\
                    Changes not staged for commit:\n\
                    \tmodified:   src/main.rs\n";
        assert_eq!(detect(text), Some(FilterKind::GitStatus));
    }

    #[test]
    fn diff_and_log_are_detected() {
        assert_eq!(
            detect("diff --git a/src/main.rs b/src/main.rs\n@@ -1,3 +1,4 @@\n"),
            Some(FilterKind::GitDiff)
        );
        assert_eq!(
            detect("* commit 0123456789abcdef0123456789abcdef01234567\nAuthor: A\n"),
            Some(FilterKind::GitLog)
        );
    }

    #[test]
    fn grep_output_is_detected() {
        let text = "src/main.rs:12:fn main() {\nsrc/lib.rs:44:pub mod x;\n";
        assert_eq!(detect(text), Some(FilterKind::Grep));
    }

    #[test]
    fn find_output_is_detected() {
        let text = "./src/main.rs\n./src/lib.rs\n./src/config.rs\n";
        assert_eq!(detect(text), Some(FilterKind::Find));
    }

    #[test]
    fn tree_and_ls_are_detected() {
        assert_eq!(
            detect("src/\n├── main.rs\n└── lib.rs\n"),
            Some(FilterKind::Tree)
        );
        assert_eq!(
            detect("total 24\ndrwxr-xr-x  4 user  staff  128 Jan  1 00:00 src\n"),
            Some(FilterKind::Ls)
        );
    }

    #[test]
    fn repeated_log_lines_are_deduped() {
        let text = "connecting...\nconnecting...\nconnecting...\n";
        assert_eq!(detect(text), Some(FilterKind::DedupLog));
    }

    #[test]
    fn a_long_undifferentiated_blob_falls_back_to_smart_truncate() {
        let text = (0..300)
            .map(|index| format!("unique line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(detect(text), Some(FilterKind::SmartTruncate));
    }

    #[test]
    fn prose_is_not_detected() {
        assert_eq!(
            detect("Here is a summary of what I changed.\nLooks good.\n"),
            None
        );
        assert_eq!(detect(""), None);
    }
}

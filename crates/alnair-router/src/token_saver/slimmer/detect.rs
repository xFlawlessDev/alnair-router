//! Tool-output detection: decides what a blob of tool output *is*.
//!
//! The order of checks is load-bearing and mirrors the RTK pipeline:
//!
//! ```text
//! git-log → git-diff → git-status → build-output → porcelain → grep → find
//!   → tree → ls → search-list → read-numbered → dedup-log → prose → smart-truncate
//! ```
//!
//! Build output is checked **before** the porcelain check on purpose: cargo and
//! npm print `Compiling …` / `added N packages`, which the loose porcelain row
//! pattern would otherwise misread as `git status`.
//!
//! Prose is checked last, just before the truncate fallback, so every shape a
//! tool actually produces keeps its own filter. The truncate fallback drops the
//! middle of a blob, which is data loss for a document read as a whole.
//!
//! Every shape detector wants a share of matching lines, not a single hit:
//! prose about a `git status` block, a quoted `---`, or one `ERROR:` line in a
//! guide must not hand the whole blob to a filter that rewrites it.
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

/// Fewest non-empty lines that can be a log dump worth de-duplicating.
const DEDUP_MIN_LINES: usize = 12;

/// Non-empty lines the grep detector reads before deciding.
const GREP_SAMPLE_LINES: usize = 12;

/// Too few lines to tell a document from a log dump.
const PROSE_MIN_LINES: usize = 20;

/// Lines read when judging whether a blob is prose.
const PROSE_SAMPLE_LINES: usize = 100;

/// Words in a line before it counts as a sentence rather than a label.
const PROSE_MIN_WORDS: usize = 4;

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
    if is_git_status(window) {
        return Some(FilterKind::GitStatus);
    }
    // Before the porcelain check: see the module comment.
    if is_build_output(window) {
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
    if is_tree(window) {
        return Some(FilterKind::Tree);
    }
    if is_mostly_ls(window) || window.lines().any(is_ls_total) {
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
    // Above the truncate floor, prose is the one thing truncation must not cut:
    // a document is read as a whole, and losing its middle is data loss, not a
    // compression trade-off.
    if looks_like_prose(text) {
        return None;
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

/// A human `git status` header. Two headers together are also accepted, because
/// documented workflows quote a whole `git status` block — `On branch` followed
/// by `nothing to commit` — and a single quoted line would otherwise be enough.
fn is_git_status(window: &str) -> bool {
    let lines: Vec<&str> = non_empty(window).collect();
    let hits = lines.iter().filter(|line| is_git_status_line(line)).count();
    hits >= 2 || (hits == 1 && lines.len() == 1)
}

/// Prefixes that only a package manager or compiler prints, never prose.
const BUILD_ONLY_PREFIXES: [&str; 6] = [
    "npm warn",
    "npm error",
    "npm ERR!",
    "yarn warn",
    "yarn error",
    "Successfully installed",
];

/// Prefixes that prose also uses when it *describes* a build. They count only
/// next to a command line, which real build output always has nearby.
const BUILD_COMMON_PREFIXES: [&str; 6] = [
    "[ERROR]",
    "BUILD SUCCESS",
    "BUILD FAILED",
    "ERROR:",
    "Finished ",
    "Successfully built",
];

fn is_build_output_line(line: &str) -> bool {
    let trimmed = line.trim_start();

    if BUILD_ONLY_PREFIXES
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
    {
        return true;
    }

    line.starts_with("   Compiling ")
        || line.starts_with("   Downloading ")
        || trimmed.starts_with("added ") && trimmed.contains(" package")
}

/// True when the trimmed line opens with an executable and its arguments.
fn is_build_command_line(line: &str) -> bool {
    const COMMANDS: [&str; 12] = [
        "cargo ", "npm ", "pnpm ", "yarn ", "make", "mvn ", "gradle", "dotnet ", "go build",
        "go test", "tsc", "pip ",
    ];
    let trimmed = line.trim();
    COMMANDS.iter().any(|command| trimmed.starts_with(command))
}

/// Build output is a header line plus its body. A single `ERROR:` or
/// `BUILD FAILED` line in prose describes build output without being it, so the
/// detector wants either an unambiguous prefix, or a common prefix next to a
/// command line.
fn is_build_output(window: &str) -> bool {
    let lines: Vec<&str> = non_empty(window).collect();
    if lines.is_empty() {
        return false;
    }
    if lines.iter().any(|line| is_build_output_line(line)) {
        return true;
    }
    if !lines.iter().any(|line| is_build_command_line(line)) {
        return false;
    }
    lines.iter().any(|line| {
        let trimmed = line.trim_start();
        BUILD_COMMON_PREFIXES
            .iter()
            .any(|prefix| trimmed.starts_with(prefix))
    })
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
    let (Some(path), Some(number), Some(rest)) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };

    !path.is_empty()
        && !path.contains(' ')
        && !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
        && !rest.is_empty()
}

/// `grep` with line numbers: a third of the first twelve non-empty lines are
/// match rows, with at least two of them.
///
/// Prose that names one file and a line number — `README.md:42:` — is common in
/// documentation; a real grep result is mostly rows.
fn is_grep(window: &str) -> bool {
    let lines: Vec<&str> = non_empty(window).take(GREP_SAMPLE_LINES).collect();
    let rows = lines.iter().filter(|line| is_grep_row(line)).count();
    rows >= 2 && rows * 3 >= lines.len()
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

/// `ls -l` row: type char, nine permission chars, then the column separator
/// and at least the link count, owner, group, size, date, and name fields.
/// Requiring the tail is what keeps a Markdown `---` rule and short dash runs
/// out of the detector.
fn is_ls_row(line: &str) -> bool {
    let mut chars = line.chars();
    let Some(kind) = chars.next() else {
        return false;
    };
    if !LS_TYPE_CHARS.contains(&kind) {
        return false;
    }

    let perms: String = chars.by_ref().take(9).collect();
    if perms.chars().count() != 9
        || !perms
            .chars()
            .all(|c| matches!(c, 'r' | 'w' | 'x' | '-' | 's' | 'S' | 't' | 'T'))
    {
        return false;
    }

    // macOS appends one marker (`@` for xattrs, `+` for ACLs, `.` for SELinux)
    // before the separator; GNU and BSD `ls` often print none.
    let mut next = chars.next();
    if matches!(next, Some('@' | '+' | '.')) {
        next = chars.next();
    }
    // Real rows continue with a space, a link count, and the remaining fields.
    if !next.is_some_and(char::is_whitespace) {
        return false;
    }
    line.split_whitespace().count() >= 7
}

/// A listing needs most of its lines to be `ls -l` rows, so one stray
/// permission-looking line in a document does not claim the whole blob.
fn is_mostly_ls(window: &str) -> bool {
    let lines: Vec<&str> = non_empty(window).collect();
    if lines.len() < 3 {
        return false;
    }

    let rows = lines.iter().filter(|line| is_ls_row(line)).count();
    rows * 100 / lines.len() >= 60
}

/// `tree` output: three or more lines carry the box-drawing gutter. A single
/// glyph in prose is not a tree.
fn is_tree(window: &str) -> bool {
    non_empty(window)
        .filter(|line| line.contains("├──") || line.contains("└──") || line.contains("│  "))
        .count()
        >= 3
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
///
/// Repeating prose (a documentation example quoted twice) has no bulk to give
/// up, so a repeat only counts when the blob is big enough to be a log: shorter
/// than the smart-truncate floor, at least twelve lines, and one repeat for
/// every four non-empty lines.
fn has_consecutive_duplicates(window: &str) -> bool {
    let lines: Vec<&str> = non_empty(window).collect();
    if lines.len() < DEDUP_MIN_LINES || lines.len() >= super::filters::SMART_TRUNCATE_MIN_LINES {
        return false;
    }

    let repeats = lines.windows(2).filter(|pair| pair[0] == pair[1]).count();
    repeats >= 3 && repeats * 4 >= lines.len()
}

/// True when a blob reads as prose or Markdown rather than machine output.
///
/// The question is only asked when nothing else matched, so a real listing or
/// log that happens to contain a sentence still takes its own filter first.
/// Both tests describe writing, not tooling: a line that starts with a word and
/// runs on, and a share of Markdown markers high enough that the document is
/// structured by them.
fn looks_like_prose(text: &str) -> bool {
    let lines: Vec<&str> = non_empty(text).collect();
    if lines.len() < PROSE_MIN_LINES {
        return false;
    }

    let sample = &lines[..lines.len().min(PROSE_SAMPLE_LINES)];
    let sentences = sample.iter().filter(|line| is_sentence_line(line)).count();
    let markers = sample
        .iter()
        .filter(|line| is_markdown_marker_line(line))
        .count();

    // Markdown markers alone settle it: tool output does not write headings and
    // rules. Sentences alone are weaker evidence because some logs do read as
    // prose, so they also need the blob to carry no machine-shaped lines.
    if markers * 4 >= sample.len() {
        return true;
    }
    if sentences * 2 < sample.len() {
        return false;
    }

    let machine = sample
        .iter()
        .filter(|line| is_machine_shaped_line(line))
        .count();
    machine * 20 <= sample.len()
}

/// A line only a tool writes: an indent, a status column, a bracket, a
/// timestamp, a path, or a `key: value` pair. Prose does not open this way.
fn is_machine_shaped_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(first) = trimmed.chars().next() else {
        return false;
    };
    if line.starts_with(' ') || line.starts_with('\t') {
        return true;
    }
    if !first.is_alphanumeric() {
        return true;
    }

    let head: String = trimmed.chars().take(10).collect();
    let digits = head.chars().filter(|c| c.is_ascii_digit()).count();
    digits * 4 >= head.chars().count() || trimmed.contains('/') || trimmed.contains(": ")
}

/// A line that opens with a word and runs on with spaces and punctuation.
///
/// Log lines, code, data dumps and listings all start with something else — a
/// bracket, an indent, a digit, a path, a status column — so the first two
/// characters are enough to tell the shapes apart.
fn is_sentence_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    let mut chars = trimmed.chars();
    let (Some(first), Some(second)) = (chars.next(), chars.next()) else {
        return false;
    };
    if first.is_uppercase() && (second.is_lowercase() || second.is_whitespace()) {
        return trimmed.split_whitespace().count() >= PROSE_MIN_WORDS;
    }
    false
}

/// A Markdown construct that only appears in a written document.
fn is_markdown_marker_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('#')
        || trimmed.starts_with("- ")
        || trimmed.starts_with("* ")
        || trimmed.starts_with("> ")
        || trimmed.starts_with("```")
        || trimmed.starts_with("|")
        || is_markdown_rule(trimmed)
}

/// `---`, `***`, or `___` repeated — a horizontal rule.
fn is_markdown_rule(line: &str) -> bool {
    let line = line.trim();
    [('-', 3), ('*', 3), ('_', 3)]
        .iter()
        .any(|(glyph, min)| line.chars().all(|c| c == *glyph) && line.chars().count() >= *min)
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
            detect("src/\n├── main.rs\n├── config.rs\n└── lib.rs\n"),
            Some(FilterKind::Tree)
        );
        assert_eq!(
            detect("total 24\ndrwxr-xr-x  4 user  staff  128 Jan  1 00:00 src\n"),
            Some(FilterKind::Ls)
        );
    }

    #[test]
    fn repeated_log_lines_are_deduped() {
        let text = "boot\n".to_string() + &"connecting...\n".repeat(12) + "ready\n";
        assert_eq!(detect(&text), Some(FilterKind::DedupLog));
    }

    #[test]
    fn a_short_repeat_is_not_a_log_dump() {
        // A documentation example quoted twice has no bulk to de-duplicate.
        let text = "npm install\n\nnpm install\n";
        assert_eq!(detect(text), None);
    }

    #[test]
    fn a_single_tree_glyph_in_prose_is_not_a_tree() {
        let text = "The output looks like:\n├── main.rs\nThat is all.\n";
        assert_eq!(detect(text), None);
    }

    #[test]
    fn a_quoted_git_status_block_needs_its_second_line() {
        let alone = "Run `git status` first.\nNothing to commit here.\n";
        assert_eq!(detect(alone), None);
        let block = "On branch main\nnothing to commit, working tree clean\n";
        assert_eq!(detect(block), Some(FilterKind::GitStatus));
    }

    #[test]
    fn prose_describing_build_output_is_not_build_output() {
        let prose = "When the build fails, the last line reads:\n\n\
                     ERROR: cannot find module 'left-pad'\n";
        assert_eq!(detect(prose), None);

        let real = "cargo build\n\
                    [ERROR] /tmp/app/src/main.rs:[12,4] cannot find symbol\n\
                    BUILD FAILED\n";
        assert_eq!(detect(real), Some(FilterKind::BuildOutput));
    }

    #[test]
    fn a_long_undifferentiated_blob_falls_back_to_smart_truncate() {
        let text = (0..300)
            .map(|index| format!("unique line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(detect(&text), Some(FilterKind::SmartTruncate));
    }

    #[test]
    fn prose_is_not_detected() {
        assert_eq!(
            detect("Here is a summary of what I changed.\nLooks good.\n"),
            None
        );
        assert_eq!(detect(""), None);
    }

    #[test]
    fn markdown_with_horizontal_rule_is_not_an_ls_listing() {
        let text = "# Title\n\nIntro paragraph here.\n\n---\n\n## Section\n\nBody text.\n";
        assert_eq!(detect(text), None);
    }

    #[test]
    fn markdown_horizontal_rules_do_not_match_the_ls_row_detector() {
        for rule in ["---", "--", "-", "-----", "---------"] {
            assert!(!is_ls_row(rule), "'{rule}' must not be an ls row");
        }
    }

    #[test]
    fn a_single_ls_like_line_in_prose_is_not_a_listing() {
        let text = "Intro.\ndrwxr-xr-x  4 user  staff  128 Jan  1 00:00 src\nMore prose.\n";
        assert_eq!(detect(text), None);
    }

    #[test]
    fn real_ls_listing_still_matches_the_row_detector() {
        assert!(is_ls_row("drwxr-xr-x  4 user  staff  128 Jan  1 00:00 src"));
        assert!(is_ls_row(
            "-rw-r--r--@ 1 user  staff  1024 Jan  1 00:00 Cargo.toml"
        ));
    }

    /// A guide long enough to clear the truncate floor.
    fn long_guide() -> String {
        let mut doc =
            String::from("# Release guide\n\nThis guide walks through a release.\n\n---\n\n");
        for index in 0..120 {
            doc.push_str(&format!(
                "## Step {index}\n\nThe operator picks the next candidate carefully.\n\n"
            ));
        }
        doc
    }

    #[test]
    fn a_long_markdown_document_is_left_alone() {
        let doc = long_guide();
        assert!(doc.lines().count() > super::super::filters::SMART_TRUNCATE_MIN_LINES);
        assert_eq!(detect(&doc), None);
    }

    #[test]
    fn a_long_unstructured_document_of_sentences_is_left_alone() {
        let doc = (0..300)
            .map(|index| format!("Sentence {index} explains one more detail of the change."))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(detect(&doc), None);
    }

    #[test]
    fn a_long_log_without_markdown_still_truncates() {
        let log = (0..300)
            .map(|index| format!("[info] worker-{index:x} finished the current unit"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(detect(&log), Some(FilterKind::SmartTruncate));
    }
}

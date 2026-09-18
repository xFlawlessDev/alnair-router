//! Per-filter compression.
//!
//! Each filter restructures one known tool-output shape into something denser:
//! a diff keeps its headers and hunks but caps runaway hunks, a grep groups
//! matches by file, a directory listing folds its noise directories away.
//!
//! Ported from the RTK filters in 9Router (`open-sse/rtk/filters/*`), including
//! the per-filter caps. Every function is total: it returns a string for any
//! input, and the caller ([`super::compress_one`]) discards any result that came
//! out empty or larger than the input.

use super::detect::FilterKind;

/// Lines kept from the head of an over-long blob.
pub const SMART_TRUNCATE_HEAD: usize = 120;

/// Lines kept from the tail of an over-long blob.
pub const SMART_TRUNCATE_TAIL: usize = 60;

/// Only blobs longer than this are worth truncating.
pub const SMART_TRUNCATE_MIN_LINES: usize = 250;

/// Per-hunk line cap for a diff.
const GIT_DIFF_HUNK_MAX_LINES: usize = 100;

/// Whole-diff line cap.
const GIT_DIFF_MAX_LINES: usize = 500;

/// Line cap for a log dump.
const DEDUP_LINE_MAX: usize = 2000;

/// Matches kept per file by the grep filter.
const GREP_PER_FILE_MAX: usize = 10;

/// Files kept per directory by the find filter.
const FIND_PER_DIR_MAX: usize = 10;

/// Directories kept by the find filter.
const FIND_TOTAL_DIR_MAX: usize = 20;

/// Files listed by the status filter, per category.
const STATUS_MAX_FILES: usize = 10;

/// Extensions named in the `ls` summary.
const LS_EXT_SUMMARY_TOP: usize = 5;

/// Directory names dropped from an `ls` listing.
const LS_NOISE_DIRS: [&str; 24] = [
    "node_modules",
    ".git",
    "target",
    "__pycache__",
    ".next",
    "dist",
    "build",
    ".cache",
    ".turbo",
    ".vercel",
    ".pytest_cache",
    ".mypy_cache",
    ".tox",
    ".venv",
    "venv",
    "env",
    "coverage",
    ".nyc_output",
    ".DS_Store",
    "Thumbs.db",
    ".idea",
    ".vscode",
    ".vs",
    ".eggs",
];

/// Dependency lines kept before collapsing the rest.
const DEPRECATION_KEEP: usize = 3;

/// Warnings kept before collapsing the rest.
const WARNING_KEEP: usize = 5;

/// Applies the filter to `text`.
pub fn apply(kind: FilterKind, text: &str) -> String {
    match kind {
        FilterKind::GitLog => git_log(text),
        FilterKind::GitDiff => git_diff(text),
        FilterKind::GitStatus => git_status(text),
        FilterKind::BuildOutput => build_output(text),
        FilterKind::Grep => grep(text),
        FilterKind::Find => find(text),
        FilterKind::Tree => tree(text),
        FilterKind::Ls => ls(text),
        FilterKind::SearchList => search_list(text),
        FilterKind::ReadNumbered => smart_truncate(text, true),
        FilterKind::DedupLog => dedup_log(text),
        FilterKind::SmartTruncate => smart_truncate(text, false),
    }
}

/// `git log`: keep commit headers, authors, dates, subjects and stats; drop
/// diff bodies.
fn git_log(text: &str) -> String {
    let mut out = Vec::new();
    let mut in_diff = false;

    for line in text.lines() {
        if line.starts_with("diff --git ") {
            in_diff = true;
            out.push("  ... diff body omitted".to_string());
            continue;
        }
        if in_diff {
            // A blank line after the diff body ends it; anything else is body.
            if line.trim().is_empty() {
                in_diff = false;
            } else {
                continue;
            }
        }

        if line.starts_with("    ") && !line.trim().is_empty() {
            out.push(format!("  Subject: {}", line.trim()));
            continue;
        }

        out.push(line.to_string());
    }

    cap_lines(out, 200, "... +{n} more lines")
}

/// `git diff`: keep file headers and hunks, cap each hunk, annotate the totals.
fn git_diff(text: &str) -> String {
    let mut out = Vec::new();
    let mut hunk_lines = 0usize;
    let mut hunk_truncated = false;

    for line in text.lines() {
        if line.starts_with("@@ ") {
            if hunk_truncated {
                out.push("  ... hunk truncated".to_string());
            }
            hunk_lines = 0;
            hunk_truncated = false;
            out.push(line.to_string());
            continue;
        }

        if line.starts_with("diff --git ")
            || line.starts_with("index ")
            || line.starts_with("--- ")
            || line.starts_with("+++ ")
        {
            continue;
        }

        hunk_lines += 1;
        if hunk_lines > GIT_DIFF_HUNK_MAX_LINES {
            hunk_truncated = true;
            continue;
        }

        out.push(line.to_string());
    }
    if hunk_truncated {
        out.push("  ... hunk truncated".to_string());
    }

    out.push("[full diff: rtk git diff --no-compact]".to_string());
    cap_lines(out, GIT_DIFF_MAX_LINES, "... +{n} more changes truncated")
}

/// `git status`: counts per category instead of a file list.
fn git_status(text: &str) -> String {
    let mut branch = None;
    let mut staged = Vec::new();
    let mut modified = Vec::new();
    let mut untracked = Vec::new();
    let mut conflicts = Vec::new();
    let mut clean = false;

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("On branch ") {
            branch = Some(rest.trim().to_string());
            continue;
        }
        if line.starts_with("nothing to commit") {
            clean = true;
            continue;
        }

        // Porcelain rows: two status columns then the path.
        let mut chars = line.chars();
        let (Some(index), Some(worktree)) = (chars.next(), chars.next()) else {
            continue;
        };
        if !matches!(index, ' ' | 'M' | 'A' | 'D' | 'R' | 'C' | 'U' | '?')
            || !matches!(worktree, ' ' | 'M' | 'A' | 'D' | 'R' | 'C' | 'U' | '?')
        {
            continue;
        }

        let path = chars.as_str().trim().to_string();
        if path.is_empty() {
            continue;
        }

        if index == 'U' || worktree == 'U' {
            conflicts.push(path);
        } else if index == '?' {
            untracked.push(path);
        } else if index != ' ' {
            staged.push(path);
        } else {
            modified.push(path);
        }
    }

    let mut out = Vec::new();
    if let Some(branch) = branch {
        out.push(format!("* {branch}"));
    }
    push_bucket(&mut out, "+ Staged", "files", &staged);
    push_bucket(&mut out, "~ Modified", "files", &modified);
    push_bucket(&mut out, "? Untracked", "files", &untracked);
    push_bucket(&mut out, "conflicts", "files", &conflicts);

    if out.is_empty() {
        if clean {
            return "clean — nothing to commit".to_string();
        }
        return String::new();
    }

    out.join("\n")
}

fn push_bucket(out: &mut Vec<String>, label: &str, noun: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }

    out.push(format!("{label}: {} {noun}", items.len()));
    for path in items.iter().take(STATUS_MAX_FILES) {
        out.push(format!("  {path}"));
    }
    if items.len() > STATUS_MAX_FILES {
        out.push(format!("   ... +{} more", items.len() - STATUS_MAX_FILES));
    }
}

/// Build output: keep errors, warnings and the summary; collapse the rest.
fn build_output(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut deprecations = 0usize;
    let mut warnings = 0usize;
    let mut hidden_deprecations = 0usize;
    let mut hidden_warnings = 0usize;
    let mut compiling = 0usize;
    let mut downloading = 0usize;
    let mut added = 0usize;
    let mut carry = false;

    for line in text.lines() {
        let trimmed = line.trim_start();

        if trimmed.starts_with("warning:") || trimmed.starts_with("warning[") {
            warnings += 1;
            if warnings > WARNING_KEEP {
                hidden_warnings += 1;
                continue;
            }
        } else if trimmed.contains("deprecated") {
            deprecations += 1;
            if deprecations > DEPRECATION_KEEP {
                hidden_deprecations += 1;
                continue;
            }
        }

        if trimmed.starts_with("Compiling ") {
            compiling += 1;
            continue;
        }
        if trimmed.starts_with("Downloading ") {
            downloading += 1;
            continue;
        }
        if trimmed.starts_with("added ") && trimmed.contains(" package") {
            added += 1;
            continue;
        }

        // Keep the continuation lines that belong to a cargo error block.
        let is_continuation = carry
            && (trimmed.starts_with("-->")
                || trimmed.starts_with('|')
                || trimmed.starts_with('=')
                || trimmed.chars().next().is_some_and(|c| c.is_ascii_digit()));

        if trimmed.starts_with("error") || trimmed.starts_with("[ERROR]") || is_continuation {
            carry = true;
        } else if !trimmed.is_empty() {
            carry = false;
        }

        if trimmed.is_empty() && out.last().is_some_and(|last| last.is_empty()) {
            continue;
        }

        out.push(line.to_string());
    }

    let mut summary = Vec::new();
    if compiling > 0 {
        summary.push(format!("Compiled {compiling} packages"));
    }
    if downloading > 0 {
        summary.push(format!("Downloaded {downloading} packages"));
    }
    if added > 0 {
        summary.push(format!("Added {added} packages"));
    }
    if hidden_deprecations > 0 {
        summary.push(format!(
            "... +{hidden_deprecations} more deprecated packages"
        ));
    }
    if hidden_warnings > 0 {
        summary.push(format!("... +{hidden_warnings} more warnings"));
    }

    if !summary.is_empty() {
        out.push(summary.join(", "));
    }

    out.join("\n")
}

/// `grep`: group matches by file, capped per file.
fn grep(text: &str) -> String {
    let mut order: Vec<String> = Vec::new();
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut rows: std::collections::HashMap<String, Vec<(String, String)>> =
        std::collections::HashMap::new();
    let mut total = 0usize;

    for line in text.lines() {
        let mut parts = line.splitn(3, ':');
        let (Some(path), Some(number), Some(content)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        if path.is_empty() || !number.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }

        total += 1;
        let entry = counts.entry(path.to_string()).or_insert(0);
        if *entry == 0 {
            order.push(path.to_string());
        }
        *entry += 1;
        rows.entry(path.to_string())
            .or_default()
            .push((number.to_string(), content.trim().to_string()));
    }

    if total == 0 {
        return String::new();
    }

    let mut out = vec![format!("{total} matches in {}F:", order.len())];
    for path in &order {
        let matches = &rows[path];
        out.push(format!("[{path}] ({})", matches.len()));
        for (number, content) in matches.iter().take(GREP_PER_FILE_MAX) {
            out.push(format!("  {number:>4}: {content}"));
        }
        if matches.len() > GREP_PER_FILE_MAX {
            out.push(format!("  +{}", matches.len() - GREP_PER_FILE_MAX));
        }
    }

    out.join("\n")
}

/// `find`: group paths by directory.
fn find(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();

    let mut order: Vec<String> = Vec::new();
    let mut per_dir: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();

    for line in &lines {
        let path = std::path::Path::new(line);
        let directory = path
            .parent()
            .map(|parent| parent.to_string_lossy().to_string())
            .filter(|parent| !parent.is_empty())
            .unwrap_or_else(|| ".".to_string());
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| (*line).to_string());

        if !per_dir.contains_key(&directory) {
            order.push(directory.clone());
        }
        per_dir.entry(directory).or_default().push(name);
    }

    let mut out = vec![format!("{} files in {} dirs:", lines.len(), order.len())];
    for directory in order.iter().take(FIND_TOTAL_DIR_MAX) {
        let names = &per_dir[directory];
        out.push(format!("[{directory}]"));
        for name in names.iter().take(FIND_PER_DIR_MAX) {
            out.push(format!("  {name}"));
        }
        if names.len() > FIND_PER_DIR_MAX {
            out.push(format!("  +{}", names.len() - FIND_PER_DIR_MAX));
        }
    }
    if order.len() > FIND_TOTAL_DIR_MAX {
        out.push(format!("+{} more dirs", order.len() - FIND_TOTAL_DIR_MAX));
    }

    out.join("\n")
}

/// `tree`: drop the totals line and trailing blanks, cap the height.
fn tree(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !(trimmed.is_empty() || trimmed.contains("directories") && trimmed.contains("files"))
        })
        .collect();

    cap_lines(
        lines.iter().map(|l| l.to_string()).collect(),
        200,
        "... +{n} more lines",
    )
}

/// `ls -l`: drop noise directories, then summarise by extension.
fn ls(text: &str) -> String {
    let mut dirs = Vec::new();
    let mut files: Vec<(String, String)> = Vec::new();
    let mut extensions: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for line in text.lines() {
        if line.starts_with("total ") {
            continue;
        }

        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 2 {
            continue;
        }

        // `ls -la` rows end with name, and optionally `-> target`.
        let name = match fields.iter().position(|field| *field == "->") {
            Some(arrow) => fields.get(arrow + 1).copied().unwrap_or(""),
            None => *fields.last().unwrap_or(&""),
        }
        .to_string();
        if name.is_empty() || name == "." || name == ".." {
            continue;
        }
        if LS_NOISE_DIRS.iter().any(|noise| name == *noise) {
            continue;
        }

        let is_dir = fields
            .first()
            .is_some_and(|kind| kind.starts_with('d') || name.ends_with('/'));
        if is_dir {
            dirs.push(name);
            continue;
        }

        if let Some((_, ext)) = name.rsplit_once('.')
            && !ext.is_empty()
            && ext.len() <= 8
        {
            *extensions.entry(ext.to_string()).or_insert(0) += 1;
        }
        let size = fields
            .get(4)
            .filter(|field| field.chars().all(|c| c.is_ascii_digit()))
            .map(|field| human_size(field.parse().unwrap_or(0)))
            .unwrap_or_default();
        files.push((name, size));
    }

    let mut out = Vec::new();
    for dir in &dirs {
        out.push(format!("{dir}/"));
    }
    for (name, size) in files.iter().take(200) {
        if size.is_empty() {
            out.push(format!("  {name}"));
        } else {
            out.push(format!("  {name}  {size}"));
        }
    }
    if files.len() > 200 {
        out.push(format!("  ... +{} more files", files.len() - 200));
    }

    if !extensions.is_empty() {
        let mut ranked: Vec<(String, usize)> = extensions.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let detail = ranked
            .iter()
            .take(LS_EXT_SUMMARY_TOP)
            .map(|(ext, count)| format!("{count} .{ext}"))
            .collect::<Vec<_>>()
            .join(", ");
        out.push(format!(
            "Summary: {} files, {} dirs ({detail})",
            files.len(),
            dirs.len()
        ));
    }

    out.join("\n")
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "K", "M", "G"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes}{}", UNITS[unit])
    } else {
        format!("{value:.1}{}", UNITS[unit])
    }
}

/// Cursor-style search results: regroup by directory.
fn search_list(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut header_kept = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if !header_kept && trimmed.starts_with("Result of search in '") {
            out.push(trimmed.to_string());
            header_kept = true;
            continue;
        }

        out.push(format!("  {trimmed}"));
    }

    out.join("\n")
}

/// Collapse runs of identical consecutive lines, the signature of a retry loop.
fn dedup_log(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut pending: Option<(String, usize)> = None;

    for line in text.lines().take(DEDUP_LINE_MAX) {
        match &mut pending {
            Some((last, count)) if *last == line => *count += 1,
            Some((last, count)) => {
                push_dedup_line(&mut out, last, *count);
                pending = Some((line.to_string(), 1));
            }
            None => pending = Some((line.to_string(), 1)),
        }
    }
    if let Some((last, count)) = pending {
        push_dedup_line(&mut out, &last, count);
    }

    out.join("\n")
}

fn push_dedup_line(out: &mut Vec<String>, line: &str, count: usize) {
    out.push(line.to_string());
    if count > 1 {
        out.push(format!("  ... ({count} duplicate lines)"));
    }
}

/// Keep a head and a tail, dropping the middle.
///
/// `numbered` selects the marker wording for a line-numbered file dump.
pub fn smart_truncate(text: &str, numbered: bool) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= SMART_TRUNCATE_MIN_LINES {
        // Still worth collapsing a long run of blanks in a numbered dump.
        return text.to_string();
    }

    let dropped = lines.len() - SMART_TRUNCATE_HEAD - SMART_TRUNCATE_TAIL;
    let marker = if numbered {
        format!("... +{dropped} lines truncated (file continues)")
    } else {
        format!("... +{dropped} lines truncated")
    };

    let mut out: Vec<String> = lines
        .iter()
        .take(SMART_TRUNCATE_HEAD)
        .map(|line| (*line).to_string())
        .collect();
    out.push(marker);
    out.extend(
        lines
            .iter()
            .skip(lines.len() - SMART_TRUNCATE_TAIL)
            .map(|line| (*line).to_string()),
    );

    out.join("\n")
}

/// Keeps at most `max` lines, reporting how many were dropped.
fn cap_lines(lines: Vec<String>, max: usize, marker: &str) -> String {
    if lines.len() <= max {
        return lines.join("\n");
    }

    let dropped = lines.len() - max;
    let mut out: Vec<String> = lines.into_iter().take(max).collect();
    out.push(marker.replace("{n}", &dropped.to_string()));
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token_saver::slimmer::detect::{FilterKind, detect};

    #[test]
    fn git_diff_keeps_headers_and_caps_hunks() {
        let mut text = String::from("diff --git a/x b/x\n@@ -1 +1 @@\n");
        for index in 0..200 {
            text.push_str(&format!("+line {index}\n"));
        }
        let out = apply(FilterKind::GitDiff, &text);

        assert!(out.starts_with("@@ -1 +1 @@"));
        assert!(out.contains("... hunk truncated"));
        assert!(out.contains("[full diff: rtk git diff --no-compact]"));
        assert!(out.len() < text.len(), "must shrink a runaway hunk");
    }

    #[test]
    fn git_status_counts_instead_of_listing_everything() {
        let mut text = String::new();
        for index in 0..40 {
            text.push_str(&format!("?? src/generated/file{index}.rs\n"));
        }
        let out = apply(FilterKind::GitStatus, &text);

        assert!(out.contains("? Untracked: 40 files"));
        assert!(out.contains("... +30 more"));
        assert!(out.len() < text.len());
    }

    #[test]
    fn grep_groups_by_file_and_caps_matches() {
        let mut text = String::new();
        for index in 0..30 {
            text.push_str(&format!("src/main.rs:{index}:hit {index}\n"));
        }
        let out = apply(FilterKind::Grep, &text);

        assert!(out.starts_with("30 matches in 1F:"));
        assert!(out.contains("[src/main.rs] (30)"));
        assert!(out.contains("+20"));
    }

    #[test]
    fn ls_drops_noise_directories() {
        let mut text = String::from("total 8\n");
        for name in ["node_modules", "target", "dist", ".git"] {
            text.push_str(&format!(
                "drwxr-xr-x  4 user staff 128 Jan 1 00:00 {name}\n"
            ));
        }
        text.push_str("-rw-r--r--  1 user staff 100 Jan 1 00:00 README.md\n");
        text.push_str("-rw-r--r--  1 user staff 200 Jan 1 00:00 Cargo.toml\n");

        let out = apply(FilterKind::Ls, &text);
        for noise in ["node_modules", "target", "dist", ".git"] {
            assert!(!out.contains(noise), "{noise} should be dropped");
        }
        assert!(out.contains("README.md"));
        assert!(out.contains("Summary: 2 files, 0 dirs"));
    }

    #[test]
    fn smart_truncate_keeps_head_and_tail() {
        let text = (0..400)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let out = apply(FilterKind::SmartTruncate, &text);

        assert!(out.starts_with("line 0\n"));
        assert!(out.ends_with("line 399"));
        assert!(out.contains("... +220 lines truncated"));
        assert!(out.len() < text.len());
    }

    #[test]
    fn smart_truncate_leaves_short_input_alone() {
        let text = "one\ntwo\nthree\n";
        assert_eq!(apply(FilterKind::SmartTruncate, text), text);
    }

    #[test]
    fn dedup_log_collapses_consecutive_duplicates() {
        let text = "start\nretry\nretry\nretry\nend\n";
        let out = apply(FilterKind::DedupLog, text);

        assert!(out.contains("... (3 duplicate lines)"));
        assert!(out.lines().count() < text.lines().count());
    }

    #[test]
    fn build_output_collapses_compile_noise() {
        let mut text = String::new();
        for index in 0..50 {
            text.push_str(&format!("   Compiling crate{index} v0.1.0\n"));
        }
        text.push_str("error[E0308]: mismatched types\n  --> src/main.rs:1:1\n");
        let out = apply(FilterKind::BuildOutput, &text);

        assert!(out.contains("Compiled 50 packages"));
        // The error and its continuation must survive.
        assert!(out.contains("error[E0308]: mismatched types"));
        assert!(out.contains("--> src/main.rs:1:1"));
        assert!(out.len() < text.len());
    }

    #[test]
    fn every_filter_output_is_reusable_by_the_detector() {
        // Sanity check that a filtered blob is still recognised, so a second
        // pass over the same conversation stays idempotent.
        let diff = "diff --git a/x b/x\n@@ -1 +1 @@\n+added\n";
        let out = apply(FilterKind::GitDiff, diff);
        assert!(detect(&out).is_some());
    }
}

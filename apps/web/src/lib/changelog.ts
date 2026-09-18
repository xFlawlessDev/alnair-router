// The root changelog, bundled so the page works offline. A relative path is
// used because an alias would have to resolve a file plus the `?raw` suffix.
import raw from "../../../../CHANGELOG.md?raw";

/** The trailing `([hash](url))` standard-version appends to each bullet. */
export interface ChangelogCommit {
  /** Abbreviated hash, e.g. `7d64c57`. */
  hash: string;
  url: string;
}

/** One bullet under a release, grouped by its conventional-commit type. */
export interface ChangelogChange {
  /** `Features`, `Bug Fixes`, `Performance Improvements`, … */
  group: string;
  text: string;
  /** Conventional-commit scope, e.g. `routing` from `feat(routing): …`. */
  scope: string | null;
  /** Commit this line came from, when the changelog carries a link. */
  commit: ChangelogCommit | null;
}

/** A released version and everything listed under it. */
export interface ChangelogRelease {
  version: string;
  /** `YYYY-MM-DD` when the heading carried one, else null. */
  date: string | null;
  /** `Added`, `Fixed`, … as written in the changelog; null for the root entry. */
  kind: string | null;
  /** Group headings in the order they appear, so the UI keeps the file's order. */
  groups: string[];
  changes: ChangelogChange[];
  /** Prose that is not a bullet or a heading, in file order. */
  paragraphs: string[];
}

const HEADING = /^(#{2,3})\s+(.+?)\s*$/;
const LINK = /\[([^\]]+)\]\([^)]*\)/g;
const BULLET = /^[*-]\s+(.+?)\s*$/;

/**
 * A version such as `1.2.3` or `v1.2.3`. standard-version writes the patch
 * releases as a linked `###` heading, so a heading only counts as a release
 * when its text carries a version and a bare word (`### Features`) does not.
 */
const VERSION = /\bv?\d+\.\d+\.\d+\b/;

/** `[1.2.3](https://…)` -> `1.2.3`, leaving a plain heading untouched. */
function stripLinks(heading: string): string {
  return heading.replace(LINK, "$1");
}

/** `1.2.3 (2026-01-31)` or `[1.2.3] - 2026-01-31` -> version + date. */
export function parseReleaseHeading(heading: string): {
  version: string;
  kind: string | null;
} {
  const trimmed = stripLinks(heading).replace(/[[\]]/g, "").trim();
  const match = /^(\S+)(?:\s*[-–—(]\s*([^)]+?)\)?\s*)?$/.exec(trimmed);
  const version = (match?.[1] ?? trimmed).trim();
  const kind = /^\d/.test(version) ? null : version;
  return { version, kind };
}

/** Pulls a scope out of `**scope:** text`, the shape standard-version writes. */
function splitScope(text: string): { scope: string | null; text: string } {
  const match = /^\*\*(.+?):?\*\*:?\s*(.*)$/.exec(text);
  if (!match) return { scope: null, text };
  return { scope: match[1]!.trim(), text: match[2]!.trim() };
}

/** The `([hash](url))` suffix standard-version appends to a bullet. */
const COMMIT = /\s*\(\[([0-9a-f]{7,})\]\(([^)]+)\)\)\s*$/i;

/** Splits a bullet into its prose and the commit link at the end, if any. */
function splitCommit(text: string): {
  text: string;
  commit: ChangelogCommit | null;
} {
  const match = COMMIT.exec(text);
  if (!match) return { text, commit: null };
  const trimmed = text.slice(0, match.index).trim();
  return { text: trimmed, commit: { hash: match[1]!, url: match[2]! } };
}

/** Strips markdown links and code ticks so the bullet reads as plain prose. */
export function stripMarkdown(text: string): string {
  return text
    .replace(LINK, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .trim();
}

/** Parses a bullet into its change, lifting the scope and commit link out. */
export function parseChange(
  raw: string,
  group: string,
): ChangelogChange | null {
  const { scope, text: afterScope } = splitScope(raw);
  const { text, commit } = splitCommit(afterScope);
  const clean = stripMarkdown(text);
  if (!clean) return null;
  return { group, text: clean, scope, commit };
}

/**
 * Splits the bundled changelog into releases, newest first.
 *
 * The file is plain markdown rendered from conventional commits, so this is a
 * deliberately small reader: anything it does not recognise lands in
 * `paragraphs` and is still shown, rather than being dropped.
 */
export function parseChangelog(markdown: string): ChangelogRelease[] {
  const releases: ChangelogRelease[] = [];
  let release: ChangelogRelease | null = null;
  let group = "";

  for (const line of markdown.split(/\r?\n/)) {
    const heading = HEADING.exec(line);
    const groupMatch = heading && !VERSION.test(heading[2]!) ? heading : null;

    if (heading && !groupMatch) {
      const { version, kind } = parseReleaseHeading(heading[2]!);
      const date = /\((\d{4}-\d{2}-\d{2})\)|\b(\d{4}-\d{2}-\d{2})\b/.exec(
        stripLinks(heading[2]!),
      );
      release = {
        version,
        date: date?.[1] ?? date?.[2] ?? null,
        kind,
        groups: [],
        changes: [],
        paragraphs: [],
      };
      releases.push(release);
      group = "";
      continue;
    }

    if (groupMatch) {
      group = stripLinks(groupMatch[2]!).trim();
      if (release && !release.groups.includes(group)) {
        release.groups.push(group);
      }
      continue;
    }

    if (!release) continue;

    const bullet = BULLET.exec(line);
    if (bullet) {
      const change = parseChange(bullet[1]!, group);
      if (change) release.changes.push(change);
      continue;
    }

    const text = stripMarkdown(line.trim());
    if (text && !text.startsWith("#")) release.paragraphs.push(text);
  }

  return releases;
}

export const changelog = parseChangelog(raw);

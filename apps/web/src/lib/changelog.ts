// The root changelog, bundled so the page works offline. A relative path is
// used because an alias would have to resolve a file plus the `?raw` suffix.
import raw from "../../../../CHANGELOG.md?raw";

/** One bullet under a release, grouped by its conventional-commit type. */
export interface ChangelogChange {
  /** `Features`, `Bug Fixes`, `Performance Improvements`, … */
  group: string;
  text: string;
  /** Conventional-commit scope, e.g. `routing` from `feat(routing): …`. */
  scope: string | null;
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

const RELEASE_HEADING = /^##\s+(.+?)\s*$/;
const GROUP_HEADING = /^###\s+(.+?)\s*$/;
const BULLET = /^[*-]\s+(.+?)\s*$/;

/** `1.2.3 (2026-01-31)` or `[1.2.3] - 2026-01-31` -> version + date. */
export function parseReleaseHeading(heading: string): {
  version: string;
  kind: string | null;
} {
  const trimmed = heading.replace(/[[\]]/g, "").trim();
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
    const releaseMatch = RELEASE_HEADING.exec(line);
    if (releaseMatch) {
      const { version, kind } = parseReleaseHeading(releaseMatch[1]!);
      const date = /\((\d{4}-\d{2}-\d{2})\)|\b(\d{4}-\d{2}-\d{2})\b/.exec(
        releaseMatch[1]!,
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

    const groupMatch = GROUP_HEADING.exec(line);
    if (groupMatch) {
      group = groupMatch[1]!.trim();
      if (release && !release.groups.includes(group)) {
        release.groups.push(group);
      }
      continue;
    }

    if (!release) continue;

    const bullet = BULLET.exec(line);
    if (bullet) {
      const { scope, text } = splitScope(bullet[1]!);
      if (text) release.changes.push({ group, text, scope });
      continue;
    }

    const text = line.trim();
    if (text && !text.startsWith("#")) release.paragraphs.push(text);
  }

  return releases;
}

export const changelog = parseChangelog(raw);

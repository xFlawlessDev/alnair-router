import { describe, expect, it } from "vitest";

import {
  changelog,
  parseChangelog,
  parseReleaseHeading,
  stripMarkdown,
} from "./changelog";

const SAMPLE = `# Changelog

All notable changes to this project are documented in this file.

## 1.2.0 (2026-02-01)

### Features

* **routing:** prefixed alias resolution
* a change with no scope
* **keys:** \`hot\` reload of plans

### Bug Fixes

* keep usage rows when a refresh fails

### Notes

Supersedes 1.1.0 for anyone on Windows.

## 1.1.0 (2026-01-15)

### Features

* **web:** add the changelog page
`;

describe("parseReleaseHeading", () => {
  it("reads a plain version with its date", () => {
    expect(parseReleaseHeading("1.2.0 (2026-02-01)")).toEqual({
      version: "1.2.0",
      kind: null,
    });
  });

  it("reads a bracketed version and a non-version heading", () => {
    expect(parseReleaseHeading("[1.2.0] - 2026-02-01").version).toBe("1.2.0");
    expect(parseReleaseHeading("Unreleased").version).toBe("Unreleased");
  });

  it("flags a heading that is not a version as a kind", () => {
    expect(parseReleaseHeading("Unreleased").kind).toBe("Unreleased");
    expect(parseReleaseHeading("1.2.0").kind).toBeNull();
  });
});

describe("parseChangelog", () => {
  it("splits the file into releases, newest first", () => {
    const releases = parseChangelog(SAMPLE);

    expect(releases.map((release) => release.version)).toEqual([
      "1.2.0",
      "1.1.0",
    ]);
    expect(releases[0]!.date).toBe("2026-02-01");
    expect(releases[1]!.date).toBe("2026-01-15");
  });

  it("groups changes under their heading, in file order", () => {
    const [latest] = parseChangelog(SAMPLE);

    expect(latest!.groups).toEqual(["Features", "Bug Fixes", "Notes"]);
    expect(latest!.changes.filter((c) => c.group === "Features")).toHaveLength(
      3,
    );
    expect(latest!.changes.filter((c) => c.group === "Bug Fixes")).toHaveLength(
      1,
    );
  });

  it("lifts the conventional-commit scope off the bullet", () => {
    const [latest] = parseChangelog(SAMPLE);
    const scoped = latest!.changes.find((c) => c.scope === "routing");

    expect(scoped?.text).toBe("prefixed alias resolution");
    // A bullet without a scope keeps its text intact.
    const plain = latest!.changes.find((c) =>
      c.text.startsWith("a change with no scope"),
    );
    expect(plain?.scope).toBeNull();
  });

  it("keeps prose that is not a bullet under the release", () => {
    const [latest] = parseChangelog(SAMPLE);
    expect(latest!.paragraphs).toContain(
      "Supersedes 1.1.0 for anyone on Windows.",
    );
  });

  it("lifts the commit link off a bullet", () => {
    const releases = parseChangelog(
      "## 1.0.0 (2026-01-01)\n\n* fix the thing ([7d64c57](https://example.com/commit/7d64c57))\n",
    );
    const change = releases[0]!.changes[0]!;

    expect(change.text).toBe("fix the thing");
    expect(change.commit).toEqual({
      hash: "7d64c57",
      url: "https://example.com/commit/7d64c57",
    });
  });

  it("renders inline code without the backticks", () => {
    const releases = parseChangelog(
      "## 1.0.0 (2026-01-01)\n\n* add `router.catalog_ttl_ms` support\n",
    );
    expect(releases[0]!.changes[0]!.text).toBe(
      "add router.catalog_ttl_ms support",
    );
  });

  it("keeps a plain bullet as plain text with no commit", () => {
    const releases = parseChangelog(
      "## 1.0.0 (2026-01-01)\n\n* a change with no scope\n",
    );
    expect(releases[0]!.changes[0]!.commit).toBeNull();
  });

  it("strips markdown links and code from free text", () => {
    expect(stripMarkdown("see [docs](https://example.com) and `code`")).toBe(
      "see docs and code",
    );
  });

  it("ignores the file header before the first release", () => {
    const [latest] = parseChangelog(SAMPLE);
    expect(latest!.version).toBe("1.2.0");
    expect(
      parseChangelog(SAMPLE).some((release) =>
        release.paragraphs.some((line) => line.includes("notable changes")),
      ),
    ).toBe(false);
  });

  it("returns nothing for a file with no release headings", () => {
    expect(parseChangelog("# Changelog\n\nNothing shipped yet.\n")).toEqual([]);
  });

  it("reads standard-version's linked patch headings as releases", () => {
    // Patch releases arrive as `### [1.2.1](compare-url) (date)`; they must not
    // be swallowed as group names under the previous release.
    const linked = `# Changelog

### [1.2.1](https://example.com/compare/v1.2.0...v1.2.1) (2026-02-02)


### Bug Fixes

* fix one

## 1.2.0 (2026-02-01)

### Features

* add one
`;
    const releases = parseChangelog(linked);

    expect(releases.map((release) => release.version)).toEqual([
      "1.2.1",
      "1.2.0",
    ]);
    expect(releases[0]!.date).toBe("2026-02-02");
    expect(releases[0]!.groups).toEqual(["Bug Fixes"]);
    expect(releases[0]!.changes[0]!.text).toBe("fix one");
  });

  it("parses the bundled changelog into releases", () => {
    // Guards the shipped file: a malformed heading would silently empty the
    // page, and the parser is the only thing reading it.
    expect(changelog.length).toBeGreaterThan(0);
    expect(changelog[0]!.version).toMatch(/^\d+\.\d+\.\d+$/);
    expect(changelog[0]!.changes.length).toBeGreaterThan(0);
  });
});

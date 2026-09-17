import { describe, expect, it } from "vitest";

import { changelog, parseChangelog, parseReleaseHeading } from "./changelog";

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

  it("parses the bundled changelog into releases", () => {
    // Guards the shipped file: a malformed heading would silently empty the
    // page, and the parser is the only thing reading it.
    expect(changelog.length).toBeGreaterThan(0);
    expect(changelog[0]!.version).toMatch(/^\d+\.\d+\.\d+$/);
    expect(changelog[0]!.changes.length).toBeGreaterThan(0);
  });
});

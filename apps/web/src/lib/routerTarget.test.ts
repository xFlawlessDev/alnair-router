import { describe, expect, it } from "vitest";

import {
  DEFAULT_ROUTER_URL,
  parsePidAddress,
  resolveRouterTarget,
  routerHome,
} from "./routerTarget";

describe("parsePidAddress", () => {
  it("reads the second line of a PID record", () => {
    expect(parsePidAddress("1234\n127.0.0.1:9000\n")).toBe("127.0.0.1:9000");
  });

  it("tolerates CRLF and surrounding whitespace", () => {
    expect(parsePidAddress("1234\r\n  127.0.0.1:9000  \r\n")).toBe(
      "127.0.0.1:9000",
    );
  });

  it("is undefined for a missing or empty address", () => {
    for (const contents of ["", "1234", "1234\n", "1234\n  \n"]) {
      expect(parsePidAddress(contents)).toBeUndefined();
    }
  });
});

describe("routerHome", () => {
  it("prefers ALNAIR_ROUTER_HOME", () => {
    expect(routerHome({ ALNAIR_ROUTER_HOME: "C:\\router" })).toBe("C:\\router");
  });

  it("falls back to ~/.alnair-router", () => {
    expect(routerHome({})).toMatch(/\.alnair-router$/);
  });
});

describe("resolveRouterTarget", () => {
  const read = (contents: string | undefined) => () => contents;

  it("prefers an explicit ALNAIR_ROUTER_URL", () => {
    expect(
      resolveRouterTarget(
        { ALNAIR_ROUTER_URL: "http://192.168.1.10:9000" },
        read("1234\n127.0.0.1:7000\n"),
      ),
    ).toBe("http://192.168.1.10:9000");
  });

  it("follows the port the running router recorded", () => {
    expect(resolveRouterTarget({}, read("1234\n127.0.0.1:9000\n"))).toBe(
      "http://127.0.0.1:9000",
    );
  });

  it("falls back to the default without a PID file", () => {
    expect(resolveRouterTarget({}, read(undefined))).toBe(DEFAULT_ROUTER_URL);
  });

  it("ignores a malformed PID file", () => {
    expect(resolveRouterTarget({}, read("1234\n"))).toBe(DEFAULT_ROUTER_URL);
  });
});

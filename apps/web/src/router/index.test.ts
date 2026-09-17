import { describe, expect, it } from "vitest";

import { router } from "./index";

describe("router", () => {
  it("defines the dashboard routes", () => {
    // `getRoutes()` orders by path specificity, and the legacy playground
    // redirect has no name, so names are compared as a set.
    const names = router
      .getRoutes()
      .map((route) => route.name)
      .filter((name): name is string => typeof name === "string");

    expect(names).toContain("playground");
    expect(names).toContain("token-saver");
    expect(names).toContain("changelog");
    expect(names).not.toContain("token-saver-playground");
    expect(names).toHaveLength(16);
  });

  it("redirects the old playground URL to the hub", () => {
    // The playground outgrew its token-saver-only URL, but a bookmark must
    // still land somewhere useful.
    const legacy = router
      .getRoutes()
      .find((route) => route.path === "/token-saver/playground");
    expect(legacy?.redirect).toEqual({ name: "playground" });
  });
});

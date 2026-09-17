import { describe, expect, it } from "vitest";

import { router } from "./index";

describe("router", () => {
  it("defines the dashboard routes", () => {
    // `getRoutes()` orders by path specificity, so the nested playground route
    // lands at the front.
    expect(router.getRoutes().map((route) => route.name)).toEqual([
      "token-saver-playground",
      "overview",
      "connections",
      "aliases",
      "combos",
      "keys",
      "pricing",
      "token-saver",
      "settings",
      "usage",
      "console",
      "guide",
      "login",
      "my-usage",
      "not-found",
    ]);
  });
});

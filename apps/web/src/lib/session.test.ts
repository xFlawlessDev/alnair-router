import { nextTick } from "vue";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { AuthSession } from "@/types/api";

const sample: AuthSession = {
  access_token: "access-1",
  refresh_token: "refresh-1",
  access_expires_at: "2026-01-01T00:00:00Z",
  refresh_expires_at: "2026-01-08T00:00:00Z",
};

describe("session store", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.resetModules();
  });

  it("persists the tokens as JSON", async () => {
    const { setSession } = await import("./session");
    setSession(sample);
    await nextTick();

    expect(localStorage.getItem("alnair-router.session")).toBe(
      JSON.stringify(sample),
    );
  });

  it("restores the tokens after a page reload", async () => {
    const first = await import("./session");
    first.setSession(sample);
    await nextTick();

    vi.resetModules();
    const reloaded = await import("./session");

    expect(reloaded.getSession()).toEqual(sample);
    expect(reloaded.getAccessToken()).toBe("access-1");
    expect(reloaded.getRefreshToken()).toBe("refresh-1");
  });

  it("drops the stored tokens on sign-out", async () => {
    const { setSession, getSession } = await import("./session");
    setSession(sample);
    await nextTick();

    setSession(null);
    await nextTick();

    expect(localStorage.getItem("alnair-router.session")).toBeNull();
    expect(getSession()).toBeNull();
  });
});

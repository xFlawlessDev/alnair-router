import { beforeEach, describe, expect, it, vi } from "vitest";

import type { AuthStatus } from "@/types/api";

const { loadAuthStatus } = vi.hoisted(() => ({ loadAuthStatus: vi.fn() }));

vi.mock("@/lib/authState", () => ({ loadAuthStatus }));

import { router } from "./index";

const status = (overrides: Partial<AuthStatus> = {}): AuthStatus => ({
  password_set: true,
  setup_required: false,
  authenticated: true,
  admin_token_set: false,
  admin_open: false,
  ...overrides,
});

describe("router", () => {
  beforeEach(async () => {
    loadAuthStatus.mockReset();
    // A null status is the unreachable-router case, so the guard lets a reset
    // navigation through. `/login` is public, so the reset is always allowed.
    loadAuthStatus.mockResolvedValue(null);
    await router.replace("/login");
  });

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

  it("sends an unauthenticated visitor to login with a redirect", async () => {
    loadAuthStatus.mockResolvedValue(status({ authenticated: false }));

    await router.push("/connections");

    expect(router.currentRoute.value.name).toBe("login");
    expect(router.currentRoute.value.query.redirect).toBe("/connections");
  });

  it("routes a first run to setup even under the open posture", async () => {
    // Before any password exists the loopback posture is `admin_open`, but the
    // owner still has to land on the setup form.
    loadAuthStatus.mockResolvedValue(
      status({
        authenticated: false,
        password_set: false,
        setup_required: true,
        admin_open: true,
      }),
    );

    await router.push("/");

    expect(router.currentRoute.value.name).toBe("login");
  });

  it("allows the open localhost posture once a password exists", async () => {
    loadAuthStatus.mockResolvedValue(
      status({ authenticated: false, admin_open: true }),
    );

    await router.push("/connections");

    expect(router.currentRoute.value.name).toBe("connections");
  });

  it("keeps public routes reachable without a session", async () => {
    loadAuthStatus.mockResolvedValue(status({ authenticated: false }));

    await router.push("/me");

    expect(router.currentRoute.value.name).toBe("my-usage");
  });
});

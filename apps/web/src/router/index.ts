import { createRouter, createWebHistory } from "vue-router";

import AliasesPage from "@/pages/AliasesPage.vue";
import CombosPage from "@/pages/CombosPage.vue";
import ConnectionsPage from "@/pages/ConnectionsPage.vue";
import ConsolePage from "@/pages/ConsolePage.vue";
import GuidePage from "@/pages/GuidePage.vue";
import KeysPage from "@/pages/KeysPage.vue";
import LoginPage from "@/pages/LoginPage.vue";
import NotFoundPage from "@/pages/NotFoundPage.vue";
import OverviewPage from "@/pages/OverviewPage.vue";
import PricingPage from "@/pages/PricingPage.vue";
import PublicUsagePage from "@/pages/PublicUsagePage.vue";
import SettingsPage from "@/pages/SettingsPage.vue";
import UsagePage from "@/pages/UsagePage.vue";

import { loadAuthStatus } from "@/lib/authState";
import { getAdminToken } from "@/lib/adminToken";

declare module "vue-router" {
  interface RouteMeta {
    title?: string;
    /** Renders the minimal public shell instead of the admin dashboard. */
    public?: boolean;
  }
}

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: "/",
      name: "overview",
      component: OverviewPage,
      meta: { title: "Overview" },
    },
    {
      path: "/connections",
      name: "connections",
      component: ConnectionsPage,
      meta: { title: "Connections" },
    },
    {
      path: "/aliases",
      name: "aliases",
      component: AliasesPage,
      meta: { title: "Aliases" },
    },
    {
      path: "/combos",
      name: "combos",
      component: CombosPage,
      meta: { title: "Combos" },
    },
    {
      path: "/keys",
      name: "keys",
      component: KeysPage,
      meta: { title: "API Keys" },
    },
    {
      path: "/pricing",
      name: "pricing",
      component: PricingPage,
      meta: { title: "Pricing" },
    },
    {
      path: "/settings",
      name: "settings",
      component: SettingsPage,
      meta: { title: "Settings" },
    },
    {
      path: "/usage",
      name: "usage",
      component: UsagePage,
      meta: { title: "Usage" },
    },
    {
      path: "/logs",
      name: "console",
      component: ConsolePage,
      meta: { title: "Console" },
    },
    {
      path: "/guide",
      name: "guide",
      component: GuidePage,
      meta: { title: "API Guide" },
    },
    {
      path: "/login",
      name: "login",
      component: LoginPage,
      meta: { title: "Sign in", public: true },
    },
    {
      path: "/me",
      name: "my-usage",
      component: PublicUsagePage,
      meta: { title: "My Usage", public: true },
    },
    {
      path: "/:pathMatch(.*)*",
      name: "not-found",
      component: NotFoundPage,
      meta: { title: "Not found" },
    },
  ],
  scrollBehavior: () => ({ top: 0 }),
});

router.afterEach((to) => {
  document.title = to.meta.title
    ? `${to.meta.title} | Alnair Router`
    : "Alnair Router";
});

// Admin routes need a password session, a legacy admin token, or the open
// localhost posture. Public routes (the sign-in screen and /me) skip the check.
router.beforeEach(async (to) => {
  if (to.meta.public) return true;

  const status = await loadAuthStatus();
  if (status === null) return true;

  if (status.authenticated || status.admin_open || getAdminToken()) return true;
  return { name: "login", query: { redirect: to.fullPath } };
});

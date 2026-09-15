import { createRouter, createWebHistory } from 'vue-router';

import AliasesPage from '@/pages/AliasesPage.vue';
import CombosPage from '@/pages/CombosPage.vue';
import ConnectionsPage from '@/pages/ConnectionsPage.vue';
import ConsolePage from '@/pages/ConsolePage.vue';
import GuidePage from '@/pages/GuidePage.vue';
import KeysPage from '@/pages/KeysPage.vue';
import ModelsPage from '@/pages/ModelsPage.vue';
import NotFoundPage from '@/pages/NotFoundPage.vue';
import OverviewPage from '@/pages/OverviewPage.vue';
import PricingPage from '@/pages/PricingPage.vue';
import SettingsPage from '@/pages/SettingsPage.vue';
import UsagePage from '@/pages/UsagePage.vue';

declare module 'vue-router' {
  interface RouteMeta {
    title?: string;
  }
}

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/', name: 'overview', component: OverviewPage, meta: { title: 'Overview' } },
    {
      path: '/connections',
      name: 'connections',
      component: ConnectionsPage,
      meta: { title: 'Connections' },
    },
    { path: '/aliases', name: 'aliases', component: AliasesPage, meta: { title: 'Aliases' } },
    { path: '/combos', name: 'combos', component: CombosPage, meta: { title: 'Combos' } },
    { path: '/models', name: 'models', component: ModelsPage, meta: { title: 'Models' } },
    { path: '/keys', name: 'keys', component: KeysPage, meta: { title: 'API Keys' } },
    { path: '/pricing', name: 'pricing', component: PricingPage, meta: { title: 'Pricing' } },
    { path: '/settings', name: 'settings', component: SettingsPage, meta: { title: 'Settings' } },
    { path: '/usage', name: 'usage', component: UsagePage, meta: { title: 'Usage' } },
    { path: '/logs', name: 'console', component: ConsolePage, meta: { title: 'Console' } },
    { path: '/guide', name: 'guide', component: GuidePage, meta: { title: 'API Guide' } },
    {
      path: '/:pathMatch(.*)*',
      name: 'not-found',
      component: NotFoundPage,
      meta: { title: 'Not found' },
    },
  ],
  scrollBehavior: () => ({ top: 0 }),
});

router.afterEach((to) => {
  document.title = to.meta.title ? `${to.meta.title} | Alnair Router` : 'Alnair Router';
});

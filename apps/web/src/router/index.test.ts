import { describe, expect, it } from 'vitest';

import { router } from './index';

describe('router', () => {
  it('defines the dashboard routes', () => {
    expect(router.getRoutes().map((route) => route.name)).toEqual([
      'overview',
      'connections',
      'aliases',
      'combos',
      'models',
      'keys',
      'pricing',
      'settings',
      'usage',
      'console',
      'guide',
      'login',
      'my-usage',
      'not-found',
    ]);
  });
});

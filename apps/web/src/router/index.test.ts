import { describe, expect, it } from 'vitest';

import { router } from './index';

describe('router', () => {
  it('defines the dashboard routes', () => {
    expect(router.getRoutes().map((route) => route.name)).toEqual([
      'overview',
      'connections',
      'aliases',
      'combos',
      'keys',
      'usage',
      'console',
      'about',
      'not-found',
    ]);
  });
});

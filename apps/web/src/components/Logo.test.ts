import { createApp, h, nextTick } from 'vue';
import { describe, expect, it } from 'vitest';

import Logo from './Logo.vue';

async function mount(props: Record<string, unknown> = {}) {
  const container = document.createElement('div');
  document.body.appendChild(container);

  const app = createApp({ render: () => h(Logo, props) });
  app.mount(container);
  await nextTick();

  const root = container.firstElementChild as HTMLElement;

  return {
    container,
    root,
    unmount: () => {
      app.unmount();
      container.remove();
    },
  };
}

describe('Logo', () => {
  it('renders the shared artwork repainted with currentColor', async () => {
    const { container, unmount } = await mount();

    expect(container.querySelector('svg')).toBeTruthy();
    expect(container.querySelector('path, polygon')).toBeTruthy();
    expect(container.innerHTML).toContain('fill: currentColor');
    expect(container.innerHTML).not.toContain('#fff');

    unmount();
  });

  it('keeps sizing classes on the wrapper', async () => {
    const { root, unmount } = await mount({ class: 'size-8' });

    expect(root.classList.contains('size-8')).toBe(true);

    unmount();
  });

  it('is decorative by default and labelled when a title is given', async () => {
    const decorative = await mount();
    expect(decorative.root.getAttribute('aria-hidden')).toBe('true');
    expect(decorative.root.getAttribute('role')).toBeNull();
    decorative.unmount();

    const labelled = await mount({ title: 'Alnair Router' });
    expect(labelled.root.getAttribute('role')).toBe('img');
    expect(labelled.root.getAttribute('aria-label')).toBe('Alnair Router');
    expect(labelled.root.getAttribute('aria-hidden')).toBeNull();
    labelled.unmount();
  });
});

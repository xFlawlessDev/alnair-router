import { describe, expect, it } from 'vitest';

import {
  LEFT_X,
  NODE_WIDTH,
  RIGHT_X,
  ROUTER_ID,
  ROUTER_SIZE,
  ROUTER_X,
  buildTopology,
} from './topology';
import type { ConnectionActivity } from '@/types/api';

function connection(id: string, inFlight = 0): ConnectionActivity {
  return {
    id,
    name: id,
    in_flight: inFlight,
    requests: 0,
    failures: 0,
    prompt_tokens: 0,
    completion_tokens: 0,
    total_latency_ms: 0,
  };
}

describe('buildTopology', () => {
  it('keeps columns from overlapping the router hub', () => {
    const { nodes } = buildTopology([connection('a'), connection('b'), connection('c')]);
    const router = nodes.find((node) => node.id === ROUTER_ID);

    expect(router).toBeDefined();
    expect(ROUTER_X).toBeGreaterThanOrEqual(LEFT_X + NODE_WIDTH + 24);
    expect(RIGHT_X).toBeGreaterThanOrEqual(ROUTER_X + ROUTER_SIZE + 24);
    expect(router!.position.x).toBe(ROUTER_X);
  });

  it('creates one edge per connection, animated only while in flight', () => {
    const { edges } = buildTopology([
      connection('idle'),
      connection('busy', 2),
    ]);

    const idle = edges.find((edge) => edge.source === 'idle');
    const busy = edges.find((edge) => edge.source === 'busy');

    expect(edges).toHaveLength(2);
    expect(idle!.animated).toBe(false);
    expect(idle!.style.strokeDasharray).toBe('7 5');
    expect(busy!.animated).toBe(true);
    expect(busy!.style.stroke).toBe('#10b981');
    expect(busy!.targetHandle).toBe('right');
  });

  it('routes left-column edges to the router left handle', () => {
    const { edges } = buildTopology([connection('first'), connection('second')]);

    expect(edges.find((edge) => edge.source === 'first')!.targetHandle).toBe('left');
    expect(edges.find((edge) => edge.source === 'second')!.targetHandle).toBe('right');
  });
});

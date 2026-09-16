import type { ConnectionActivity } from "@/types/api";

/** Layout constants for the provider topology, in flow coordinates (px). */
export const ROUTER_ID = "router";
export const ROW_HEIGHT = 104;
export const NODE_WIDTH = 176;
export const ROUTER_SIZE = 96;
export const COLUMN_GAP = 80;
export const LEFT_X = 0;
export const ROUTER_X = LEFT_X + NODE_WIDTH + COLUMN_GAP;
export const RIGHT_X = ROUTER_X + ROUTER_SIZE + COLUMN_GAP;

export interface TopologyNodeData {
  connection: ConnectionActivity;
  side: "left" | "right";
}

export interface TopologyNode {
  id: string;
  type: "router" | "connection";
  position: { x: number; y: number };
  data: TopologyNodeData | null;
  draggable: boolean;
  selectable?: boolean;
}

export interface TopologyEdge {
  id: string;
  source: string;
  target: string;
  sourceHandle: string;
  targetHandle: string;
  animated: boolean;
  style: Record<string, string | number>;
}

export interface Topology {
  nodes: TopologyNode[];
  edges: TopologyEdge[];
}

/**
 * Builds the graph: connections alternate left/right columns around the router
 * hub, and an edge animates only while that provider has a request in flight.
 */
export function buildTopology(connections: ConnectionActivity[]): Topology {
  const left = connections.filter((_, index) => index % 2 === 0);
  const right = connections.filter((_, index) => index % 2 === 1);
  const rows = Math.max(left.length, right.length, 1);
  const centerY = ((rows - 1) * ROW_HEIGHT) / 2 - 40;

  const nodes: TopologyNode[] = [];

  left.forEach((connection, index) => {
    nodes.push({
      id: connection.id,
      type: "connection",
      position: { x: LEFT_X, y: index * ROW_HEIGHT },
      data: { connection, side: "left" },
      draggable: false,
    });
  });

  right.forEach((connection, index) => {
    nodes.push({
      id: connection.id,
      type: "connection",
      position: { x: RIGHT_X, y: index * ROW_HEIGHT },
      data: { connection, side: "right" },
      draggable: false,
    });
  });

  nodes.push({
    id: ROUTER_ID,
    type: "router",
    position: { x: ROUTER_X, y: centerY },
    data: null,
    draggable: false,
    selectable: false,
  });

  const edges: TopologyEdge[] = connections.map((connection) => {
    const active = connection.in_flight > 0;
    const side = left.some((item) => item.id === connection.id)
      ? "left"
      : "right";
    const style: Record<string, string | number> = active
      ? { stroke: "#10b981", strokeWidth: 2.5 }
      : {
          stroke: "var(--muted-foreground)",
          strokeWidth: 2,
          strokeDasharray: "7 5",
          opacity: 0.7,
        };

    return {
      id: `edge-${connection.id}`,
      source: connection.id,
      target: ROUTER_ID,
      sourceHandle: "source",
      targetHandle: side,
      animated: active,
      style,
    };
  });

  return { nodes, edges };
}

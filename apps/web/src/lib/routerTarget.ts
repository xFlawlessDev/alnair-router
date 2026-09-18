import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

/** Where the proxy points when nothing else says otherwise. */
export const DEFAULT_ROUTER_URL = "http://127.0.0.1:7878";

/** Reads the `host:port` a serving router recorded in its PID file. */
export function parsePidAddress(contents: string): string | undefined {
  const lines = contents.split(/\r?\n/);
  const address = lines[1]?.trim();
  return address ? address : undefined;
}

/** Router home: `$ALNAIR_ROUTER_HOME`, else `~/.alnair-router`. */
export function routerHome(env: NodeJS.ProcessEnv = process.env): string {
  const home = env.ALNAIR_ROUTER_HOME?.trim();
  return home ? home : join(homedir(), ".alnair-router");
}

/**
 * Resolves the dev-proxy target, in order: an explicit `ALNAIR_ROUTER_URL`,
 * the address in `router.pid` (so `--port` and `server.port` are followed
 * automatically), then the default.
 */
export function resolveRouterTarget(
  env: NodeJS.ProcessEnv = process.env,
  read: (path: string) => string | undefined = readText,
): string {
  const explicit = env.ALNAIR_ROUTER_URL?.trim();
  if (explicit) return explicit;

  const address = parsePidAddress(read(join(routerHome(env), "router.pid")) ?? "");
  return address ? `http://${address}` : DEFAULT_ROUTER_URL;
}

function readText(path: string): string | undefined {
  try {
    return readFileSync(path, "utf8");
  } catch {
    return undefined;
  }
}

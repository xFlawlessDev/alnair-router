import {
  copyFile,
  cp,
  mkdir,
  readFile,
  rename,
  rm,
  writeFile,
} from "node:fs/promises";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repo = resolve(root, "../..");
const output = resolve(root, "dist");
const staging = resolve(root, "dist-next");

await rm(staging, { recursive: true, force: true });
await mkdir(resolve(staging, "assets"), { recursive: true });
await cp(resolve(root, "src"), staging, { recursive: true });
await cp(resolve(root, "public"), staging, { recursive: true });
await copyFile(
  resolve(repo, "apps/web/public/favicon.ico"),
  resolve(staging, "favicon.ico"),
);
const assets = [
  ["assets/alnair-white.svg", "alnair-white.svg"],
  ["apps/web/public/favicon.svg", "favicon.svg"],
  ["docs/screenshots/02-overview.png", "overview.png"],
  ["docs/screenshots/03-connections.png", "connections.png"],
  ["docs/screenshots/18-usage-trend.png", "usage.png"],
  ["docs/screenshots/17-my-usage-share.png", "usage-share.png"],
];
for (const [source, destination] of assets) {
  await copyFile(
    resolve(repo, source),
    resolve(staging, "assets", destination),
  );
}
const hashes = new Set();
for (const page of ["index.html", "compare/index.html"]) {
  const html = await readFile(resolve(staging, page), "utf8");
  for (const [, json] of html.matchAll(
    /<script type="application\/ld\+json">([\s\S]*?)<\/script>/g,
  )) {
    JSON.parse(json);
    hashes.add(
      `'sha256-${createHash("sha256").update(json).digest("base64")}'`,
    );
  }
}
const headersPath = resolve(staging, "_headers");
const headers = await readFile(headersPath, "utf8");
await writeFile(
  headersPath,
  headers.replace(
    "script-src 'self'",
    `script-src 'self' ${[...hashes].join(" ")}`,
  ),
);
await rm(output, { recursive: true, force: true });
await rename(staging, output);
console.log(
  "Built apps/landing/dist with shared brand assets and dashboard screenshots.",
);

#!/usr/bin/env node
/**
 * Keeps every manifest in lockstep with the version standard-version wrote to
 * the root package.json: both crate manifests, the dashboard package, and the
 * resolved Cargo.lock entry.
 */
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';

const version = JSON.parse(readFileSync('package.json', 'utf8')).version;
if (!/^\d+\.\d+\.\d+/.test(version)) {
  throw new Error(`unexpected version in package.json: ${version}`);
}

const crates = ['crates/alnair-router/Cargo.toml', 'crates/alnair-llm/Cargo.toml'];

for (const path of crates) {
  const toml = readFileSync(path, 'utf8');
  if (!/^version = "[^"]*"/m.test(toml)) {
    throw new Error(`no version field found in ${path}`);
  }
  writeFileSync(path, toml.replace(/^version = "[^"]*"/m, `version = "${version}"`));
}

const webPath = 'apps/web/package.json';
const web = JSON.parse(readFileSync(webPath, 'utf8'));
if (web.version !== version) {
  web.version = version;
  writeFileSync(webPath, `${JSON.stringify(web, null, 2)}\n`);
}

execFileSync('cargo', ['update', '--offline', '-p', 'alnair-router', '-p', 'alnair-llm'], {
  stdio: 'inherit',
});

// stage what we touched so the standard-version release commit includes it
execFileSync('git', ['add', 'Cargo.lock', webPath, ...crates], { stdio: 'inherit' });

console.log(`synced workspace manifests to v${version}`);

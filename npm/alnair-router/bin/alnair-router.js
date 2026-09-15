#!/usr/bin/env node
'use strict';

const { spawnSync } = require('node:child_process');
const path = require('node:path');

const PLATFORM_PACKAGES = {
  'darwin-arm64': '@xflawlessdev/alnair-router-darwin-arm64',
  'linux-x64': '@xflawlessdev/alnair-router-linux-x64',
  'win32-x64': '@xflawlessdev/alnair-router-win32-x64',
};

function fail(message) {
  console.error(`alnair-router: ${message}`);
  process.exit(1);
}

const key = `${process.platform}-${process.arch}`;
const packageName = PLATFORM_PACKAGES[key];

if (!packageName) {
  fail(
    `no prebuilt binary for ${key} (supported: ${Object.keys(PLATFORM_PACKAGES).join(', ')}); ` +
      'build from source instead: https://github.com/xFlawlessDev/alnair-router',
  );
}

const binaryName = process.platform === 'win32' ? 'alnair-router.exe' : 'alnair-router';

let binary;
try {
  const packageDir = path.dirname(require.resolve(`${packageName}/package.json`));
  binary = path.join(packageDir, 'bin', binaryName);
} catch {
  fail(`optional dependency ${packageName} is missing; reinstall without --no-optional`);
}

const result = spawnSync(binary, process.argv.slice(2), { stdio: 'inherit' });

if (result.error) {
  fail(result.error.message);
}

process.exit(result.status === null ? 1 : result.status);

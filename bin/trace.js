#!/usr/bin/env node

import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

import { BINARY_OVERRIDE_ENV } from '../npm/lib/installer.mjs';
import { getInstalledBinaryPath } from '../npm/lib/platform.mjs';

const rootDir = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..'
);

function resolveBinaryPath() {
  if (process.env[BINARY_OVERRIDE_ENV]) {
    return process.env[BINARY_OVERRIDE_ENV];
  }

  return getInstalledBinaryPath(rootDir);
}

let binaryPath;

try {
  binaryPath = resolveBinaryPath();
} catch (error) {
  console.error(`[vectizeit] ${error.message}`);
  process.exit(1);
}

if (!binaryPath || !existsSync(binaryPath)) {
  console.error(
    "[vectizeit] Native CLI binary is missing. Reinstall the package with 'npm install -g vectizeit' or set VECTIZEIT_BIN_PATH."
  );
  process.exit(1);
}

const child = spawn(binaryPath, process.argv.slice(2), {
  stdio: 'inherit',
});

child.on('error', (error) => {
  console.error(`[vectizeit] Failed to launch ${binaryPath}: ${error.message}`);
  process.exit(1);
});

child.on('exit', (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }

  process.exit(code ?? 1);
});

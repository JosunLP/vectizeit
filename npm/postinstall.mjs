#!/usr/bin/env node

import {
  getRepositoryRoot,
  installNativeBinary,
  shouldSkipBinaryDownload,
} from './lib/installer.mjs';

const rootDir = getRepositoryRoot();
const skip = await shouldSkipBinaryDownload(rootDir);

if (skip.skip) {
  console.log(`[vectizeit] ${skip.reason}`);
  process.exit(0);
}

try {
  await installNativeBinary({ rootDir, logger: console });
} catch (error) {
  console.error(
    `[vectizeit] Failed to install the native CLI: ${error.message}`
  );
  process.exit(1);
}

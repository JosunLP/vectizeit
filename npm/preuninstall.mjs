#!/usr/bin/env node

import { getRepositoryRoot, removeInstalledBinary } from './lib/installer.mjs';

try {
  await removeInstalledBinary(getRepositoryRoot());
} catch (error) {
  console.error(
    `[vectizeit] Failed to remove installed binaries: ${error.message}`
  );
  process.exit(1);
}

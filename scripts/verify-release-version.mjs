#!/usr/bin/env node

import { promises as fs } from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const rootDir = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..'
);
const expectedVersion = normalizeCliVersion(process.argv[2]);

const versionSources = {
  'package.json': async () =>
    JSON.parse(await fs.readFile(path.join(rootDir, 'package.json'), 'utf8'))
      .version,
  'crates/vectize/Cargo.toml': async () =>
    readCargoVersion('crates/vectize/Cargo.toml'),
  'crates/vectize-cli/Cargo.toml': async () =>
    readCargoVersion('crates/vectize-cli/Cargo.toml'),
  'crates/vectize-wasm/Cargo.toml': async () =>
    readCargoVersion('crates/vectize-wasm/Cargo.toml'),
};

const results = [];
for (const [file, reader] of Object.entries(versionSources)) {
  results.push([file, await reader()]);
}

const distinctVersions = [...new Set(results.map(([, version]) => version))];
if (distinctVersions.length !== 1) {
  const details = results
    .map(([file, version]) => `- ${file}: ${version}`)
    .join('\n');
  console.error(`Release version mismatch detected:\n${details}`);
  process.exit(1);
}

const resolvedVersion = distinctVersions[0];
if (expectedVersion && resolvedVersion !== expectedVersion) {
  console.error(
    `Release version '${resolvedVersion}' does not match the expected tag '${expectedVersion}'.`
  );
  process.exit(1);
}

console.log(
  `Verified release version ${resolvedVersion} across ${results.length} manifests.`
);

async function readCargoVersion(relativePath) {
  const content = await fs.readFile(path.join(rootDir, relativePath), 'utf8');
  const match = content.match(/^version\s*=\s*"([^"]+)"$/m);
  if (!match) {
    throw new Error(`Could not find a version entry in ${relativePath}.`);
  }

  return match[1];
}

function normalizeCliVersion(version) {
  if (!version) {
    return null;
  }

  return version.startsWith('v') ? version.slice(1) : version;
}

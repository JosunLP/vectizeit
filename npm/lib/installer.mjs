import { createHash } from 'node:crypto';
import { createWriteStream, promises as fs } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import process from 'node:process';
import { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { fileURLToPath } from 'node:url';

import extractZip from 'extract-zip';
import tar from 'tar';

import {
  buildReleaseDownloadUrl,
  DEFAULT_BINARY_NAME,
  RELEASE_CHECKSUM_FILE,
} from './github.mjs';
import {
  getInstalledBinaryPath,
  getPlatformAssetInfo,
  getVendorDirectory,
} from './platform.mjs';

export const SKIP_DOWNLOAD_ENV = 'VECTIZEIT_SKIP_DOWNLOAD';
export const DOWNLOAD_BASE_URL_ENV = 'VECTIZEIT_DOWNLOAD_BASE_URL';
export const BINARY_OVERRIDE_ENV = 'VECTIZEIT_BIN_PATH';

export function getRepositoryRoot() {
  return path.resolve(fileURLToPath(new URL('../..', import.meta.url)));
}

export async function readPackageVersion(rootDir = getRepositoryRoot()) {
  const packageJsonPath = path.join(rootDir, 'package.json');
  const packageJson = JSON.parse(await fs.readFile(packageJsonPath, 'utf8'));
  return packageJson.version;
}

export async function isRepositoryCheckout(rootDir = getRepositoryRoot()) {
  try {
    await fs.access(path.join(rootDir, '.git'));
    return true;
  } catch {
    return false;
  }
}

export async function shouldSkipBinaryDownload(rootDir = getRepositoryRoot()) {
  if (process.env[SKIP_DOWNLOAD_ENV] === '1') {
    return {
      skip: true,
      reason: `Skipping native download because ${SKIP_DOWNLOAD_ENV}=1.`,
    };
  }

  if (process.env[BINARY_OVERRIDE_ENV]) {
    return {
      skip: true,
      reason: `Skipping native download because ${BINARY_OVERRIDE_ENV} points to an explicit binary.`,
    };
  }

  if (await isRepositoryCheckout(rootDir)) {
    return {
      skip: true,
      reason:
        'Repository checkout detected; skipping native download during local development. Set VECTIZEIT_SKIP_DOWNLOAD=0 to force it.',
    };
  }

  return { skip: false };
}

function resolveDownloadUrl(version, assetName) {
  const overrideBaseUrl = process.env[DOWNLOAD_BASE_URL_ENV];
  if (overrideBaseUrl) {
    return `${overrideBaseUrl.replace(/\/$/, '')}/${assetName}`;
  }

  return buildReleaseDownloadUrl(version, assetName);
}

async function downloadToFile(url, filePath) {
  const response = await fetch(url, {
    headers: {
      'user-agent': 'vectizeit-npm-installer',
      accept: 'application/octet-stream, text/plain, */*',
    },
  });

  if (!response.ok || !response.body) {
    throw new Error(
      `Download failed for ${url}: ${response.status} ${response.statusText}`
    );
  }

  await pipeline(Readable.fromWeb(response.body), createWriteStream(filePath));
}

async function sha256(filePath) {
  const hash = createHash('sha256');
  const data = await fs.readFile(filePath);
  hash.update(data);
  return hash.digest('hex');
}

function parseExpectedChecksum(checksumContents, assetName) {
  const escapedAssetName = assetName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const checksumPattern = new RegExp(
    `^([a-f0-9]{64})\\s+[* ]${escapedAssetName}$`,
    'im'
  );
  const match = checksumContents.match(checksumPattern);

  if (!match) {
    throw new Error(
      `Could not find checksum for ${assetName} in ${RELEASE_CHECKSUM_FILE}.`
    );
  }

  return match[1];
}

async function verifyChecksum(archivePath, checksumFilePath, assetName) {
  const expected = parseExpectedChecksum(
    await fs.readFile(checksumFilePath, 'utf8'),
    assetName
  );
  const actual = await sha256(archivePath);

  if (actual !== expected) {
    throw new Error(
      `Checksum mismatch for ${assetName}. Expected ${expected}, received ${actual}.`
    );
  }
}

async function extractArchive(archivePath, extractDir, archiveExtension) {
  if (archiveExtension === 'zip') {
    await extractZip(archivePath, { dir: extractDir });
    return;
  }

  await tar.x({
    file: archivePath,
    cwd: extractDir,
    gzip: true,
  });
}

async function findBinary(searchDir, binaryName) {
  const entries = await fs.readdir(searchDir, { withFileTypes: true });

  for (const entry of entries) {
    const fullPath = path.join(searchDir, entry.name);

    if (entry.isFile() && entry.name === binaryName) {
      return fullPath;
    }

    if (entry.isDirectory()) {
      const nested = await findBinary(fullPath, binaryName);
      if (nested) {
        return nested;
      }
    }
  }

  return null;
}

export async function installNativeBinary({
  rootDir = getRepositoryRoot(),
  version,
  logger = console,
} = {}) {
  const resolvedVersion = version ?? (await readPackageVersion(rootDir));
  const info = getPlatformAssetInfo();
  const vendorDirectory = getVendorDirectory(rootDir);
  const binaryPath = getInstalledBinaryPath(rootDir);
  const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'vectizeit-npm-'));
  const archivePath = path.join(tempRoot, info.assetName);
  const checksumPath = path.join(tempRoot, RELEASE_CHECKSUM_FILE);
  const extractDir = path.join(tempRoot, 'extract');

  await fs.mkdir(extractDir, { recursive: true });

  try {
    const assetUrl = resolveDownloadUrl(resolvedVersion, info.assetName);
    const checksumUrl = resolveDownloadUrl(
      resolvedVersion,
      RELEASE_CHECKSUM_FILE
    );

    logger.log(
      `[vectizeit] Downloading ${info.assetName} for ${info.target}...`
    );
    await downloadToFile(assetUrl, archivePath);
    await downloadToFile(checksumUrl, checksumPath);
    await verifyChecksum(archivePath, checksumPath, info.assetName);
    await extractArchive(archivePath, extractDir, info.archiveExtension);

    const extractedBinary = await findBinary(extractDir, info.binaryName);
    if (!extractedBinary) {
      throw new Error(
        `Archive ${info.assetName} does not contain ${info.binaryName}.`
      );
    }

    await fs.rm(vendorDirectory, { force: true, recursive: true });
    await fs.mkdir(vendorDirectory, { recursive: true });
    await fs.copyFile(extractedBinary, binaryPath);

    if (info.binaryName === DEFAULT_BINARY_NAME) {
      await fs.chmod(binaryPath, 0o755);
    }

    logger.log(`[vectizeit] Installed ${info.binaryName} to ${binaryPath}.`);
    return binaryPath;
  } finally {
    await fs.rm(tempRoot, { force: true, recursive: true });
  }
}

export async function removeInstalledBinary(rootDir = getRepositoryRoot()) {
  await fs.rm(path.join(rootDir, 'vendor'), { force: true, recursive: true });
}

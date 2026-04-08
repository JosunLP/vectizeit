import path from 'node:path';
import process from 'node:process';

import { DEFAULT_BINARY_NAME } from './github.mjs';

const PLATFORM_MATRIX = {
  'darwin:x64': {
    target: 'x86_64-apple-darwin',
    archiveExtension: 'tar.gz',
    binaryName: DEFAULT_BINARY_NAME,
  },
  'darwin:arm64': {
    target: 'aarch64-apple-darwin',
    archiveExtension: 'tar.gz',
    binaryName: DEFAULT_BINARY_NAME,
  },
  'linux:x64': {
    target: 'x86_64-unknown-linux-musl',
    archiveExtension: 'tar.gz',
    binaryName: DEFAULT_BINARY_NAME,
  },
  'win32:x64': {
    target: 'x86_64-pc-windows-msvc',
    archiveExtension: 'zip',
    binaryName: `${DEFAULT_BINARY_NAME}.exe`,
  },
};

export function getPlatformAssetInfo(
  platform = process.platform,
  arch = process.arch
) {
  const key = `${platform}:${arch}`;
  const match = PLATFORM_MATRIX[key];

  if (!match) {
    throw new Error(
      `Unsupported platform '${platform}' / '${arch}'. Supported targets: ${Object.keys(
        PLATFORM_MATRIX
      ).join(', ')}.`
    );
  }

  return {
    ...match,
    assetName: `vectizeit-${match.target}.${match.archiveExtension}`,
  };
}

export function getVendorDirectory(
  rootDir,
  platform = process.platform,
  arch = process.arch
) {
  return path.join(
    rootDir,
    'vendor',
    getPlatformAssetInfo(platform, arch).target
  );
}

export function getInstalledBinaryPath(
  rootDir,
  platform = process.platform,
  arch = process.arch
) {
  const info = getPlatformAssetInfo(platform, arch);
  return path.join(rootDir, 'vendor', info.target, info.binaryName);
}

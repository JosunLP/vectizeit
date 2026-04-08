import assert from 'node:assert/strict';
import path from 'node:path';
import test from 'node:test';

import {
  getInstalledBinaryPath,
  getPlatformAssetInfo,
} from '../lib/platform.mjs';

test('platform mapping returns the expected release asset metadata', () => {
  assert.deepEqual(getPlatformAssetInfo('linux', 'x64'), {
    target: 'x86_64-unknown-linux-musl',
    archiveExtension: 'tar.gz',
    binaryName: 'trace',
    assetName: 'vectizeit-x86_64-unknown-linux-musl.tar.gz',
  });

  assert.deepEqual(getPlatformAssetInfo('darwin', 'arm64'), {
    target: 'aarch64-apple-darwin',
    archiveExtension: 'tar.gz',
    binaryName: 'trace',
    assetName: 'vectizeit-aarch64-apple-darwin.tar.gz',
  });

  assert.deepEqual(getPlatformAssetInfo('win32', 'x64'), {
    target: 'x86_64-pc-windows-msvc',
    archiveExtension: 'zip',
    binaryName: 'trace.exe',
    assetName: 'vectizeit-x86_64-pc-windows-msvc.zip',
  });
});

test('unsupported platforms fail with a clear error', () => {
  assert.throws(
    () => getPlatformAssetInfo('freebsd', 'x64'),
    /Unsupported platform 'freebsd' \/ 'x64'/
  );
});

test('installed binary path resolves inside the vendor directory', () => {
  assert.equal(
    getInstalledBinaryPath('/repo', 'darwin', 'x64'),
    path.join('/repo', 'vendor', 'x86_64-apple-darwin', 'trace')
  );
});

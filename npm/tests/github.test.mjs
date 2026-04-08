import assert from 'node:assert/strict';
import test from 'node:test';

import {
  buildReleaseDownloadUrl,
  normalizeVersion,
  RELEASE_BASE_URL,
  RELEASE_CHECKSUM_FILE,
} from '../lib/github.mjs';

test('normalizeVersion preserves latest and adds v-prefix', () => {
  assert.equal(normalizeVersion('latest'), 'latest');
  assert.equal(normalizeVersion('0.2.3'), 'v0.2.3');
  assert.equal(normalizeVersion('v1.2.3'), 'v1.2.3');
});

test('buildReleaseDownloadUrl supports latest and tagged releases', () => {
  assert.equal(
    buildReleaseDownloadUrl('latest', RELEASE_CHECKSUM_FILE),
    `${RELEASE_BASE_URL}/latest/download/${RELEASE_CHECKSUM_FILE}`
  );
  assert.equal(
    buildReleaseDownloadUrl(
      '0.1.0',
      'vectizeit-x86_64-unknown-linux-musl.tar.gz'
    ),
    `${RELEASE_BASE_URL}/download/v0.1.0/vectizeit-x86_64-unknown-linux-musl.tar.gz`
  );
});

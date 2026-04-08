export const REPOSITORY_OWNER = 'JosunLP';
export const REPOSITORY_NAME = 'vectizeit';
export const REPOSITORY_SLUG = `${REPOSITORY_OWNER}/${REPOSITORY_NAME}`;
export const RELEASE_BASE_URL = `https://github.com/${REPOSITORY_SLUG}/releases`;
export const RELEASE_CHECKSUM_FILE = 'checksums.txt';
export const DEFAULT_BINARY_NAME = 'trace';

export function normalizeVersion(version) {
  if (!version || version === 'latest') {
    return 'latest';
  }

  return version.startsWith('v') ? version : `v${version}`;
}

export function buildReleaseDownloadUrl(version, assetName) {
  const normalizedVersion = normalizeVersion(version);

  if (normalizedVersion === 'latest') {
    return `${RELEASE_BASE_URL}/latest/download/${assetName}`;
  }

  return `${RELEASE_BASE_URL}/download/${normalizedVersion}/${assetName}`;
}

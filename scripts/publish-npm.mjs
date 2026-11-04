import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { lstatSync, readFileSync } from 'node:fs';
import path from 'node:path';

const [directory, version, sourceCommit, mode, releasePath, ...extra] = process.argv.slice(2);
assert(directory && version && sourceCommit && extra.length === 0,
  'Usage: node scripts/publish-npm.mjs DIRECTORY VERSION SOURCE_COMMIT [--check-release GITHUB_RELEASE_JSON | --publish GITHUB_RELEASE_JSON | --publish-local]');
assert([undefined, '--check-release', '--publish', '--publish-local'].includes(mode), 'Unknown publish mode');
const fromRelease = mode === '--check-release' || mode === '--publish';
assert.equal(Boolean(releasePath), fromRelease, 'Release validation requires GitHub Release metadata');
const channel = version.match(/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-(alpha|beta)\.(0|[1-9]\d*))?$/);
assert(channel, 'Release version must be X.Y.Z, X.Y.Z-alpha.N or X.Y.Z-beta.N');
const tag = channel[4] ?? 'latest';
assert(mode !== '--publish-local' || tag === 'alpha',
  'Only alphas publish from a local build; beta and stable come from a published GitHub Release');
assert(/^[a-f0-9]{40}$/.test(sourceCommit), 'Invalid source commit');

const root = path.resolve(directory);
const manifestPath = path.join(root, 'release-manifest.json');
assert(lstatSync(manifestPath).isFile(), 'Release manifest must be a regular file');
const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
assert.equal(manifest.version, version, 'Release version mismatch');
assert.equal(manifest.sourceCommit, sourceCommit, 'Release source mismatch');

const platforms = [
  ['darwin-arm64', 'darwin', 'arm64'],
  ['darwin-x64', 'darwin', 'x64'],
  ['linux-arm64', 'linux', 'arm64'],
  ['linux-x64', 'linux', 'x64'],
  ['windows-arm64', 'win32', 'arm64'],
  ['windows-x64', 'win32', 'x64'],
];
assert(Array.isArray(manifest.npmPackages), 'Missing npm package manifest');
const shipped = platforms.filter(([platform]) => manifest.npmPackages.some(({ name }) => name === `dimcode-${platform}`));
if (fromRelease && tag === 'alpha') assert.equal(manifest.account, true, 'CI alphas require the Dim account build');
assert(shipped.length === platforms.length || (!fromRelease && tag === 'alpha' && shipped.length > 0),
  'Release must contain all six platform packages and one wrapper');
const wrapper = tag === 'alpha' ? 'dimcode-alpha' : 'dimcode';
const names = [...shipped.map(([platform]) => `dimcode-${platform}`), 'dimcode-web', wrapper];
assert.deepEqual(manifest.npmPackages.map(({ name }) => name).sort(), [...names].sort(),
  'Release must contain exactly its platform packages, the Web workspace and one wrapper');

if (fromRelease) {
  const release = JSON.parse(readFileSync(releasePath, 'utf8'));
  assert.equal(release.draft, false, 'Cannot publish npm packages from a draft Release');
  assert(typeof release.published_at === 'string' && release.published_at.length > 0, 'GitHub Release must be published');
  assert.equal(release.tag_name, `v${version}`, 'GitHub Release tag mismatch');
  assert.deepEqual(release.assets.filter(({ name }) => name.endsWith('.tgz') || name === 'release-manifest.json')
    .map(({ name }) => name).sort(), [...manifest.npmPackages.map(({ file }) => file), 'release-manifest.json'].sort(),
    'GitHub Release must contain the exact npm packages and manifest');
}

const archives = [];
for (const name of names) {
  const entry = manifest.npmPackages.find((entry) => entry.name === name);
  assert.equal(entry.file, `${name}-${version}.tgz`, 'Unexpected package filename');
  assert(typeof entry.sha256 === 'string' && /^[a-f0-9]{64}$/.test(entry.sha256),
    'Invalid package digest');
  const archive = path.join(root, entry.file);
  assert(lstatSync(archive).isFile(), 'Package archive must be a regular file');
  assert.equal(createHash('sha256').update(readFileSync(archive)).digest('hex'), entry.sha256,
    `Package digest mismatch: ${name}`);
  const packed = spawnSync('tar', ['-xOzf', archive, 'package/package.json'], { encoding: 'utf8' });
  assert.ifError(packed.error);
  assert.equal(packed.status, 0, `Cannot read package metadata: ${name}`);
  const metadata = JSON.parse(packed.stdout);
  assert.equal(metadata.name, name, 'Packed package name mismatch');
  assert.equal(metadata.version, version, 'Packed package version mismatch');
  assert.equal(metadata.repository?.url, 'git+https://github.com/arcships/dim.git',
    'Packed package repository mismatch');
  if (name === wrapper) {
    assert.deepEqual(metadata.optionalDependencies,
      Object.fromEntries(names.slice(0, -1).map((name) => [name, version])),
      'Wrapper must reference the exact platform package versions');
    if (tag === 'alpha') {
      assert.deepEqual(Object.keys(metadata.bin ?? {}), ['dima'], 'An alpha installs only the dima command');
      assert(typeof metadata.bin.dima === 'string' && metadata.bin.dima.length > 0, 'Missing dima command');
    } else {
      assert.equal(typeof metadata.bin?.dim, 'string', 'Missing dim command');
      assert(metadata.bin.dim.length > 0 && metadata.bin.dimcode === metadata.bin.dim,
        'dim and dimcode must use the same wrapper');
    }
  } else if (name === 'dimcode-web') {
    assert.equal(metadata.os, undefined, 'The Web workspace package is platform-independent');
    assert.equal(metadata.cpu, undefined, 'The Web workspace package is platform-independent');
  } else {
    const [, os, cpu] = platforms.find(([platform]) => name === `dimcode-${platform}`);
    assert.deepEqual(metadata.os, [os], 'Packed package OS mismatch');
    assert.deepEqual(metadata.cpu, [cpu], 'Packed package CPU mismatch');
  }
  archives.push(archive);
}

function registryField(spec, field) {
  const result = spawnSync('npm', ['view', spec, field, '--json', '--registry', 'https://registry.npmjs.org'], { encoding: 'utf8' });
  assert.ifError(result.error);
  let data;
  try { data = JSON.parse(result.stdout); }
  catch { assert.fail('Cannot verify registry state: ' + spec); }
  if (result.status === 0) return data;
  assert.equal(data?.error?.code, 'E404', 'Cannot verify registry state: ' + spec);
}

console.log(`Validated ${archives.length} npm packages for ${version} (${sourceCommit}).`);
if (mode) {
  if (tag === 'alpha') {
    const versions = registryField('dimcode', 'versions') ?? [];
    const core = (value) => value.split('-')[0].split('.').map(Number);
    const below = (left, right) => {
      const index = left.findIndex((part, index) => part !== right[index]);
      return index === -1 || left[index] < right[index];
    };
    const betas = [versions].flat().filter((value) => /^\d+\.\d+\.\d+-beta\.\d+$/.test(value));
    assert(betas.length === 0 || betas.some((beta) => below(core(version), core(beta))),
      `Alpha ${version} must stay below the newest dimcode beta so beta installs do not upgrade to it`);
  }
  const pending = [];
  for (const [index, archive] of archives.entries()) {
    const name = names[index];
    if (fromRelease && tag === 'alpha') {
      const current = registryField(name + '@alpha', 'version');
      if (current !== undefined) {
        assert(/^\d+\.\d+\.\d+-alpha\.\d+$/.test(current), `Unexpected alpha tag on ${name}: ${current}`);
        assert(current.localeCompare(version, 'en', { numeric: true }) <= 0, `${name}@alpha already points to newer ${current}`);
      }
    }
    const metadata = registryField(name + '@' + version, 'dist');
    if (metadata !== undefined) {
      const integrity = `sha512-${createHash('sha512').update(readFileSync(archive)).digest('base64')}`;
      assert.equal(metadata.integrity, integrity, `Existing version has different content: ${name}`);
      console.log(`Already published with matching integrity: ${name}@${version}`);
    } else {
      pending.push(archive);
    }
  }
  if (mode !== '--check-release') {
    for (const archive of pending) {
      const published = spawnSync('npm', ['publish', archive, '--access', 'public', '--tag', tag,
        '--ignore-scripts', '--registry', 'https://registry.npmjs.org'], { stdio: 'inherit' });
      assert.ifError(published.error);
      assert.equal(published.status, 0, `npm publish failed: ${path.basename(archive)}`);
    }
  }
}

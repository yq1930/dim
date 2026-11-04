import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const script = fileURLToPath(new URL('./publish-npm.mjs', import.meta.url));
const sourceCommit = 'a'.repeat(40);
const platforms = [
  ['darwin-arm64', 'darwin', 'arm64'], ['darwin-x64', 'darwin', 'x64'],
  ['linux-arm64', 'linux', 'arm64'], ['linux-x64', 'linux', 'x64'],
  ['windows-arm64', 'win32', 'arm64'], ['windows-x64', 'win32', 'x64'],
];

function fixture(t, version = '1.2.3', shipped = platforms) {
  const root = mkdtempSync(path.join(os.tmpdir(), 'dim-npm-release-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const alpha = version.includes('-alpha.');
  const wrapper = alpha ? 'dimcode-alpha' : 'dimcode';
  const names = [...shipped.map(([platform]) => `dimcode-${platform}`), 'dimcode-web', wrapper];
  const manifest = { sourceCommit, version, account: true, npmPackages: [] };
  for (const name of names) {
    const directory = path.join(root, name);
    mkdirSync(path.join(directory, 'package'), { recursive: true });
    const metadata = {
      name, version, repository: { type: 'git', url: 'git+https://github.com/arcships/dim.git' },
    };
    if (name === wrapper) {
      metadata.optionalDependencies = Object.fromEntries(names.slice(0, -1).map((name) => [name, version]));
      metadata.bin = alpha ? { dima: 'bin/dim.mjs' } : { dim: 'bin/dim.mjs', dimcode: 'bin/dim.mjs' };
    } else if (name === 'dimcode-web') {
    } else {
      const [, os, cpu] = platforms.find(([platform]) => name === `dimcode-${platform}`);
      metadata.os = [os];
      metadata.cpu = [cpu];
    }
    writeFileSync(path.join(directory, 'package/package.json'), JSON.stringify(metadata));
    const file = `${name}-${version}.tgz`;
    const packed = spawnSync('tar', ['-czf', path.join(root, file), '-C', directory, 'package']);
    assert.equal(packed.status, 0);
    manifest.npmPackages.push({ name, file,
      sha256: createHash('sha256').update(readFileSync(path.join(root, file))).digest('hex') });
  }
  const bin = path.join(root, 'bin');
  mkdirSync(bin);
  writeFileSync(path.join(bin, 'npm'), `#!/bin/sh
if [ "$1" = view ]; then
  if [ "$2" = dimcode ] && [ "$3" = versions ]; then printf '%s' "$REGISTRY_VERSIONS"; exit 0; fi
  if [ "$REGISTRY_MODE" = newer ] && [ "$2" = dimcode-web@alpha ]; then printf '"1.2.3-alpha.10"'; exit 0; fi
  if [ "$REGISTRY_MODE" = fail ]; then printf '{"error":{"code":"E401"}}'; exit 1; fi
  if [ "$REGISTRY_MODE" = mismatch ]; then printf '{"integrity":"sha512-wrong"}'; exit 0; fi
  if [ "$REGISTRY_MODE" = existing ] && [ "$2" = dimcode-darwin-arm64@1.2.3 ]; then
    printf '{"integrity":"%s"}' "$EXISTING_INTEGRITY"; exit 0
  fi
  printf '{"error":{"code":"E404"}}'; exit 1
fi
printf "%s\\n" "$*" >> "$PUBLISH_LOG"
`, { mode: 0o755 });
  const log = path.join(root, 'published.log');
  writeFileSync(log, '');
  const release = { tag_name: `v${version}`, draft: false, published_at: '2026-10-14T00:00:00Z',
    assets: [...manifest.npmPackages.map(({ file }) => ({ name: file })), { name: 'release-manifest.json' }] };
  const releasePath = path.join(root, 'github-release.json');
  function run({ publish = false, checkRelease = false, local = false, commit = sourceCommit, registry = 'missing', versions = [] } = {}) {
    writeFileSync(path.join(root, 'release-manifest.json'), JSON.stringify(manifest));
    writeFileSync(releasePath, JSON.stringify(release));
    return spawnSync(process.execPath, [script, root, version, commit, ...(local ? ['--publish-local'] : publish ? ['--publish', releasePath] : checkRelease ? ['--check-release', releasePath] : [])], {
      encoding: 'utf8', env: { ...process.env, PATH: `${bin}${path.delimiter}${process.env.PATH}`, PUBLISH_LOG: log,
        REGISTRY_MODE: registry, REGISTRY_VERSIONS: JSON.stringify(versions), EXISTING_INTEGRITY: 'sha512-' + createHash('sha512').update(readFileSync(path.join(root, manifest.npmPackages[0].file))).digest('base64') },
    });
  }
  return { root, names, manifest, release, log, run };
}

test('validates Release and registry state without publishing', (t) => {
  const { release, manifest, log, run } = fixture(t, '1.2.3-alpha.2');
  assert.equal(run({ checkRelease: true }).status, 0);
  assert.notEqual(run({ publish: true, registry: 'newer' }).status, 0);
  for (const registry of ['fail', 'mismatch']) assert.notEqual(run({ checkRelease: true, registry }).status, 0);
  assert.notEqual(run({ checkRelease: true, versions: ['1.2.2-beta.0'] }).status, 0);
  manifest.account = false;
  assert.notEqual(run({ publish: true }).status, 0);
  manifest.account = true;
  release.draft = true;
  assert.notEqual(run({ checkRelease: true }).status, 0);
  assert.equal(readFileSync(log, 'utf8'), '');
});

test('rejects draft, unpublished, mismatched and incomplete Releases before registry writes', (t) => {
  const fixtureRelease = fixture(t);
  const { release, log, run } = fixtureRelease;
  release.draft = true;
  assert.notEqual(run({ publish: true }).status, 0);
  release.draft = false;
  release.published_at = null;
  assert.notEqual(run({ publish: true }).status, 0);
  release.published_at = '2026-10-14T00:00:00Z';
  release.tag_name = 'v1.2.4';
  assert.notEqual(run({ publish: true }).status, 0);
  release.tag_name = 'v1.2.3';
  release.assets.pop();
  assert.notEqual(run({ publish: true }).status, 0);
  assert.equal(readFileSync(log, 'utf8'), '');
});

test('validates without publishing and publishes the wrapper after every platform', (t) => {
  const release = fixture(t);
  assert.equal(release.run().status, 0);
  assert.equal(readFileSync(release.log, 'utf8'), '');
  const published = release.run({ publish: true });
  assert.equal(published.status, 0, published.stderr);
  const calls = readFileSync(release.log, 'utf8').trim().split('\n');
  assert.equal(calls.length, 8);
  for (const [index, name] of release.names.entries()) {
    assert.equal(calls[index], `publish ${path.join(release.root, `${name}-1.2.3.tgz`)} --access public --tag latest --ignore-scripts --registry https://registry.npmjs.org`);
  }
});

test('only an alpha ships a subset of platforms, with a wrapper referencing exactly them', (t) => {
  const darwin = platforms.slice(0, 1);
  const alpha = fixture(t, '1.2.3-alpha.0', darwin);
  const published = alpha.run({ local: true, versions: ['1.2.3-beta.0'] });
  assert.equal(published.status, 0, published.stderr);
  assert.deepEqual(readFileSync(alpha.log, 'utf8').trim().split('\n').map((call) => path.basename(call.split(' ')[1])),
    ['dimcode-darwin-arm64-1.2.3-alpha.0.tgz', 'dimcode-web-1.2.3-alpha.0.tgz', 'dimcode-alpha-1.2.3-alpha.0.tgz']);
  assert.notEqual(alpha.run({ checkRelease: true }).status, 0);
  const stable = fixture(t, '1.2.3', darwin);
  assert.notEqual(stable.run().status, 0);
  assert.equal(readFileSync(stable.log, 'utf8'), '');
});

test('rejects a corrupted wrapper before any platform is published', (t) => {
  const release = fixture(t);
  writeFileSync(path.join(release.root, 'dimcode-1.2.3.tgz'), 'corrupt');
  assert.notEqual(release.run({ publish: true }).status, 0);
  assert.equal(readFileSync(release.log, 'utf8'), '');
});

test('rejects wrong source commits, missing platforms and mismatched packed versions', (t) => {
  const release = fixture(t);
  assert.notEqual(release.run({ publish: true, commit: 'b'.repeat(40) }).status, 0);
  const platform = release.manifest.npmPackages.shift();
  assert.notEqual(release.run({ publish: true }).status, 0);
  release.manifest.npmPackages.unshift(platform);
  const packageDirectory = path.join(release.root, 'dimcode');
  const metadataPath = path.join(packageDirectory, 'package/package.json');
  const metadata = JSON.parse(readFileSync(metadataPath, 'utf8'));
  metadata.version = '1.2.4';
  writeFileSync(metadataPath, JSON.stringify(metadata));
  const wrapper = release.manifest.npmPackages.at(-1);
  const archive = path.join(release.root, wrapper.file);
  assert.equal(spawnSync('tar', ['-czf', archive, '-C', packageDirectory, 'package']).status, 0);
  wrapper.sha256 = createHash('sha256').update(readFileSync(archive)).digest('hex');
  assert.notEqual(release.run({ publish: true }).status, 0);
  assert.equal(readFileSync(release.log, 'utf8'), '');
});

test('an alpha wrapper installs only dima, so it never replaces dim', (t) => {
  const release = fixture(t, '1.2.3-alpha.1');
  assert.equal(release.manifest.npmPackages.at(-1).name, 'dimcode-alpha');
  assert.equal(release.run().status, 0);
  const packageDirectory = path.join(release.root, 'dimcode-alpha');
  const metadataPath = path.join(packageDirectory, 'package/package.json');
  const metadata = JSON.parse(readFileSync(metadataPath, 'utf8'));
  metadata.bin.dim = metadata.bin.dima;
  writeFileSync(metadataPath, JSON.stringify(metadata));
  const wrapper = release.manifest.npmPackages.at(-1);
  const archive = path.join(release.root, wrapper.file);
  assert.equal(spawnSync('tar', ['-czf', archive, '-C', packageDirectory, 'package']).status, 0);
  wrapper.sha256 = createHash('sha256').update(readFileSync(archive)).digest('hex');
  assert.notEqual(release.run().status, 0);
});

test('each channel publishes to its own dist-tag', (t) => {
  for (const [version, tag] of [['1.2.3-beta.1', 'beta'], ['1.2.3-alpha.1', 'alpha']]) {
    const release = fixture(t, version);
    const published = release.run({ publish: true });
    assert.equal(published.status, 0, published.stderr);
    const calls = readFileSync(release.log, 'utf8').trim().split('\n');
    assert.equal(calls.length, 8);
    for (const call of calls) assert(call.endsWith(`--tag ${tag} --ignore-scripts --registry https://registry.npmjs.org`));
  }
  assert.notEqual(fixture(t, '1.2.3-rc.1').run().status, 0);
});

test('publishes only alphas from a local build', (t) => {
  const beta = fixture(t, '1.2.3-beta.1');
  assert.notEqual(beta.run({ local: true }).status, 0);
  assert.equal(readFileSync(beta.log, 'utf8'), '');
  const alpha = fixture(t, '1.2.3-alpha.1');
  const published = alpha.run({ local: true });
  assert.equal(published.status, 0, published.stderr);
  const calls = readFileSync(alpha.log, 'utf8').trim().split('\n');
  assert.equal(calls.length, 8);
  for (const call of calls) assert(call.includes('--tag alpha'));
});

test('rejects an alpha that would outrank the newest beta before registry writes', (t) => {
  const release = fixture(t, '1.2.3-alpha.1');
  for (const versions of [['1.2.2', '1.2.2-beta.4'], ['1.2.3-beta.0', '2.0.0']]) {
    const allowed = versions.includes('1.2.3-beta.0');
    assert.equal(release.run({ publish: true, versions }).status === 0, allowed);
    if (!allowed) assert.equal(readFileSync(release.log, 'utf8'), '');
  }
});

 test('retries matching published packages and stops before writes on mismatches or registry errors', (t) => {
  const release = fixture(t);
  const resumed = release.run({publish:true,registry:'existing'});
  assert.equal(resumed.status,0,resumed.stderr);
  const calls = readFileSync(release.log,'utf8').trim().split('\n');
  assert.equal(calls.length,7);
  assert(calls[0].includes('dimcode-darwin-x64-1.2.3.tgz'));
  writeFileSync(release.log,'');
  assert.notEqual(release.run({publish:true,registry:'mismatch'}).status,0);
  assert.equal(readFileSync(release.log,'utf8'),'');
  assert.notEqual(release.run({publish:true,registry:'fail'}).status,0);
  assert.equal(readFileSync(release.log,'utf8'),'');
});

test('verifies the registry wrapper and syncs each published alpha package to the mirror', (t) => {
  const { root, manifest, names } = fixture(t, '1.2.3-alpha.1');
  const receipt = path.join(root, 'npm-publication.json');
  writeFileSync(receipt, JSON.stringify({ ...manifest, legacyTags: { latest: '1.2.2' } }));
  const preload = path.join(root, 'registry.mjs');
  writeFileSync(preload, `
    import assert from 'node:assert/strict';
    const [mode, failure] = [process.env.REGISTRY, process.env.FAILURE];
    const names = ${JSON.stringify(names)};
    const version = ${JSON.stringify(manifest.version)};
    const requests = [];
    globalThis.fetch = async (url, options) => {
      const { hostname, pathname } = new URL(url);
      assert.equal(hostname, mode === 'npm' ? 'registry.npmjs.org' : 'registry.npmmirror.com');
      if (options.method === 'PUT') {
        assert.equal(mode, 'mirror');
        requests.push(pathname);
        return Response.json({ ok: failure !== 'sync' });
      }
      if (pathname === '/-/package/dimcode/dist-tags') return Response.json({ latest: failure === 'tags' ? '1.2.3' : '1.2.2' });
      const [, name, tag] = pathname.split('/');
      assert(names.includes(name));
      assert.equal(tag, mode === 'mirror' && name !== 'dimcode-alpha' ? version : 'alpha');
      const metadata = { name, version };
      if (name === 'dimcode-alpha') {
        metadata.bin = { [failure === 'bin' ? 'dim' : 'dima']: 'bin/dim.mjs' };
        metadata.optionalDependencies = Object.fromEntries(names.slice(0, -1).map(n => [n, version]));
      }
      return Response.json(metadata);
    };
    process.on('beforeExit', () => {
      if (!failure && mode === 'mirror') assert.deepEqual(requests, names.map(name => '/-/package/' + name + '/syncs'));
    });
  `);
  const check = fileURLToPath(new URL('./check-alpha-registry.mjs', import.meta.url));
  for (const [mode, failure] of [['npm', ''], ['mirror', ''], ['npm', 'tags'], ['npm', 'bin'], ['mirror', 'sync']]) {
    const result = spawnSync(process.execPath, ['--import', preload, check, receipt, mode], {
      encoding: 'utf8', env: { ...process.env, REGISTRY: mode, FAILURE: failure },
    });
    assert.equal(result.status === 0, !failure, result.stderr);
  }
});

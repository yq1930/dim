import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { setTimeout as delay } from 'node:timers/promises';

const [receipt, mode] = process.argv.slice(2);
assert(receipt && ['npm', 'mirror'].includes(mode), 'Usage: node scripts/check-alpha-registry.mjs RECEIPT npm|mirror');
const { version, npmPackages, legacyTags } = JSON.parse(readFileSync(receipt));
const wrapper = 'dimcode-alpha';
const names = npmPackages.map(({ name }) => name);
const registry = mode === 'mirror' ? 'https://registry.npmmirror.com' : 'https://registry.npmjs.org';
const dependencies = Object.fromEntries(names.filter(name => name !== wrapper).map(name => [name, version]));

if (mode === 'mirror') {
  for (const name of names) {
    const response = await fetch(`${registry}/-/package/${name}/syncs`, {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ skipDependencies: true }),
      signal: AbortSignal.timeout(30_000),
    });
    assert(response.ok && (await response.json()).ok, `Mirror sync failed for ${name} (HTTP ${response.status})`);
  }
}

const pending = new Set(names);
const deadline = Date.now() + 600_000;
while (pending.size) {
  await Promise.all([...pending].map(async name => {
    const ref = mode === 'mirror' && name !== wrapper ? version : 'alpha';
    const url = `${registry}/${name}/${ref}`;
    let response;
    try {
      response = await fetch(url, { signal: AbortSignal.timeout(Math.min(15_000, Math.max(1, deadline - Date.now()))) });
    } catch (error) {
      console.error(`${url}: ${error.message}`);
      return;
    }
    if (response.status === 404 || response.status === 429 || response.status >= 500) return;
    assert(response.ok, `${url} returned HTTP ${response.status}`);
    const metadata = await response.json();
    if (metadata.version !== version) return;
    assert.equal(metadata.name, name);
    if (name === wrapper) {
      assert.deepEqual(Object.keys(metadata.bin), ['dima']);
      assert.deepEqual(metadata.optionalDependencies, dependencies);
    }
    pending.delete(name);
    console.log(`Verified ${name}@${version} on ${registry}`);
  }));
  if (pending.size) {
    assert(Date.now() < deadline, `Timed out waiting for ${version} on ${registry}: ${[...pending].join(', ')}`);
    await delay(Math.min(15_000, deadline - Date.now()));
  }
}
if (mode === 'npm') {
  const response = await fetch(`${registry}/-/package/dimcode/dist-tags`, { signal: AbortSignal.timeout(15_000) });
  assert(response.ok, `Cannot verify dimcode dist-tags (HTTP ${response.status})`);
  assert.deepEqual(await response.json(), legacyTags, 'dimcode dist-tags changed during publication');
}

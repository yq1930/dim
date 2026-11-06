import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';

const args = process.argv.slice(2);
assert(args.length === 0 || (args.length === 1 && args[0] === '--debug'),
  'Usage: node scripts/build-macos-app.mjs [--debug]');
assert(process.platform === 'darwin' && process.arch === 'arm64',
  'Build Lacus.app on an Apple Silicon Mac.');

const repo = path.resolve(import.meta.dirname, '..');
const desktop = path.join(repo, 'desktop');
const stage = path.join(repo, 'staging/macos-app');
const profile = args.includes('--debug') ? 'debug' : 'release';
const target = 'aarch64-apple-darwin';
const env = { ...process.env, CARGO_INCREMENTAL: '0' };
delete env.RUSTC_WRAPPER;

function run(command, parameters, cwd = repo) {
  const result = spawnSync(command, parameters, { cwd, env, stdio: 'inherit' });
  assert.ifError(result.error);
  assert.equal(result.status, 0, `${command} ${parameters.join(' ')} failed`);
}

function read(command, parameters, cwd = repo) {
  return execFileSync(command, parameters, { cwd, env, encoding: 'utf8' }).trim();
}

const sourceBranch = read('git', ['branch', '--show-current']);
assert.equal(sourceBranch, 'master', 'Build Lacus.app from master; switch branches before building.');
assert.equal(read('git', ['status', '--porcelain']), '', 'Build Lacus.app from a clean worktree.');
run('git', ['fetch', '--no-tags', 'origin', 'refs/heads/master:refs/remotes/origin/master']);
const sourceCommit = read('git', ['rev-parse', 'origin/master']);

function verifySource() {
  assert.equal(read('git', ['branch', '--show-current']), sourceBranch,
    'The build branch changed; build again from master.');
  assert.equal(read('git', ['rev-parse', 'HEAD']), sourceCommit,
    'Local master must match origin/master. Update with git merge --ff-only origin/master; do not build unpushed commits.');
  assert.equal(read('git', ['status', '--porcelain']), '', 'The build worktree must remain clean.');
}

verifySource();
const version = JSON.parse(readFileSync(path.join(repo, 'npm/package.json'), 'utf8')).version;
assert.equal(JSON.parse(readFileSync(path.join(desktop, 'package.json'), 'utf8')).version, version,
  'Keep desktop/package.json and npm/package.json at the same product version.');

run('npm', ['ci', '--prefix', desktop, '--ignore-scripts', '--no-audit', '--no-fund', '--registry=https://registry.npmjs.org']);
const binary = path.join(repo, 'target', profile, 'dimcode');

rmSync(stage, { recursive: true, force: true });
mkdirSync(stage, { recursive: true });

run('npm', ['ci', '--prefix', path.join(repo, 'web'), '--ignore-scripts', '--no-audit', '--no-fund', '--registry=https://registry.npmjs.org']);
run('npm', ['run', 'build', '--prefix', path.join(repo, 'web'), '--', '--outDir', path.join(stage, 'web')]);
assert(existsSync(path.join(stage, 'web/THIRD-PARTY-NOTICES.txt')), 'The frontend must carry its license notices.');
const protocolVersion = Number(readFileSync(path.join(repo, 'crates/dim-app-server/src/protocol.rs'), 'utf8')
  .match(/pub const PROTOCOL_VERSION: u32 = (\d+);/)[1]);
writeFileSync(path.join(stage, 'web/dimcode-web.json'), `${JSON.stringify({ version, protocolVersion, sourceCommit }, null, 2)}\n`);

run('node', ['scripts/stage-plugins.mjs', path.join(stage, 'plugins-payload')]);
mkdirSync(path.join(stage, 'boot'));
writeFileSync(path.join(stage, 'boot/index.html'), readFileSync(path.join(repo, 'web/index.html'), 'utf8')
  .replace(/\s*<script type="module" src="\/src\/main\.ts"><\/script>/, '')
  .replace('aria-label="Loading"', 'aria-label="正在启动野塘漫水"'));
run('npm', ['exec', '--', 'tauri', 'icon', path.join(desktop, 'icon.svg'), '--output', path.join(stage, 'icons')], desktop);

mkdirSync(path.join(stage, 'licenses'));
for (const file of ['LICENSE', 'NOTICE']) copyFileSync(path.join(repo, file), path.join(stage, 'licenses', file));
copyFileSync(path.join(repo, 'vendor/aimux/LICENSE'), path.join(stage, 'licenses/aimux-LICENSE'));
for (const file of ['LICENSE', 'NOTICE']) {
  copyFileSync(path.join(repo, 'vendor/grok-build', file), path.join(stage, 'licenses', `grok-build-${file}`));
}
run('cargo', ['build', '--locked', '-p', 'dimcode', ...(profile === 'release' ? ['--release'] : [])]);
assert.equal(read('lipo', ['-archs', binary]), 'arm64', 'The backend must be Apple Silicon native.');
copyFileSync(binary, path.join(stage, `dimcode-${target}`));
chmodSync(path.join(stage, `dimcode-${target}`), 0o755);
verifySource();
writeFileSync(path.join(stage, 'build-info.json'), `${JSON.stringify({
  version, sourceCommit, sourceBranch, sourceRemote: 'origin', sourceRef: 'refs/heads/master', sourceDirty: false,
  target, profile, backendVersion: read(binary, ['--version']), builtAt: new Date().toISOString(),
}, null, 2)}\n`);

run('npm', ['exec', '--', 'tauri', 'build', '--bundles', 'app', '--target', target,
  ...(profile === 'debug' ? ['--debug'] : []), '--', '--locked'], desktop);
const bundle = path.join(desktop, 'src-tauri/target', target, profile, 'bundle/macos/Lacus.app');
assert(existsSync(bundle), 'Tauri did not produce Lacus.app.');
for (const name of ['dim-desktop', 'dimcode']) {
  assert.equal(read('lipo', ['-archs', path.join(bundle, 'Contents/MacOS', name)]), 'arm64', name);
}
run('codesign', ['--verify', '--deep', '--strict', bundle]);

verifySource();
assert.equal(read('git', ['ls-remote', '--exit-code', '--heads', 'origin', 'refs/heads/master']),
  `${sourceCommit}\trefs/heads/master`, 'Remote master changed during the build; update master and build again.');
const output = path.join(repo, 'dist/macos/Lacus.app');
mkdirSync(path.dirname(output), { recursive: true });
rmSync(output, { recursive: true, force: true });
cpSync(bundle, output, { recursive: true, preserveTimestamps: true });
run('codesign', ['--verify', '--deep', '--strict', output]);
console.log(`Built ${output}`);

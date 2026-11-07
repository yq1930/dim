import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import path from 'node:path';

const args = process.argv.slice(2);
assert(args.length === 0 || (args.length === 1 && args[0] === '--dry-run'),
  'Usage: node scripts/publish-macos-app.mjs [--dry-run]');
assert(process.platform === 'darwin', 'Publish Lacus.app on macOS.');

const repo = path.resolve(import.meta.dirname, '..');
const desktop = path.join(repo, 'desktop');
const app = path.join(repo, 'dist/macos/Lacus.app');
const stage = path.join(repo, 'staging/macos-release');
const key = process.env.TAURI_SIGNING_PRIVATE_KEY_PATH ?? path.join(homedir(), '.tauri/lacus-updater.key');

function run(command, parameters, options = {}) {
  const result = spawnSync(command, parameters, { cwd: repo, stdio: 'inherit', ...options });
  assert.ifError(result.error);
  assert.equal(result.status, 0, `${command} ${parameters.join(' ')} failed`);
}

function read(command, parameters, options = {}) {
  return execFileSync(command, parameters, { cwd: repo, encoding: 'utf8', ...options }).trim();
}

assert(existsSync(app), 'Build dist/macos/Lacus.app first with node scripts/build-macos-app.mjs.');
const info = JSON.parse(readFileSync(path.join(app, 'Contents/Resources/build-info.json'), 'utf8'));
assert.equal(info.profile, 'release', 'Publish a release build.');
assert.equal(info.sourceRef, 'refs/heads/master', 'Publish a build of master.');
assert.equal(info.sourceDirty, false, 'Publish a build of a clean worktree.');
run('git', ['fetch', '--no-tags', 'origin', 'refs/heads/master:refs/remotes/origin/master']);
assert.equal(read('git', ['rev-parse', 'origin/master']), info.sourceCommit,
  'dist/macos/Lacus.app was not built from the current origin/master; build it again.');
run('codesign', ['--verify', '--deep', '--strict', app]);

const config = JSON.parse(read('git', ['show', `${info.sourceCommit}:desktop/src-tauri/tauri.conf.json`]));
const { pubkey, endpoints = [] } = config.plugins?.updater ?? {};
assert(pubkey && endpoints.length === 1, 'This build has no updater; build a commit that configures it.');
const slug = endpoints[0].match(/^https:\/\/github\.com\/([\w.-]+\/[\w.-]+)\/releases\/latest\/download\/latest\.json$/)?.[1];
assert(slug, `The updater endpoint ${endpoints[0]} is not the latest.json of a GitHub release.`);
assert(read('git', ['remote', 'get-url', 'origin']).replace(/\.git$/, '').endsWith(`github.com/${slug}`),
  `origin is not ${slug}, which the updater reads.`);
assert(existsSync(key), `The update signing key ${key} is missing; set TAURI_SIGNING_PRIVATE_KEY_PATH.`);
assert.equal(readFileSync(`${key}.pub`, 'utf8').trim(), pubkey, `${key} does not belong to the updater public key of this build.`);
assert(readFileSync(path.join(app, 'Contents/MacOS/dim-desktop')).includes(pubkey),
  'The bundle does not carry the updater public key.');

const stamp = String(info.builtAt).slice(0, 19).replace(/\D/g, '');
assert.equal(stamp.length, 14, 'build-info.json records no build time.');
const commit = info.sourceCommit.slice(0, 7);
const version = `${info.version}+${stamp}.${commit}`;
const tag = `lacus-${stamp.slice(0, 8)}-${commit}`;
const date = `${stamp.slice(0, 4)}-${stamp.slice(4, 6)}-${stamp.slice(6, 8)}`;

const published = read('git', ['ls-remote', '--tags', 'origin', 'refs/tags/lacus-*']).split('\n').filter(Boolean)
  .map((line) => line.split('\t'))
  .map(([sha, ref]) => ({ sha, tag: ref.slice('refs/tags/'.length) }))
  .sort((a, b) => a.tag.localeCompare(b.tag));
const existing = published.find((release) => release.sha === info.sourceCommit);
assert(!existing, `${commit} is already published as ${existing?.tag}.`);
const previous = published.at(-1);
const changes = previous
  && spawnSync('git', ['merge-base', '--is-ancestor', previous.sha, info.sourceCommit], { cwd: repo }).status === 0
  ? read('git', ['log', '--no-merges', '--format=- %s', `${previous.sha}..${info.sourceCommit}`]).split('\n').filter(Boolean)
  : [];
const notes = changes.length > 30 ? [...changes.slice(0, 30), `- 另有 ${changes.length - 30} 个提交`] : changes;

rmSync(stage, { recursive: true, force: true });
mkdirSync(stage, { recursive: true });
const archive = path.join(stage, 'Lacus.app.tar.gz');
run('tar', ['-czf', archive, '-C', path.dirname(app), 'Lacus.app'], { env: { ...process.env, COPYFILE_DISABLE: '1' } });
assert(read('tar', ['-tzf', archive]).split('\n').every((entry) => entry.startsWith('Lacus.app/') && !/(^|\/)\._/.test(entry)),
  'The update archive must hold only Lacus.app, without AppleDouble files.');
run('npm', ['exec', '--', 'tauri', 'signer', 'sign', '--private-key-path', key, '--app-version', version, archive], {
  cwd: desktop,
  env: { ...process.env, TAURI_SIGNING_PRIVATE_KEY_PASSWORD: process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ?? '' },
});
const zip = path.join(stage, 'Lacus-macos-arm64.zip');
run('ditto', ['-c', '-k', '--keepParent', '--norsrc', '--noextattr', app, zip]);

const manifest = {
  version,
  notes: notes.join('\n'),
  pub_date: info.builtAt,
  platforms: {
    'darwin-aarch64': {
      signature: readFileSync(`${archive}.sig`, 'utf8').trim(),
      url: `https://github.com/${slug}/releases/download/${tag}/Lacus.app.tar.gz`,
    },
  },
};
const latest = path.join(stage, 'latest.json');
writeFileSync(latest, `${JSON.stringify(manifest, null, 2)}\n`);
const body = path.join(stage, 'notes.md');
writeFileSync(body, [
  `Lacus 构建于 ${date}，提交 ${commit}。`,
  ...(notes.length ? ['', ...notes] : []),
  '',
  '已安装的 Lacus 会在应用内提示并完成更新。首次安装：下载 Lacus-macos-arm64.zip，解压后把 Lacus.app 拖入“应用程序”，第一次在 Finder 中右键选择“打开”。',
  '',
].join('\n'));

if (args.includes('--dry-run')) {
  console.log(`Staged ${tag} (${version}) in ${stage}; nothing was published.`);
  process.exit(0);
}
run('gh', ['release', 'create', tag, archive, `${archive}.sig`, zip, latest, '--repo', slug,
  '--target', info.sourceCommit, '--title', `Lacus ${date} (${commit})`, '--notes-file', body, '--latest']);
assert.equal(read('gh', ['release', 'view', '--repo', slug, '--json', 'tagName', '--jq', '.tagName']), tag,
  `${tag} is not the latest release of ${slug}.`);
console.log(`Published ${tag} (${version}) to ${slug}.`);

#!/usr/bin/env node
// The browser preview of the Web workspace with hot reload, in one command:
// an isolated `dimcode web` entry and the Vite dev server that forwards /api
// and /ws to it (web/README.md). Ctrl+C stops both and the home's daemon.
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { parseArgs } from 'node:util';

const USAGE = `用法：node scripts/web-preview.mjs [--home 目录] [--backend 路径] [--port 端口] [--no-open]

  --home DIR      预览用的独立数据目录，默认 ~/.dimcode-preview
  --backend PATH  直接使用这个 dimcode，不编译 target/release/dimcode
  --port N        Vite 端口，默认 5173，被占用时自动另选
  --no-open       只打印预览地址，不打开浏览器`;

let options;
try {
  ({ values: options } = parseArgs({
    options: {
      home: { type: 'string' },
      backend: { type: 'string' },
      port: { type: 'string' },
      'no-open': { type: 'boolean', default: false },
      help: { type: 'boolean', short: 'h', default: false },
    },
  }));
} catch (error) {
  console.error(`${error.message}\n\n${USAGE}`);
  process.exit(2);
}
if (options.help) {
  console.log(USAGE);
  process.exit(0);
}

const repo = path.resolve(import.meta.dirname, '..');
const web = path.join(repo, 'web');
const home = path.resolve(options.home ?? path.join(os.homedir(), '.dimcode-preview'));
const assets = path.join(home, 'preview-web');
const lock = path.join(home, 'preview.pid');
const preferredPort = Number(options.port ?? 5173);
let locked = false;

function fail(message) {
  console.error(message);
  if (locked) rmSync(lock, { force: true });
  process.exit(1);
}

if (!Number.isInteger(preferredPort) || preferredPort < 1 || preferredPort > 65535) fail(`端口无效：${options.port}`);
// The daemon listens on <home>/rust/daemon.sock, and macOS caps a socket path at 104 bytes.
if (Buffer.byteLength(path.join(home, 'rust', 'daemon.sock')) >= 100) {
  fail(`数据目录的路径太长，放不下 Unix socket：${home}。用 --home 换一个短路径。`);
}

function run(command, parameters, env = process.env) {
  const result = spawnSync(command, parameters, { cwd: repo, env, stdio: 'inherit' });
  if (result.error !== undefined || result.status !== 0) fail(`${command} ${parameters.join(' ')} 失败。`);
}

function alive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error.code === 'EPERM';
  }
}

// Two previews of one home would stop each other's daemon.
mkdirSync(assets, { recursive: true, mode: 0o700 });
const previous = existsSync(lock) ? Number(readFileSync(lock, 'utf8')) : 0;
if (Number.isInteger(previous) && previous > 0 && previous !== process.pid && alive(previous)) {
  console.error(`这个数据目录已有预览在运行（进程 ${previous}）。先在那边按 Ctrl+C，或用 --home 另开一个；确认没有预览在运行时，删除 ${lock} 再试。`);
  process.exit(1);
}
writeFileSync(lock, String(process.pid));
locked = true;

let backend = options.backend === undefined ? undefined : path.resolve(options.backend);
if (backend === undefined) {
  // build-macos-app.mjs builds with these settings; matching them shares target/release instead of rebuilding it.
  const env = { ...process.env, CARGO_INCREMENTAL: '0' };
  delete env.RUSTC_WRAPPER;
  console.log('编译后端（代码没变时约 1 秒）…');
  run('cargo', ['build', '--locked', '--release', '-p', 'dimcode'], env);
  backend = path.join(repo, 'target', 'release', 'dimcode');
}
if (!existsSync(backend)) fail(`找不到后端：${backend}`);

const vite = path.join(web, 'node_modules', '.bin', 'vite');
if (!existsSync(vite)) {
  console.log('安装前端依赖…');
  run('npm', ['ci', '--prefix', web, '--ignore-scripts', '--no-audit', '--no-fund', '--registry=https://registry.npmjs.org']);
}

const entryEnv = { ...process.env, DIMCODE_WEB_ASSETS: assets };
// As ./dev does: the default plugins, staged once and refreshed when they change.
if (!entryEnv.DIMCODE_DEFAULT_PLUGINS) {
  const stage = path.join(repo, 'staging', 'dev-plugins');
  const staged = spawnSync(process.execPath, [path.join(repo, 'scripts', 'stage-plugins.mjs'), stage, '--update'],
    { cwd: repo, stdio: ['ignore', 'ignore', 'inherit'] });
  if (staged.status === 0) entryEnv.DIMCODE_DEFAULT_PLUGINS = path.join(stage, 'default-plugins.json');
  else console.warn('默认插件没有准备好，这次预览不带默认插件。');
}

/** Whether nothing listens on `port` at `host`; an address this machine lacks (no IPv6) does not count. */
function free(port, host) {
  return new Promise((resolve) => {
    const server = createServer();
    server.once('error', (error) => resolve(error.code === 'EADDRNOTAVAIL' || error.code === 'EAFNOSUPPORT'));
    server.listen({ port, host }, () => server.close(() => resolve(true)));
  });
}

/** `preferred` when it is free on both loopback addresses, either of which `localhost` may name; else a free port. */
async function pickPort(preferred, taken = []) {
  const usable = async (port) => !taken.includes(port) && await free(port, '127.0.0.1') && await free(port, '::1');
  if (preferred !== undefined && await usable(preferred)) return preferred;
  for (;;) {
    const server = createServer();
    await new Promise((resolve, reject) => {
      server.once('error', reject);
      server.listen(0, '127.0.0.1', resolve);
    });
    const { port } = server.address();
    await new Promise((resolve) => server.close(resolve));
    if (await usable(port)) return port;
  }
}

function stopDaemon() {
  spawnSync(backend, ['--home', home, 'daemon', 'stop'], { cwd: home, stdio: 'ignore' });
}

const children = [];
let stopping = false;

async function stop(code) {
  if (stopping) return;
  stopping = true;
  const running = children.filter((child) => child.exitCode === null && child.signalCode === null);
  for (const child of running) child.kill('SIGINT');
  await Promise.all(running.map((child) => Promise.race([
    once(child, 'exit'),
    delay(5000).then(() => child.kill('SIGKILL')),
  ])));
  stopDaemon();
  rmSync(lock, { force: true });
  console.log('预览已停止。');
  process.exit(code);
}

for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) process.on(signal, () => void stop(0));

/**
 * A child that ends on its own ends the preview. Ctrl+C in a terminal reaches
 * the children too, so the check waits for this process's own handler first.
 */
function stopWhenExited(child, name) {
  child.once('exit', (code, signal) => setTimeout(() => {
    if (stopping) return;
    console.error(`${name}已退出（${code ?? signal}），正在停止预览。`);
    void stop(1);
  }, 300));
}

try {
  const vitePort = await pickPort(preferredPort);
  const entryPort = await pickPort(undefined, [vitePort]);
  writeFileSync(path.join(assets, 'index.html'), [
    '<!doctype html>',
    '<meta charset="utf-8">',
    '<title>Lacus 预览后端</title>',
    `<p>这是浏览器预览用的后端，页面在 <a href="http://localhost:${vitePort}/">http://localhost:${vitePort}/</a>。</p>`,
    '',
  ].join('\n'));

  // A daemon left from an earlier run may hold an older backend or other web assets.
  stopDaemon();
  const entry = spawn(backend, ['--home', home, 'web', '--port', String(entryPort), '--no-open'],
    { cwd: home, env: entryEnv, stdio: ['ignore', 'pipe', 'inherit'] });
  children.push(entry);
  const token = await new Promise((resolve, reject) => {
    let output = '';
    const timer = setTimeout(() => reject(new Error('后端 60 秒内没有给出登录链接。')), 60_000);
    entry.stdout.on('data', (chunk) => {
      output += chunk;
      const match = /#bootstrap=([\w-]+)/.exec(output);
      if (match === null) return;
      clearTimeout(timer);
      entry.stdout.removeAllListeners('data');
      entry.stdout.resume();
      resolve(match[1]);
    });
    entry.once('exit', (code, signal) => {
      clearTimeout(timer);
      reject(new Error(`后端没有启动（${code ?? signal}）：${output.trim()}`));
    });
  });
  stopWhenExited(entry, '后端');

  const server = spawn(vite, ['--port', String(vitePort), '--strictPort', '--clearScreen', 'false'], {
    cwd: web,
    env: { ...process.env, DIM_WEB_TARGET: `http://127.0.0.1:${entryPort}` },
    stdio: ['ignore', 'inherit', 'inherit'],
  });
  children.push(server);
  stopWhenExited(server, 'Vite ');
  const root = `http://localhost:${vitePort}/`;
  for (let attempt = 0; ; attempt += 1) {
    if (attempt === 120) throw new Error('Vite 60 秒内没有就绪。');
    if (stopping) await new Promise(() => {});
    const ready = await fetch(root).then((response) => {
      void response.body?.cancel();
      return response.ok;
    }, () => false);
    if (ready) break;
    await delay(500);
  }

  const url = `${root}#bootstrap=${token}`;
  console.log(`\n预览地址：${url}\n数据目录：${home}\n改了前端代码，页面会自动更新；按 Ctrl+C 停止。\n`);
  if (!options['no-open']) {
    const opener = { darwin: 'open', linux: 'xdg-open' }[process.platform];
    if (opener !== undefined) {
      const child = spawn(opener, [url], { stdio: 'ignore', detached: true });
      child.on('error', () => console.warn('没能打开浏览器，请手动打开上面的地址。'));
      child.unref();
    }
  }
} catch (error) {
  if (!stopping) {
    console.error(error.message);
    await stop(1);
  }
}

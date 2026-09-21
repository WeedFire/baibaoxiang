// 插件市场发布助手：把本地 pyTools（脚本类）与依赖目录打包成 zip，用 Tauri signer 签名，
// 生成 marketplace.json（含每个插件的下载地址与签名）。与后端 marketplace_service 的清单格式一一对应。
//
// 用法：
//   node scripts/package-plugins.mjs \
//     --repo wjqnxw/baibaoxiang-plugins \
//     --tag plugins \
//     --plugins-dir pyTools \
//     --deps-dir python-deps \
//     --program ../python \
//     --out dist/plugins
//
// 三类目录的区别：
//   --plugins-dir  其【子目录】各为一个 python_script 插件（落到 <根>/pyTools/<id>/，自动登记应用）
//   --deps-dir     其【子目录】各为一个 dependency 插件（落到 <根>/python/Lib/site-packages/）
//   --program      目录【本身】为一个 program 插件（可重复；dir 名即 id，落到 <根>/<id>/）
//                  用于内置 Python 运行环境这类整体交付的运行时包（例：--program ../python → <根>/python/）
//   --meta         外部元数据覆盖文件（JSON：{ "<id>": {...} }），优先级高于包内 plugin.json；
//                  python/ 是只读目录联接，没法在里面放 plugin.json，就用它描述
//
// 签名需要的密钥（与更新包同一个密钥对）：
//   TAURI_SIGNING_PRIVATE_KEY            —— `tauri signer generate` 生成的私钥内容
//   TAURI_SIGNING_PRIVATE_KEY_PASSWORD   —— 密钥密码（未设可省略）
// 本地预览清单可用 --no-sign（signature 留空，但客户端在配置了公钥时会拒绝安装）。

import { parseArgs } from 'node:util';
import {
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  writeFileSync,
} from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

function fail(message) {
  console.error(`package-plugins: ${message}`);
  process.exit(1);
}

function run(cmd, args, env) {
  // Windows 上 npx/tar 需经 shell 启动（npx 其实是 npx.cmd，不经 shell 会 ENOENT）。
  const res = spawnSync(cmd, args, {
    stdio: 'inherit',
    shell: process.platform === 'win32',
    env: { ...process.env, ...(env ?? {}) },
  });
  if (res.error) fail(`无法启动命令：${cmd} ${args.join(' ')}（${res.error.message}）`);
  if (res.status !== 0) fail(`命令失败（退出码 ${res.status}）：${cmd} ${args.join(' ')}`);
}

const { values } = parseArgs({
  options: {
    repo: { type: 'string' },
    tag: { type: 'string' },
    'plugins-dir': { type: 'string', default: 'pyTools' },
    'deps-dir': { type: 'string' },
    program: { type: 'string', multiple: true },
    meta: { type: 'string' },
    out: { type: 'string', default: 'dist/plugins' },
    'no-sign': { type: 'boolean', default: false },
  },
});

const repo = values.repo ?? 'wjqnxw/baibaoxiang-plugins';
const tag = values.tag ?? `plugins`;
const pluginsDir = values['plugins-dir'];
const depsDir = values['deps-dir'];
const outDir = values.out;
const noSign = values['no-sign'];

if (!noSign && !process.env.TAURI_SIGNING_PRIVATE_KEY) {
  fail(
    '未设置环境变量 TAURI_SIGNING_PRIVATE_KEY（tauri signer 签名必需）。\n' +
      '  · 用已有密钥：$env:TAURI_SIGNING_PRIVATE_KEY = "私钥内容"（设过密码再加 TAURI_SIGNING_PRIVATE_KEY_PASSWORD）\n' +
      '  · 重新生成：npx tauri signer generate（公钥同步填进 update_service::PUBLIC_KEY）\n' +
      '  · 仅看结构：加 --no-sign（注意客户端已配公钥，会拒绝安装未签名插件）',
  );
}

// --meta：外部元数据覆盖文件（JSON：{ "<插件id>": { name, description, version, author, ... } }），
// 用于 python/ 这种只读/目录联接的运行时，无法在其内部放 plugin.json 的场景。
let metaOverrides = {};
if (values.meta) {
  if (!existsSync(values.meta)) fail(`--meta 文件不存在：${values.meta}`);
  metaOverrides = readJson(values.meta);
  if (!metaOverrides || typeof metaOverrides !== 'object') {
    fail(`--meta 不是合法 JSON 对象：${values.meta}`);
  }
}

if (!existsSync(outDir)) mkdirSync(outDir, { recursive: true });

function readJson(path) {
  try {
    return JSON.parse(readFileSync(path, 'utf8'));
  } catch {
    return null;
  }
}

/** 列出目录下的直接子目录（每个子目录视为一个插件/依赖包） */
function subdirs(dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir, { withFileTypes: true })
    .filter((e) => e.isDirectory())
    .map((e) => e.name);
}

/** 在目录里找第一个 .py 文件作为默认入口 */
function defaultEntry(dir) {
  const files = readdirSync(dir, { withFileTypes: true }).filter(
    (e) => e.isFile() && e.name.toLowerCase().endsWith('.py'),
  );
  if (files.some((f) => f.name.toLowerCase() === 'main.py')) return 'main.py';
  if (files.length > 0) return files[0].name;
  return '';
}

function buildPlugin(rootDir, id, fallbackKind) {
  const dir = join(rootDir, id);
  // 元数据优先级：--meta 里的覆盖 > 包目录内的 plugin.json
  // （python/ 这类目录联接/只读运行时没法在里面放 plugin.json，用外部 --meta 描述）。
  const meta = { ...(readJson(join(dir, 'plugin.json')) ?? {}), ...(metaOverrides[id] ?? {}) };
  // 版本号：优先 plugin.json 里的 version；否则若 --tag 形如版本（含 x.y）则用之，
  // 否则回退 1.0.0（Gitee 市场固定用 `plugins` 这类非版本 tag 时不会误用 tag 当版本）。
  const version = meta.version ?? (/\d+\.\d+/.test(tag) ? tag.replace(/^v/, '') : '1.0.0');
  const kind = meta.kind ?? fallbackKind;
  const entry =
    meta.entry ?? (kind === 'python_script' || kind === 'program' ? defaultEntry(dir) : '');
  const launchKind = meta.launch_kind ?? (kind === 'python_script' ? 1 : 0);
  const interpreter = meta.interpreter ?? (kind === 'python_script' ? 'python/python.exe' : '');

  const zipName = `${id}_${version}.zip`;
  const zipPath = join(outDir, zipName);

  // 打包：tar.exe 的 -a 模式直接生成 zip
  run('tar.exe', ['-a', '-cf', zipPath, '-C', dir, '.']);

  let signature = null;
  if (!noSign) {
    run('npx', ['tauri', 'signer', 'sign', zipPath]);
    try {
      signature = readFileSync(`${zipPath}.sig`, 'utf8').trim();
    } catch {
      fail(`签名文件未生成：${zipPath}.sig`);
    }
  }

  const downloadUrl = `https://gitee.com/${repo}/releases/download/${encodeURIComponent(
    tag,
  )}/${encodeURIComponent(zipName)}`;

  return {
    id,
    name: meta.name ?? id,
    description: meta.description ?? '',
    version,
    author: meta.author ?? '',
    kind,
    download_url: downloadUrl,
    signature,
    icon_url: meta.icon_url ?? null,
    entry,
    launch_kind: launchKind,
    interpreter,
    python_requirement: meta.python_requirement ?? null,
  };
}

const plugins = [];

for (const id of subdirs(pluginsDir)) {
  plugins.push(buildPlugin(pluginsDir, id, 'python_script'));
}

if (depsDir) {
  for (const id of subdirs(depsDir)) {
    plugins.push(buildPlugin(depsDir, id, 'dependency'));
  }
}

// --program：目录本身即插件（dir 名作为 id），kind=program。
for (const dir of values.program ?? []) {
  if (!existsSync(dir)) fail(`--program 目录不存在：${dir}`);
  const abs = resolve(dir);
  plugins.push(buildPlugin(dirname(abs), basename(abs), 'program'));
}

if (plugins.length === 0) {
  console.warn('package-plugins: 没有发现任何插件目录');
}

const manifest = {
  schema: 1,
  updated_at: new Date().toISOString(),
  plugins,
};

const outPath = join(outDir, 'marketplace.json');
writeFileSync(outPath, JSON.stringify(manifest, null, 2) + '\n', 'utf8');
console.log(`marketplace.json 已生成：${outPath}（共 ${plugins.length} 个插件）`);

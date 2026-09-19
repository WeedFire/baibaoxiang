// 生成 Tauri 风格的 latest.json（供客户端自动检查/下载/安装使用）。
//
// 用法：
//   node scripts/gen-latest.mjs \
//     --version 1.0.2 \
//     --repo WeedFire/baibaoxiang \
//     --tag v1.0.2 \
//     --asset baibaoxiang_1.0.2_x64-setup.exe \
//     --signature-file dist/baibaoxiang_1.0.2_x64-setup.exe.sig \
//     --notes-file release-notes.md \
//     [--platform windows-x86_64] [--url <完整下载地址>] [--out dist/latest.json]
//
// signature 直接取 `tauri signer sign` 生成的 .sig 文件内容（base64 的 minisign 文本），
// 与客户端 `update_install::verify_signature`（minisign 分支）一一对应。

import { parseArgs } from 'node:util';
import { readFileSync, writeFileSync } from 'node:fs';

function fail(message) {
  console.error(`gen-latest: ${message}`);
  process.exit(1);
}

const { values } = parseArgs({
  options: {
    version: { type: 'string' },
    repo: { type: 'string' },
    tag: { type: 'string' },
    asset: { type: 'string' },
    'signature-file': { type: 'string' },
    platform: { type: 'string', default: 'windows-x86_64' },
    'notes-file': { type: 'string' },
    url: { type: 'string' },
    out: { type: 'string', default: 'latest.json' },
  },
});

const version = values.version;
if (!version) fail('缺少 --version');
const tag = values.tag ?? `v${version}`;
const asset = values.asset;
if (!asset) fail('缺少 --asset（安装包文件名）');

// 下载地址：优先显式 --url，否则按 GitHub Release 约定拼出来
let url = values.url;
if (!url) {
  const repo = values.repo;
  if (!repo) fail('缺少 --repo（owner/name）或 --url');
  url = `https://github.com/${repo}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(asset)}`;
}

const sigFile = values['signature-file'];
if (!sigFile) fail('缺少 --signature-file（.sig 文件路径）');
let signature = '';
try {
  signature = readFileSync(sigFile, 'utf8').trim();
} catch {
  fail(`无法读取签名文件：${sigFile}`);
}
if (!signature) fail('签名文件内容为空');

let notes = '';
if (values['notes-file']) {
  try {
    notes = readFileSync(values['notes-file'], 'utf8').trim();
  } catch {
    // 没有说明文件时保持为空，不视为失败
  }
}

const manifest = {
  version,
  notes,
  pub_date: new Date().toISOString(),
  platforms: {
    [values.platform]: { signature, url },
  },
};

writeFileSync(values.out, JSON.stringify(manifest, null, 2) + '\n', 'utf8');
console.log(`latest.json 已生成：${values.out}`);
console.log(`  version=${version} platform=${values.platform}`);
console.log(`  url=${url}`);

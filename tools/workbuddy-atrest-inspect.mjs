#!/usr/bin/env node
// WorkBuddy 5.8.x 落盘加密（`$wbEncrypted` 字段信封）的**独立复核器**。
//
// 用途：不依赖网关、不依赖 Rust 构建，直接用 Node 自带的 crypto 复刻客户端
// bundle 里的 at-rest 算法，把一个登录态文件解开、打印结果。三件事：
//   1. 证明 spec（AAD 字节序列 + 密钥推导）在第二份独立实现下仍然成立；
//   2. 给运维/排查一条「到底是不是加密、能不能解开」的确定答案；
//   3. 用解出来的 accessToken 真打一次上游接口，确认拿到的是可用登录态。
//
// 算法出处（逐字对照客户端 bundle）：
//   resources/app.asar                      → app-lifetime-job.js（模块 9003）
//   resources/app.asar.unpacked/cli/dist    → codebuddy-lite-wb.mjs
//     key     = sha256(atRestSecretKey 的 UTF-8 字节)   # normalizeAtRestKeyPayload
//     keyId   = sha256(key).hex().slice(0, 16)          # deriveAtRestKeyId
//     AAD     = "WB-AAD\0" | 0x01 | lp("WBEV1") | lp("sym-v1") | u32be(suite)
//               | lp(keyId) | 0x02 | 0x00 | 0x00
//               lp(s) = u32be(Buffer.byteLength(s, "utf8")) | utf8(s)
//     字段信封 = base64(JSON)，键恰为 authTag/ciphertext/keyId/nonce/suite，
//                AES-256-GCM(key, nonce=12B, tag=16B)
//
// 保护密钥（atRestSecretKey）不在任何落盘文件里：由客户端 native 运行时经
// electron.workbuddyStorage.loggerGet() 交给主进程，再分发到 daemon / CLI。
// 取它的办法见 probe/hook-atrest-key.py（只读扫运行中客户端进程内存）。
//
// 用法：
//   node tools/workbuddy-atrest-inspect.mjs --secret <44 字符>            # 自动找文件
//   node tools/workbuddy-atrest-inspect.mjs --secret <44 字符> --file <path>
//   node tools/workbuddy-atrest-inspect.mjs --secret-file <json>          # 从文件读密钥
//   node tools/workbuddy-atrest-inspect.mjs --secret <44 字符> --offline  # 不联上游
//   node tools/workbuddy-atrest-inspect.mjs --secret .... --json          # 机器可读
// 退出码：0 全部解开；2 有字段解不开；3 找不到登录态文件或参数缺失。
//
// `--secret-file` 读的是 `{"atRestSecretKey":"…"}` 形态的 JSON（`probe/atrest_decrypt.py
// --dump` 就写这个文件），省得把密钥贴进命令行（进程列表里能看到命令行）。

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';

const AAD_DOMAIN = Buffer.from('WB-AAD\0', 'ascii');
const FIELD_MAGIC = 'WBEV1';
const SCHEME = 'sym-v1';
const FRAMING_CODE = 2; // {file:1, field:2, record:3, stream:4}
const WRAPPER_MARKER = '$wbEncrypted';
const GROWTH_TASKS = 'https://copilot.tencent.com/v2/activity/growth/tasks';

function parseArgs(argv) {
  const out = { offline: false, json: false };
  for (let i = 0; i < argv.length; i += 1) {
    const key = argv[i];
    if (key === '--offline') out.offline = true;
    else if (key === '--json') out.json = true;
    else if (key.startsWith('--')) {
      out[key.slice(2)] = argv[i + 1];
      i += 1;
    }
  }
  return out;
}

function lengthPrefixed(text) {
  const raw = Buffer.from(text, 'utf8');
  const head = Buffer.alloc(4);
  head.writeUInt32BE(raw.length, 0);
  return Buffer.concat([head, raw]);
}

function buildFieldAad(keyId, suite) {
  const suiteBytes = Buffer.alloc(4);
  suiteBytes.writeUInt32BE(suite, 0);
  return Buffer.concat([
    AAD_DOMAIN,
    Buffer.from([1]),
    lengthPrefixed(FIELD_MAGIC),
    lengthPrefixed(SCHEME),
    suiteBytes,
    lengthPrefixed(keyId),
    Buffer.from([FRAMING_CODE, 0, 0]),
  ]);
}

function deriveKey(secret) {
  const trimmed = String(secret).trim();
  const decoded = Buffer.from(trimmed, 'base64');
  const canonical = decoded.length === 32 && decoded.toString('base64') === trimmed;
  if (!canonical) throw new Error('保护密钥不是 32 字节的 canonical base64（应为 44 个字符）');
  if (decoded.every((byte) => byte === 0)) throw new Error('保护密钥是全零哨兵值（源实现同样拒绝）');
  const key = crypto.createHash('sha256').update(trimmed, 'utf8').digest();
  return { key, keyId: crypto.createHash('sha256').update(key).digest('hex').slice(0, 16) };
}

function isWrapper(value) {
  return (
    value
    && typeof value === 'object'
    && !Array.isArray(value)
    && value[WRAPPER_MARKER] === 1
    && typeof value.envelope === 'string'
  );
}

function collectWrappers(value, out = []) {
  if (Array.isArray(value)) {
    for (const item of value) collectWrappers(item, out);
  } else if (value && typeof value === 'object') {
    if (isWrapper(value)) out.push(value);
    else for (const item of Object.values(value)) collectWrappers(item, out);
  }
  return out;
}

function openEnvelope(keyInfo, envelopeBase64) {
  const json = JSON.parse(Buffer.from(envelopeBase64, 'base64').toString('utf8'));
  const keys = Object.keys(json).sort();
  if (keys.join(',') !== 'authTag,ciphertext,keyId,nonce,suite') {
    throw new Error(`信封字段集合不符：${keys.join(',')}`);
  }
  if (json.suite !== 1) throw new Error(`不支持的 suite：${json.suite}`);
  if (json.keyId !== keyInfo.keyId) {
    throw new Error(`信封 keyId（${json.keyId}）与密钥指纹（${keyInfo.keyId}）不一致`);
  }
  const nonce = Buffer.from(json.nonce, 'base64');
  const tag = Buffer.from(json.authTag, 'base64');
  if (nonce.length !== 12 || tag.length !== 16) throw new Error('nonce / authTag 长度异常');
  const decipher = crypto.createDecipheriv('aes-256-gcm', keyInfo.key, nonce, { authTagLength: 16 });
  decipher.setAAD(buildFieldAad(json.keyId, json.suite));
  decipher.setAuthTag(tag);
  return Buffer.concat([decipher.update(Buffer.from(json.ciphertext, 'base64')), decipher.final()]).toString('utf8');
}

function authDir() {
  return path.join(process.env.LOCALAPPDATA || '', 'CodeBuddyExtension', 'Data', 'Public', 'auth');
}

function pickFile(explicit) {
  if (explicit) return explicit;
  const dir = authDir();
  if (!fs.existsSync(dir)) return null;
  const names = fs.readdirSync(dir).filter((name) => /^workbuddy-desktop(-ai)?\.info$/.test(name));
  const fallback = fs
    .readdirSync(dir)
    .filter((name) => /^workbuddy-desktop(-ai)?\..+\.info$/.test(name))
    .sort()
    .reverse();
  const chosen = names[0] || fallback[0];
  return chosen ? path.join(dir, chosen) : null;
}

async function validateToken(token) {
  try {
    const res = await fetch(GROWTH_TASKS, {
      headers: {
        Authorization: `Bearer ${token}`,
        Accept: 'application/json',
        'User-Agent': 'CLI/2.63.2 CodeBuddy/2.63.2',
      },
    });
    const body = await res.json().catch(() => null);
    const tasks = body?.data?.tasks;
    return { http: res.status, code: body?.code, tasks: Array.isArray(tasks) ? tasks.length : null };
  } catch (error) {
    return { error: error.message };
  }
}

const args = parseArgs(process.argv.slice(2));
const report = { file: null, keyId: null, fields: [], plaintext: {}, ok: true };

// --secret-file：从 {"atRestSecretKey":"…"} / {"secret":"…"} / {"key":"…"} 形态的
// JSON 里取密钥，避免把密钥写进命令行（命令行在进程列表里对同机用户可见）。
if (!args.secret && args['secret-file']) {
  try {
    const data = JSON.parse(fs.readFileSync(args['secret-file'], 'utf8'));
    args.secret = data.atRestSecretKey || data.secret || data.key || '';
  } catch (error) {
    console.error(`读取 --secret-file 失败：${error.message}`);
    process.exit(3);
  }
}

if (!args.secret) {
  console.error('缺少 --secret <44 字符保护密钥>。取键办法见 tools/workbuddy-atrest-inspect.mjs 文件头。');
  process.exit(3);
}

const file = pickFile(args.file);
if (!file) {
  console.error(`没找到登录态文件（${authDir()}）。先打开官方客户端登录一次。`);
  process.exit(3);
}
report.file = file;

let keyInfo;
try {
  keyInfo = deriveKey(args.secret);
} catch (error) {
  console.error(`保护密钥不可用：${error.message}`);
  process.exit(3);
}
report.keyId = keyInfo.keyId;

const payload = JSON.parse(fs.readFileSync(file, 'utf8'));
const wrappers = collectWrappers(payload);
report.encryptedFields = wrappers.length;

if (!args.json) {
  console.log(`文件      ${file}`);
  console.log(`密钥指纹  ${keyInfo.keyId}`);
  console.log(`信封字段  ${wrappers.length} 个`);
}

const results = [];
for (const wrapper of wrappers) {
  try {
    const plaintext = openEnvelope(keyInfo, wrapper.envelope);
    results.push({ ok: true, value: plaintext });
  } catch (error) {
    report.ok = false;
    results.push({ ok: false, error: error.message });
  }
}

for (const [index, item] of results.entries()) {
  if (!item.ok) {
    if (!args.json) console.log(`  [×] 字段 #${index + 1} 解不开：${item.error}`);
    report.fields.push({ index: index + 1, ok: false, error: item.error });
    continue;
  }
  const value = item.value;
  if (value.startsWith('eyJ')) {
    const claims = JSON.parse(Buffer.from(value.split('.')[1] || '', 'base64').toString('utf8'));
    report.plaintext.accessToken = `${value.slice(0, 24)}…(${value.length} 字符)`;
    report.plaintext.sub = claims.sub;
    report.plaintext.iss = claims.iss;
    if (!args.json) {
      console.log(`  [√] 字段 #${index + 1} 令牌 ${value.length} 字符  sub=${claims.sub}`);
    }
    report.fields.push({ index: index + 1, ok: true, kind: 'token', length: value.length, sub: claims.sub });
    if (!args.offline) {
      const check = await validateToken(value);
      report.liveCheck = check;
      if (!args.json) {
        console.log(
          `      上游校验 HTTP ${check.http ?? '-'} code=${check.code ?? '-'} tasks=${check.tasks ?? '-'}${
            check.error ? ` 错误=${check.error}` : ''
          }`,
        );
      }
    }
  } else {
    report.plaintext[`field#${index + 1}`] = value;
    if (!args.json) console.log(`  [√] 字段 #${index + 1} 明文 ${JSON.stringify(value)}`);
    report.fields.push({ index: index + 1, ok: true, kind: 'text', value });
  }
}

if (args.json) console.log(JSON.stringify(report, null, 2));
process.exit(report.ok ? 0 : 2);

#!/usr/bin/env node
// 成长计划「新活动」实况巡检器（只读为主，写动作全部默认关闭）。
//
// ── 干什么 ────────────────────────────────────────────────────
// 用你自己的 WorkBuddy 账号凭证，把 https://www.workbuddy.cn/profile/growth-center
// 这个页面背后**全部**的 growth 接口打一遍，产出：
//   1. 当前任务树实况（task_code / 标题 / accept_status / progress / locked）
//   2. 哪些 task_code 是本仓 `spec.rs` **尚未登记**的（= 新活动的新任务）
//   3. 直接可粘贴的 spec.rs 登记骨架
//   4. 新活动的只读读数（订阅任务 / 徽章墙 / 成长档案 / 盲盒配额 / 抽奖 / 兑换汇总）
//   5. 全部原始响应落盘 growth-live-<uid8>.json（证据）
//
// ── 端点清单来源（不是猜的）──────────────────────────────────
// 逐条抄自线上前端产物
//   https://assets.workbuddy.cn/web/usercenter/ad5683203da67c1912ca00c315e90aa584433068/assets/growthSpace-Bhoa8Yzh.js
// （该 chunk 就是成长中心的 API 层，23 个端点；见 docs/growth-new-activity-report.md）
//
// ── 凭证来源（按顺序尝试）────────────────────────────────────
//   --token / --uid / --domain   直接给
//   AGENT2API_PROXY_HOME 或 ~/.agent2api/agent2api.db   从本机代理库的 accounts 表扫
//   --accounts-json <file>       从导出的账号 JSON 扫
// 扫库是**只读**的（打开文件读字节，不碰 SQLite 锁），不会影响正在运行的网关。
//
// ── 用法 ─────────────────────────────────────────────────────
//   node tools/growth-live-inspect.mjs                 # 自动找本机账号，逐个巡检
//   node tools/growth-live-inspect.mjs --dry          # 强制只读（默认只读）
//   node tools/growth-live-inspect.mjs --token ey... --uid 468bd250-... --domain copilot.tencent.com
//   node tools/growth-live-inspect.mjs --box-open 1   # 额外开 1 次盲盒（写动作，显式给才做）
//   node tools/growth-live-inspect.mjs --share        # 额外跑一次活跃地图分享（写动作）
//
// 退出码：0 = 至少一个账号巡检成功；1 = 没有可用凭证 / 全部失败。

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const CHAT_BASE = 'https://copilot.tencent.com';
const BILL_BASE = 'https://www.codebuddy.cn';
const WEB_BASE = 'https://www.workbuddy.cn';
const CLI_UA = 'CLI/2.63.2 CodeBuddy/2.63.2';
const WEB_UA =
  'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/152.0.0.0 Safari/537.36';
const TIMEOUT_MS = 20000;

/** 线上前端 growthSpace-Bhoa8Yzh.js 里的 23 个端点，逐条对齐。 */
const ENDPOINTS = [
  { id: 'profile', domain: 'chat', method: 'GET', path: '/v2/activity/growth/profile' },
  { id: 'subscribe_status', domain: 'chat', method: 'GET', path: '/v2/activity/growth/subscribe-task/status' },
  { id: 'tasks', domain: 'chat', method: 'GET', path: '/v2/activity/growth/tasks' },
  { id: 'tasks_mp', domain: 'chat', method: 'GET', path: '/v2/activity/growth/tasks', mp: true },
  { id: 'task_accept', domain: 'chat', method: 'POST', path: '/activity/growth/tasks/accept', body: (ctx) => ({ task_codes: ctx.accept || [] }) },
  { id: 'task_claim', domain: 'chat', method: 'POST', path: (ctx) => `/activity/growth/tasks/${ctx.claim}/claim` },
  { id: 'badges', domain: 'chat', method: 'GET', path: '/v2/activity/growth/badges' },
  { id: 'energy', domain: 'chat', method: 'GET', path: '/activity/growth/energy' },
  { id: 'streak', domain: 'chat', method: 'GET', path: '/activity/growth/streak' },
  { id: 'heatmap', domain: 'chat', method: 'GET', path: '/activity/growth/heatmap' },
  { id: 'heatmap_share', domain: 'chat', method: 'POST', path: '/activity/growth/heatmap/share', body: () => ({}), write: true },
  { id: 'redeem_summary', domain: 'chat', method: 'GET', path: '/activity/growth/redeem/summary' },
  { id: 'redeem', domain: 'chat', method: 'POST', path: '/activity/growth/redeem', body: (ctx) => ({ tier: ctx.tier || '7d', client_token: ctx.clientToken }), write: true },
  { id: 'makeup_use', domain: 'chat', method: 'POST', path: '/activity/growth/makeup-cards/use', body: (ctx) => ({ target_date: ctx.targetDate }), write: true },
  { id: 'lottery_summary', domain: 'chat', method: 'GET', path: '/activity/growth/lottery/summary' },
  { id: 'lottery_prizes', domain: 'chat', method: 'GET', path: '/activity/growth/lottery/prizes' },
  { id: 'lottery_chances', domain: 'chat', method: 'GET', path: '/activity/growth/lottery/chances' },
  { id: 'lottery_chances_logs', domain: 'chat', method: 'GET', path: '/activity/growth/lottery/chances/logs?page=1&page_size=20' },
  { id: 'lottery_draws', domain: 'chat', method: 'GET', path: '/activity/growth/lottery/draws?page=1&page_size=20' },
  { id: 'lottery_rewards', domain: 'chat', method: 'GET', path: '/activity/growth/lottery/rewards?page=1&page_size=20' },
  { id: 'lottery_draw', domain: 'chat', method: 'POST', path: '/activity/growth/lottery/draw', body: (ctx) => ({ client_token: ctx.clientToken }), write: true },
  { id: 'buddy_info', domain: 'chat', method: 'GET', path: '/activity/growth/buddy/info' },
  { id: 'buddy_list', domain: 'chat', method: 'GET', path: '/activity/growth/buddy/list' },
  { id: 'buddy_templates', domain: 'chat', method: 'GET', path: '/activity/growth/buddy/templates' },
  { id: 'buddy_quota', domain: 'chat', method: 'GET', path: '/activity/growth/buddy/quota' },
  { id: 'buddy_open', domain: 'chat', method: 'POST', path: '/activity/growth/buddy/open', body: (ctx) => ({ count: ctx.boxOpen || 1 }), write: true },
  { id: 'buddy_first', domain: 'chat', method: 'POST', path: '/activity/growth/buddy/first', body: () => ({}), write: true },
  { id: 'buddy_switch', domain: 'chat', method: 'POST', path: '/activity/growth/buddy/switch', body: (ctx) => ({ instance_id: ctx.instanceId || '' }), write: true },
  { id: 'buddy_agreement', domain: 'chat', method: 'GET', path: '/activity/growth/buddy/agreement' },
  { id: 'buddy_visible', domain: 'chat', method: 'GET', path: '/activity/growth/buddy/visible' },
  { id: 'buddy_travel_config', domain: 'chat', method: 'GET', path: '/activity/growth/buddy/travel/config' },
  { id: 'buddy_travel_status', domain: 'chat', method: 'GET', path: '/activity/growth/buddy/travel/status' },
  { id: 'buddy_travel_records', domain: 'chat', method: 'GET', path: '/activity/growth/buddy/travel/records?page=1&page_size=20' },
  { id: 'user_state_dismiss', domain: 'chat', method: 'POST', path: '/activity/growth/user-state/dismiss', body: (ctx) => ({ scene_key: ctx.sceneKey || 'growth_onboarding' }), write: true },
  { id: 'report_probe', domain: 'bill', method: 'GET', path: '/v2/activity/workbuddy/banner' },
  { id: 'web_probe', domain: 'web', method: 'GET', path: '/profile/growth-center' },
];

/** 本仓 spec.rs 已登记的 task_code（未出现在这里的任务码 = 新活动待登记）。 */
const REGISTERED = [
  'create_canvas', 'template_5', 'expert_5', 'Expert_team_use_3', 'skill_1', 'automation_1',
  'playbook_prompt', 'Expert_lighthouse', 'Hp_Appearance', 'chat_5', 'Model_chat_GLM5.2',
  'black_cat', 'first_buddy', 'RichMeow_Chat', 'Buddy_App', 'Buddy_App_QQ', 'Library_read',
  'Sequential_Tasks_1', 'Sequential_Tasks_2', 'Sequential_Tasks_3', 'Sequential_Tasks_4',
  'Sequential_Tasks_5', 'school_season', 'Expert_Philanthropy',
  // 2026-10-20 实况登记（见 docs/growth-new-activity.md §0）
  'Sequential_Tasks_6', 'Sequential_Tasks_7', 'WorkBuddy_Valorant', 'wb_wechat_oa_subscribe_task',
];

const args = process.argv.slice(2);
const flag = (name) => args.includes(`--${name}`);
// 取 `--name value`：值缺省（后面紧跟另一个开关）当空串，而不是当布尔 true ——
// `--token ""` 是用户真实输入的空凭证，必须识别为「没给」，不能变成字符串 "true"。
const opt = (name, fallback = '') => {
  const i = args.indexOf(`--${name}`);
  if (i === -1) return fallback;
  const next = args[i + 1];
  if (next === undefined) return fallback;
  if (next.startsWith('--')) return fallback;
  return next;
};

function color(text, code) {
  return process.stdout.isTTY ? `\u001b[${code}m${text}\u001b[0m` : text;
}

// ── 凭证发现：从本机 agent2api 库/导出文件扫账号（只读字节）──────────

/** 在任意文本里按花括号配平抽 JSON 对象（字符串感知，够用且不引依赖）。 */
function extractJsonObjects(text) {
  const out = [];
  for (let start = text.indexOf('{'); start !== -1; start = text.indexOf('{', start + 1)) {
    let depth = 0;
    let inString = false;
    let escaped = false;
    for (let i = start; i < text.length; i += 1) {
      const ch = text[i];
      if (inString) {
        if (escaped) escaped = false;
        else if (ch === '\\') escaped = true;
        else if (ch === '"') inString = false;
        continue;
      }
      if (ch === '"') inString = true;
      else if (ch === '{') depth += 1;
      else if (ch === '}') {
        depth -= 1;
        if (depth === 0) {
          const slice = text.slice(start, i + 1);
          if (slice.includes('accessToken')) out.push(slice);
          start = i;
          break;
        }
      }
    }
  }
  return out;
}

function candidateFromSession(session, source, accountId = '') {
  const auth = session?.auth || {};
  const account = session?.account || {};
  const token = String(auth.accessToken || '').trim();
  if (!token) return null;
  return {
    source,
    accountId,
    token,
    uid: String(account.uid || '').trim(),
    nick: String(account.nickname || account.nick || '').trim(),
    domain: String(auth.domain || 'copilot.tencent.com').trim(),
    provider: 'workbuddy',
  };
}

function scanFile(filePath, source) {
  const found = [];
  let raw = '';
  try {
    raw = fs.readFileSync(filePath, 'latin1'); // 逐字节，避免 utf8 解码截断
  } catch {
    return found;
  }
  for (const blob of extractJsonObjects(raw)) {
    let parsed = null;
    try {
      parsed = JSON.parse(blob.replace(/\\"/g, '"'));
    } catch {
      try {
        parsed = JSON.parse(blob);
      } catch {
        continue;
      }
    }
    const session = parsed?.session || parsed;
    const candidate = candidateFromSession(session, source, parsed?.id || parsed?.accountId || '');
    if (candidate) found.push(candidate);
  }
  return found;
}

function discoverAccounts() {
  const explicit = [];
  if (opt('token')) {
    explicit.push({
      source: 'cli',
      accountId: opt('account-id'),
      token: opt('token'),
      uid: opt('uid'),
      nick: opt('nick'),
      domain: opt('domain', 'copilot.tencent.com'),
      provider: opt('provider', 'workbuddy'),
    });
  }
  if (explicit.length) return explicit;

  const accountsJson = opt('accounts-json');
  if (accountsJson) {
    const found = scanFile(accountsJson, `file:${accountsJson}`);
    if (found.length) return found;
  }

  const configDir =
    opt('config-dir') ||
    process.env.AGENT2API_PROXY_HOME ||
    process.env.WORKBUDDY_PROXY_HOME ||
    path.join(os.homedir(), '.agent2api');
  const dbPath = path.join(configDir, 'agent2api.db');
  const found = [];
  for (const suffix of ['', '-wal', '-shm']) {
    const file = dbPath + suffix;
    if (fs.existsSync(file)) found.push(...scanFile(file, `store:${file}`));
  }
  // 去重（同 token 只留一条）
  const seen = new Set();
  return found.filter((item) => {
    const key = `${item.token}|${item.uid}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

// ── HTTP ─────────────────────────────────────────────────────

function headersFor(account, domain, mp) {
  const base = {
    Authorization: `Bearer ${account.token}`,
    Accept: 'application/json',
    'Content-Type': 'application/json',
    'User-Agent': domain === 'web' ? WEB_UA : CLI_UA,
    Origin: 'https://www.codebuddy.cn',
    Referer: 'https://www.codebuddy.cn/',
  };
  if (account.uid) base['X-User-Id'] = account.uid;
  if (account.domain) base['X-Domain'] = account.domain;
  if (mp) base['X-Client-Platform'] = 'miniprogram';
  if (domain === 'web') {
    base.Origin = WEB_BASE;
    base.Referer = `${WEB_BASE}/profile/growth-center`;
    base['X-Client-Platform'] = 'web';
  }
  return base;
}

async function call(account, endpoint, ctx) {
  const domain = endpoint.domain;
  const base = domain === 'chat' ? CHAT_BASE : domain === 'bill' ? BILL_BASE : WEB_BASE;
  const pathValue = typeof endpoint.path === 'function' ? endpoint.path(ctx) : endpoint.path;
  const url = pathValue.startsWith('http') ? pathValue : `${base}${pathValue}`;
  const body = endpoint.body && endpoint.method !== 'GET' ? endpoint.body(ctx) : undefined;
  const started = Date.now();
  try {
    const response = await fetch(url, {
      method: endpoint.method,
      headers: headersFor(account, domain, endpoint.mp),
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: AbortSignal.timeout(TIMEOUT_MS),
    });
    const text = await response.text();
    let payload = null;
    try {
      payload = JSON.parse(text);
    } catch {
      payload = text.slice(0, 400);
    }
    return { url, method: endpoint.method, status: response.status, ms: Date.now() - started, payload };
  } catch (error) {
    return { url, method: endpoint.method, status: 0, ms: Date.now() - started, error: String(error?.message || error) };
  }
}

/** 取业务信封的 data 段（成长域统一 {code,msg,data}）。 */
function dataOf(result) {
  const payload = result?.payload;
  if (!payload || typeof payload !== 'object') return null;
  const code = payload.code;
  if (code !== undefined && code !== 0 && code !== '0') return null;
  return payload.data ?? payload;
}

function envelopeNote(result) {
  const payload = result?.payload;
  if (result?.error) return `网络错误 ${result.error}`;
  if (!payload || typeof payload !== 'object') return `非 JSON（HTTP ${result.status}）`;
  const code = payload.code ?? payload.error_code ?? '-';
  const msg = payload.msg || payload.message || payload.desc || '';
  return `HTTP ${result.status} code=${code} ${msg}`.trim();
}

function brief(value, limit = 140) {
  if (value === null || value === undefined) return '';
  const text = typeof value === 'string' ? value : JSON.stringify(value);
  return text.length > limit ? `${text.slice(0, limit)}…` : text;
}

// ── spec.rs 骨架生成 ────────────────────────────────────────

function specSnippet(tasks) {
  const unknown = tasks.filter((task) => !REGISTERED.includes(task.code));
  if (!unknown.length) return '// 全部任务码都已登记，无需改动 spec.rs。';
  const lines = ['// ── 待登记（实况巡检发现，逐条核对判据后再落地）──'];
  for (const task of unknown) {
    const code = task.code.replace(/"/g, '\\"');
    lines.push(`    // title=${task.title} | desc=${task.description}`);
    lines.push(`    // 现状态=${task.status} 进度=${task.current}/${task.target} locked=${task.locked} 奖励=${task.reward}`);
    lines.push(`    "${code}" => TaskSpec::new("TODO_KIND", ${task.target || 1}, "TODO_对象来源", false),`);
  }
  return lines.join('\n');
}

// ── 单账号巡检 ───────────────────────────────────────────────

function normalizeTask(task) {
  const progress = task?.progress || {};
  return {
    code: String(task?.task_code || ''),
    title: String(task?.title || ''),
    description: String(task?.description || ''),
    status: String(task?.accept_status || ''),
    current: Number(progress.current || 0),
    target: Number(progress.target || 0),
    locked: Boolean(task?.locked),
    unlockAt: task?.unlock_at ?? null,
    reward: brief({ credit: task?.credit ?? task?.reward_credit, energy: task?.energy ?? task?.reward_energy }, 80),
    raw: task,
  };
}

function uuidV4() {
  // 抽奖/兑换要一次性 client_token；用系统熵源，失败回落时间戳形态。
  const bytes = new Uint8Array(16);
  try {
    globalThis.crypto.getRandomValues(bytes);
  } catch {
    for (let i = 0; i < 16; i += 1) bytes[i] = Math.floor(Math.random() * 256);
  }
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = [...bytes].map((b) => b.toString(16).padStart(2, '0')).join('');
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

async function inspect(account) {
  const ctx = {
    clientToken: uuidV4(),
    targetDate: opt('target-date'),
    tier: opt('tier'),
    accept: opt('accept') ? opt('accept').split(',') : [],
    claim: opt('claim'),
    instanceId: opt('instance-id'),
    sceneKey: opt('scene-key'),
    boxOpen: Number(opt('box-open', '0')) || 0,
  };

  const results = {};
  const evidence = { account: { uid: account.uid, nick: account.nick, source: account.source }, endpoints: {} };

  const run = async (id) => {
    const endpoint = ENDPOINTS.find((item) => item.id === id);
    const result = await call(account, endpoint, ctx);
    evidence.endpoints[id] = result;
    results[id] = result;
    return result;
  };

  // 顺序照线上前端：先刷新订阅任务状态（解锁订阅类任务），再拉任务列表。
  await run('subscribe_status');
  const tasksResult = await run('tasks');
  await run('tasks_mp');

  const tasks = [];
  for (const source of [tasksResult, results.tasks_mp]) {
    const data = dataOf(source);
    const list = Array.isArray(data?.tasks) ? data.tasks : [];
    for (const task of list) {
      const row = normalizeTask(task);
      if (row.code && !tasks.some((item) => item.code === row.code)) tasks.push(row);
    }
  }

  const readOnly = [
    'profile', 'badges', 'energy', 'streak', 'heatmap', 'redeem_summary',
    'lottery_summary', 'lottery_prizes', 'lottery_chances', 'lottery_chances_logs',
    'lottery_draws', 'lottery_rewards', 'buddy_info', 'buddy_list', 'buddy_templates',
    'buddy_quota', 'buddy_agreement', 'buddy_visible', 'buddy_travel_config',
    'buddy_travel_status', 'buddy_travel_records', 'report_probe', 'web_probe',
  ];
  for (const id of readOnly) await run(id);

  if (flag('share')) await run('heatmap_share');
  if (ctx.boxOpen > 0) await run('buddy_open');
  if (ctx.claim) await run('task_claim');
  if (ctx.accept.length) await run('task_accept');
  if (flag('redeem') && ctx.tier) await run('redeem');
  if (ctx.targetDate) await run('makeup_use');
  if (flag('lottery-draw')) await run('lottery_draw');

  return { tasks, results, evidence, ctx };
}

function render(account, run) {
  const { tasks, results } = run;
  const unknown = tasks.filter((task) => !REGISTERED.includes(task.code));
  const uid8 = (account.uid || 'unknown').slice(0, 8);
  console.log('');
  console.log(color(`═══ 账号 ${uid8} (${account.nick || '未命名'}) 来源 ${account.source}`, '1;36'));
  console.log(`  任务数：${tasks.length}    已登记：${tasks.length - unknown.length}    待登记：${unknown.length}`);
  console.log('  ── 任务实况 ──');
  for (const task of tasks) {
    const mapped = REGISTERED.includes(task.code);
    const mark = mapped ? color('已登记', '32') : color('新活动?', '33');
    console.log(
      `   ${mark} ${task.code.padEnd(24)} ${String(task.status).padEnd(14)} ` +
        `${task.current}/${task.target}${task.locked ? ` locked(${task.unlockAt})` : ''}  ${task.title}`,
    );
  }
  if (unknown.length) {
    console.log('');
    console.log(color('  ── 未登记任务（就是这次的新活动条目）──', '1;33'));
    for (const task of unknown) {
      console.log(`   • ${task.code}`);
      console.log(`     标题：${task.title}`);
      console.log(`     说明：${task.description}`);
      console.log(`     状态：${task.status}  进度：${task.current}/${task.target}  奖励：${task.reward}`);
    }
    console.log('');
    console.log('  ── 可直接粘贴进 growth/spec.rs 的骨架 ──');
    console.log(`pub fn spec_of(code: &str) -> Option<TaskSpec> {\n    let spec = match code {\n${specSnippet(tasks)}\n        _ => return None,\n    };\n    Some(spec)\n}`);
  }
  console.log('');
  console.log('  ── 新活动只读读数 ──');
  const probeIds = [
    ['成长档案 profile', 'profile'],
    ['订阅任务 subscribe', 'subscribe_status'],
    ['徽章墙 badges', 'badges'],
    ['能量 energy', 'energy'],
    ['连登 streak', 'streak'],
    ['活跃地图 heatmap', 'heatmap'],
    ['兑换汇总 redeem', 'redeem_summary'],
    ['抽奖汇总 lottery', 'lottery_summary'],
    ['抽奖奖池 prizes', 'lottery_prizes'],
    ['抽奖次数 chances', 'lottery_chances'],
    ['盲盒配额 quota', 'buddy_quota'],
    ['盲盒图鉴 list', 'buddy_list'],
    ['猫猫模板 templates', 'buddy_templates'],
    ['旅行配置 travel.config', 'buddy_travel_config'],
    ['旅行记录 travel.records', 'buddy_travel_records'],
  ];
  for (const [label, id] of probeIds) {
    const result = results[id];
    const data = dataOf(result);
    const ok = data !== null;
    const stamp = ok ? color('OK ', '32') : color('-- ', '31');
    const detail = ok ? brief(data, 150) : envelopeNote(result);
    console.log(`   ${stamp} ${label.padEnd(26)} ${detail}`);
  }
  if (flag('share') || run.ctx.boxOpen > 0 || flag('lottery-draw')) {
    console.log('');
    console.log('  ── 写动作结果（显式开启才跑）──');
    for (const [label, id] of [['活跃地图分享', 'heatmap_share'], ['开盲盒', 'buddy_open'], ['抽奖', 'lottery_draw']]) {
      if (!results[id]) continue;
      const data = dataOf(results[id]);
      console.log(`   ${data !== null ? color('OK ', '32') : color('-- ', '31')} ${label.padEnd(26)} ${data !== null ? brief(data, 150) : envelopeNote(results[id])}`);
    }
  }
  return { unknown: unknown.length, taskCount: tasks.length };
}

// ── 交互式取凭证（`--interactive`；双击 cmd 走这条路）────────────
//
// 中文提示全部留在这个 JS 里而不是 .cmd 里：cmd.exe 按系统 ANSI 代码页解析
// 批处理文件，UTF-8 中文会把 `(` `)` 那段解析坏（实测直接报「系统找不到指定
// 的驱动器」）。所以 .cmd 只做两件事：chcp 65001 + 调 Node；文案在这里。
async function promptCredentials() {
  const { createInterface } = await import('node:readline/promises');
  const rl = createInterface({ input: process.stdin, output: process.stdout });
  try {
    console.log('');
    console.log('============================================================');
    console.log(' 成长计划实况拉取（只读，不改你账号的任何状态）');
    console.log('------------------------------------------------------------');
    console.log(' 需要你贴一次登录凭证，取法：');
    console.log('   1. 浏览器打开 https://www.workbuddy.cn/profile/growth-center 并登录');
    console.log('   2. 按 F12 → 切到 Network（网络）面板 → 刷新页面');
    console.log('   3. 在请求列表里点名字带 growth 的那条（例如 tasks）');
    console.log('   4. 右侧 Headers → Request Headers 里找 Authorization');
    console.log('      Authorization: Bearer eyJhbGciOi...');
    console.log('   5. 只复制 eyJ 开头那一整串（不要带 "Bearer " 和空格）');
    console.log('');
    console.log(' 这串凭证 = 你本机登录态：几周自然过期，网页「退出登录」即刻失效；');
    console.log(' 脚本只把它放进发往 copilot.tencent.com 的请求头，不落盘、不外传。');
    console.log('============================================================');
    const token = (await rl.question('\n粘贴 eyJ 开头的凭证，回车：')).trim();
    if (!token) {
      console.error('\n[×] 没有输入凭证，退出。');
      return [];
    }
    const uid = (await rl.question('你的 uid（不知道直接回车留空）：')).trim();
    const domainInput = (await rl.question('domain（不知道直接回车，默认 copilot.tencent.com）：')).trim();
    return [
      {
        source: 'interactive',
        accountId: '',
        token,
        uid,
        nick: '',
        domain: domainInput || 'copilot.tencent.com',
        provider: 'workbuddy',
      },
    ];
  } finally {
    rl.close();
  }
}

// ── main ────────────────────────────────────────────────────

async function main() {
  const accounts = flag('interactive') ? await promptCredentials() : discoverAccounts();
  if (!accounts.length) {
    console.error('没有找到可用凭证。三条路选一条：');
    console.error('  1) 双击「拉取成长计划数据.cmd」（交互式，贴一次凭证即可）');
    console.error('  2) node tools/growth-live-inspect.mjs --token <accessToken> --uid <uid> [--domain copilot.tencent.com]');
    console.error('  3) 让本机网关跑起来并在 ~/.agent2api/agent2api.db 里有 WorkBuddy 国内版账号（默认自动扫）');
    process.exit(1);
  }
  const only = opt('account-id');
  const targets = only ? accounts.filter((item) => item.accountId === only || item.uid === only) : accounts;
  if (!targets.length) {
    console.error(`没有匹配的账号：${only}（发现 ${accounts.length} 个）`);
    process.exit(1);
  }

  const outDir = opt('out-dir', process.cwd());
  let ok = 0;
  let totalUnknown = 0;
  for (const account of targets) {
    try {
      const run = await inspect(account);
      const stat = render(account, run);
      totalUnknown += stat.unknown;
      const uid8 = (account.uid || 'unknown').slice(0, 8);
      const file = path.join(outDir, `growth-live-${uid8}.json`);
      fs.writeFileSync(
        file,
        JSON.stringify({ generatedAt: new Date().toISOString(), account: { uid: account.uid, nick: account.nick }, tasks: run.tasks, endpoints: run.evidence.endpoints }, null, 2),
      );
      console.log(`   证据落盘：${file}`);
      ok += 1;
    } catch (error) {
      console.error(`账号 ${account.uid || account.source} 巡检异常：${error?.message || error}`);
    }
  }
  console.log('');
  console.log(`收口：巡检 ${ok}/${targets.length} 个账号，发现 ${totalUnknown} 个未登记任务码。`);
  process.exit(ok ? 0 : 1);
}

main();

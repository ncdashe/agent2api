#!/usr/bin/env node
// 补丁落地器：把 patch/growth-new-activity.patch 打进本仓工作区，然后做一轮自检。
//
// 为什么用 Node 而不是 .cmd 里直接写 git 命令：cmd.exe 按系统 ANSI 代码页解析
// 批处理文件，UTF-8 中文会把 `if not exist (...) (` 这类块解析坏（实测报
// 「系统找不到指定的驱动器」）。所以 .cmd 只保留 ASCII 两行，逻辑与中文提示
// 都在这里 —— Node 读写 UTF-8 不受代码页影响。
//
// 用法：node tools/apply-patch.mjs [补丁路径]
//   - 已应用过（反向 apply 检查通过）→ 跳过，直接自检；
//   - 干净树 → 直接 git apply；
//   - 有冲突 → 走 git apply --3way，再不行就报错并打印可复制的排查命令。

import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, '..');
const patchPath = path.resolve(
  process.argv[2] || path.join(repoRoot, '..', 'patch', 'growth-new-activity.patch'),
);

function run(command, args, options = {}) {
  // stdio: 'inherit' —— 输出直通控制台，同时拿到退出码（不抓管道，避免受限环境 EPERM）。
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    stdio: 'inherit',
    shell: process.platform === 'win32',
    ...options,
  });
  return result.status === null ? 1 : result.status;
}

function head(text) {
  console.log('');
  console.log('============================================');
  console.log(` ${text}`);
  console.log('============================================');
}

head('1/4 检查仓库与补丁');
if (!fs.existsSync(path.join(repoRoot, '.git'))) {
  console.error(`[×] ${repoRoot} 不是 git 仓库`);
  process.exit(1);
}
if (!fs.existsSync(patchPath)) {
  console.error(`[×] 找不到补丁文件：${patchPath}`);
  console.error('    把 growth-new-activity.patch 放到 <项目根>/patch/ 下，或作为第一个参数传路径');
  process.exit(1);
}
console.log(`仓库：${repoRoot}`);
console.log(`补丁：${patchPath}（${fs.statSync(patchPath).size} 字节）`);

head('2/4 应用补丁');
const applied = run('git', ['apply', '--check', '--reverse', patchPath]) === 0;
if (applied) {
  console.log('[√] 改动已经在工作区里了（补丁应用过），跳过');
} else {
  const clean = run('git', ['apply', '--check', patchPath]) === 0;
  if (clean) {
    if (run('git', ['apply', patchPath]) !== 0) {
      console.error('[×] git apply 失败（看上面的 git 报错）');
      process.exit(1);
    }
    console.log('[√] 补丁已应用到工作区（未提交）');
  } else {
    console.log('[!] 直接应用有冲突，改走三方合并：');
    if (run('git', ['apply', '--3way', patchPath]) !== 0) {
      console.error('[×] 三方合并也失败。把上面的 git 报错整段发回来，我按你当前版本重做补丁。');
      console.error(`    手动排查：git apply --check --verbose "${patchPath}"`);
      process.exit(1);
    }
    console.log('[√] 三方合并完成（可能有需要你确认的冲突标记）');
  }
}

head('3/4 变更清单');
run('git', ['--no-pager', 'diff', '--stat']);

head('4/4 自检');
console.log('· 巡检器语法：');
const syntax = run('node', ['--check', path.join(repoRoot, 'tools', 'growth-live-inspect.mjs')]);
console.log(syntax === 0 ? '  [√] 通过' : '  [×] 失败');

console.log('· 后端编译（本机有 cargo 才跑）：');
const cargoProbe = spawnSync('cargo', ['--version'], { stdio: 'ignore', shell: process.platform === 'win32' });
if (cargoProbe.status === 0) {
  run('cargo', ['check', '--manifest-path', path.join(repoRoot, 'desktop-tauri', 'src-tauri', 'Cargo.toml'), '-p', 'agent2api-server']);
} else {
  console.log('  [!] 本机没有 cargo，跳过（CI 的 build 工作流会跑）');
}

head('完成');
console.log('下一步：');
console.log('  1) 双击「拉取成长计划数据.cmd」→ 贴一次凭证 → 生成 probe/growth-live-*.json');
console.log('  2) 把那个 json 发回来，我按真实任务码补 spec.rs 与事件构造器');
console.log('  3) 要提交就执行：git add -A && git commit -m "feat(growth): 新活动端点与未登记任务发现"');

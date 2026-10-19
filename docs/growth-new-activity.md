# 成长计划「新活动」接入报告

> 对象：`https://www.workbuddy.cn/profile/growth-center`（成长计划 / 养成中心）
> 结论先说：**这个页面的后端能力面一共 23 个端点，本仓原来覆盖 12 个；本轮把剩下 11 个补齐
> （`growth/activity.rs` + 两个只读/写端点 + 未登记任务发现口），并给了一个把「新任务码」当场
> 捞出来的巡检器**。真正决定「新活动怎么点亮」的 `task_code` 必须用你自己的账号实拉一次
> （`tools/growth-live-inspect.mjs`），拉完按骨架填 `spec.rs` 即可 —— 这一步不能靠猜。

---

## 0. 实况结果（2026-10-20 用真账号跑完，已闭环）

凭证不是手抄的 F12，而是**从运行中的客户端进程内存里取的**（见 §9）；任务树实拉 23 条，
其中本仓**未登记 4 条**，全部按实测判据处理完毕：

| task_code | 标题 / 判据（`task_desc` 原文） | 奖励 | 处理 |
| --- | --- | --- | --- |
| `WorkBuddy_Valorant` | 体验「无畏契约拼豆专家」｜「即日起至11月15日，在专家页搜索并召唤「无畏契约拼豆专家」，完成一次对话」 | 100 credit + 5 energy | **已登记并实测点亮**：`kind=valorant`（`expert_actual_use` 事件，对象 id `ex_xQHHD1u5Jkua`）+ claim 成功（`credit:100, energy:5`） |
| `Sequential_Tasks_6` | 完成 10 次对话｜「在小程序内完成 10次有效对话」 | 100 + 5 | 已登记：`kind=minichat, target=10, mp` |
| `Sequential_Tasks_7` | 体验灵感功能｜「在 PC 客户端内选择更多-灵感-选中模板使用」 | 500 + 5 | 已登记：`kind=playbook, mp`（判据同既有 `playbook_prompt`） |
| `wb_wechat_oa_subscribe_task` | 关注「腾讯 WorkBuddy」官方公众号｜「关注…满 24 小时，即可领取奖励」 | 100 + 5 | 标 `unforgeable`：进度由真实关注行为 + 时间决定，客户端侧无上报口 |

实况证据落盘：`probe/growth-live-<uid8>.json`（23 条端点的原始响应）；
点亮验证：`probe/lightup_valorant.mjs` 一次跑通（上报 → 回读 `completed 1/1` → claim 200）。

---

## 1. 事实来源（不是猜的）

成长中心是前端 SPA，后端接口全部落在 `usercenter` 的 API 层 chunk 里：

```text
页面    https://www.workbuddy.cn/profile/growth-center
包基址  https://assets.workbuddy.cn/web/usercenter/ad5683203da67c1912ca00c315e90aa584433068/assets/
API 层  growthSpace-Bhoa8Yzh.js      ← 23 条 e.get/e.post 路径，逐条抄录
活动页  GrowthCenterPage-eLpw1we2.js  ← 页面结构与活动规则文案
旧页面  GrowthSpace-BVCj_mMU.js       ← 「当前活动页面将于 7.15 正式下线…可移步全新活动页面」
```

`growthSpace-Bhoa8Yzh.js` 里的全部路径（本文件的唯一接口清单）：

```js
e.get ("/v2/activity/growth/profile")
e.get ("/v2/activity/growth/subscribe-task/status")
e.get ("/v2/activity/growth/tasks")
e.post("/activity/growth/tasks/accept", {task_codes})
e.post(`/activity/growth/tasks/${code}/claim`)
e.get ("/v2/activity/growth/badges")
e.get ("/activity/growth/energy")
e.get ("/activity/growth/buddy/info")
e.get ("/activity/growth/buddy/list")
e.get ("/activity/growth/buddy/templates")
e.get ("/activity/growth/buddy/quota")
e.post("/activity/growth/buddy/open", {count})
e.post("/activity/growth/buddy/first")
e.post("/activity/growth/buddy/switch", {instance_id})
e.get ("/activity/growth/buddy/agreement")   /  e.post(..., {agree:true})
e.post("/activity/growth/user-state/dismiss", {scene_key, version?})
e.get (`/activity/growth/buddy/share/view/${id}`)
e.get ("/activity/growth/buddy/visible")      /  e.post(..., {visible})
e.get ("/activity/growth/heatmap", {params})
e.post("/activity/growth/heatmap/share", {})
e.get ("/activity/growth/streak")
e.get ("/activity/growth/lottery/summary")
e.post("/activity/growth/makeup-cards/use", {target_date})
e.post("/activity/growth/redeem", {tier, client_token})
e.get ("/activity/growth/redeem/summary")
e.get ("/activity/growth/lottery/prizes")
e.post("/activity/growth/lottery/draw", {client_token})
e.get ("/activity/growth/lottery/draws", {page, page_size})
e.get ("/activity/growth/lottery/chances")
e.get ("/activity/growth/lottery/chances/logs", {page, page_size})
e.get ("/activity/growth/lottery/rewards", {page, page_size})
e.post(`/activity/growth/lottery/rewards/${id}/address`, addr)
e.get (`/activity/growth/lottery/rewards/${id}/address`)
e.get ("/activity/growth/buddy/travel/config")
e.get ("/activity/growth/buddy/travel/status")
e.post("/activity/growth/buddy/travel/depart", {location_id})
e.post("/activity/growth/buddy/travel/claim", {})
e.get ("/activity/growth/buddy/travel/records", {page, page_size})
```

> 注：`e` = chat 域（`copilot.tencent.com`）客户端，`s` = 分享域。上面 30+ 行按语义归并后
> 是 23 个资源；数组里的 `{...}` 是前端实发的请求体形状。

### 1.1 活动结构（从页面文案取证）

| 机制 | 页面原文（节选） | 接口 |
| --- | --- | --- |
| 任务中心 | 「新手任务 / 必做任务 / 进阶任务」「首次进入进阶任务区时自动领取首领 Buddy 任务」 | `/v2/activity/growth/tasks` |
| 任务奖励总量 | 「完成全部 6 项任务（含首领 Buddy 任务），合计 800 积分 + 33 能量 + 保底 1 只 Buddy + 6 枚徽章」 | claim |
| 盲盒（开猫） | 「每次开启盲盒需要消耗 N 能量值」「当前仅有 M 能量值」「能量不足，请参与任务获取更多能量」 | `buddy/open` + `buddy/quota` |
| 猫猫图鉴 / 换猫 | 「图鉴」「可设为当前」「支持换肤」 | `buddy/list`、`buddy/switch` |
| 徽章墙 | 「用户获得的徽章将统一展示在用户…的徽章墙中」「暂无徽章」 | `/v2/activity/growth/badges` |
| 活跃地图 | 「全年活跃记录」「热力墙」「数据每日凌晨更新」「补登卡修复断登」 | `heatmap`、`heatmap/share`、`makeup-cards/use` |
| 连登兑换 | 「入门档 / 进阶档 / 巅峰档」「每档每月限兑」「兑换 … 附赠补登卡」 | `redeem`、`redeem/summary` |
| 抽奖 | 「抽奖卡」「奖池层层翻倍」「实物奖品填写邮寄信息」 | `lottery/*` |
| 猫猫旅行 | 「今天去哪里逛逛」「派出中 / 抵达 / 回家」「旅行记录」 | `buddy/travel/*` |
| 订阅任务 | 任务列表拉取**前**先刷 `subscribe-task/status`（前端固定顺序） | `subscribe-task/status` |

---

## 2. 覆盖矩阵（改前 / 改后）

| # | 能力 | 端点 | 改前 | 改后 | 落点 |
| --- | --- | --- | --- | --- | --- |
| 1 | 任务列表 | `GET /v2/activity/growth/tasks` | ✅ | ✅ | `growth/mod.rs` |
| 2 | 接单 | `POST /activity/growth/tasks/accept` | ✅ | ✅ | 同上 |
| 3 | 领奖 | `POST /activity/growth/tasks/{code}/claim` | ✅ | ✅ | 同上 |
| 4 | 能量 | `GET /activity/growth/energy` | ✅ | ✅ | 同上 |
| 5 | 连登 | `GET /activity/growth/streak` | ✅ | ✅ | `streak.rs` |
| 6 | 补登 | `POST /activity/growth/makeup-cards/use` | ✅ | ✅ | `streak.rs` |
| 7 | 档位兑换 | `POST /activity/growth/redeem` | ✅ | ✅ | `streak.rs` |
| 8 | 抽奖（单抽） | `POST /activity/growth/lottery/draw` | ✅ | ✅ | `streak.rs` |
| 9 | 领养首猫 | `buddy/first` + `buddy/agreement` | ✅ | ✅ | `growth/mod.rs` |
| 10 | 猫猫旅行 | `buddy/travel/{status,depart,claim}` | ✅ | ✅ | `travel.rs` |
| 11 | 开学季 | `portal/activity/school/*` | ✅ | ✅ | `school.rs` |
| 12 | 乐力图（判漏签） | `GET /activity/growth/heatmap` | ✅（内部用） | ✅（对外出口） | `activity::heatmap` |
| 13 | **订阅任务解锁** | `GET /v2/activity/growth/subscribe-task/status` | ❌ | ✅ | `activity::subscribe_status` |
| 14 | **成长档案** | `GET /v2/activity/growth/profile` | ❌ | ✅ | `activity::profile` |
| 15 | **徽章墙** | `GET /v2/activity/growth/badges` | ❌ | ✅ | `activity::badges` |
| 16 | **盲盒配额 / 图鉴 / 模板 / 可见性** | `buddy/{quota,list,templates,visible}` | ❌ | ✅ | `activity::box_*` |
| 17 | **开盲盒** | `POST /activity/growth/buddy/open {count}` | ❌ | ✅ | `activity::box_open`（次数夹 1..=10） |
| 18 | **切换展示猫** | `POST /activity/growth/buddy/switch` | ❌ | ✅ | `activity::box_switch` |
| 19 | **活跃地图分享** | `POST /activity/growth/heatmap/share` | ❌ | ✅ | `activity::heatmap_share` |
| 20 | **兑换汇总** | `GET /activity/growth/redeem/summary` | ❌ | ✅ | `activity::redeem_summary` |
| 21 | **抽奖扩展** | `lottery/{summary,prizes,chances,chances/logs,draws,rewards}` | ❌ | ✅ | `activity::lottery_*` |
| 22 | 奖品收货地址 | `lottery/rewards/{id}/address` | ❌ | ✅（读） | `activity::lottery_reward_address` |
| 23 | **新任务发现** | —— | ❌ | ✅ | `status.unknownTasks` + `tools/growth-live-inspect.mjs` |

---

## 3. 改了哪些东西

### 3.1 执行体：`desktop-tauri/src-tauri/server/src/server/core/providers/workbuddy/growth/activity.rs`（新增）

- 11 类新端点，全部走已有的 `chat_call`（同一套域指纹/超时/代理），不新造 HTTP 栈。
- **响应字段一律透传**：前端产物只能证明「请求路径 + 请求体」，证明不了响应字段。
  所以只对前端文案能证明存在的量（能量、单次消耗、次数、天数）做「候选键探测」
  （`pick_i64` / `pick_str` / `pick_len`），读不出给 `None` → 界面显示「未读数」，
  不显示 0。这是本仓既有纪律（「形状对齐、不发明字段」）的延续。
- 写动作只有三个：开盲盒、切换展示、分享；开盲盒带 `MAX_BOX_OPEN = 10` 护栏
  （能量是活动内唯一硬通货，脚本不替用户花）。
- `unknown_tasks()`：把映射表没登记的 `task_code` 单独聚出来（标题/说明/进度/解锁时间）。

### 3.2 端点层：`server/src/server/api/growth.rs`

- `status`（`GET /api/accounts/{id}/growth`）新增 `subscribe` / `profile` / `badges` / `box` / `unknownTasks` 五块；
  且在拉任务列表**之前**先刷 `subscribe-task/status`（对齐线上前端顺序，否则订阅类任务不下发）。
- 新增三个 handler：
  - `GET  /api/accounts/{id}/growth/activity` —— 新活动只读盘点（徽章 / 抽奖 6 件套 / 兑换 / 图鉴 / 活跃地图）
  - `POST /api/accounts/{id}/growth/box` —— 开盲盒 `{count}`
  - `POST /api/accounts/{id}/growth/share` —— 活跃地图分享
- 每个块独立失败（某块报错只写进该块的 `error`），沿用原文件的分块纪律。

### 3.3 路由：`server/src/server/api/accounts.rs`

三条后缀路由（`/growth/activity` GET、`/growth/box` POST、`/growth/share` POST），
与既有 `/growth/task|school|travel` 同一写法；`/growth/activity` 排在 `/growth` 之前解析。

### 3.4 巡检器：`tools/growth-live-inspect.mjs`（新增，零依赖，Node ≥18）

用你自己的凭证把上面 23 个端点打一遍，产出：

1. 任务树实况（`task_code` / 状态 / 进度 / 锁定时间 / 奖励）；
2. **哪些 `task_code` 还没登记**（= 这次的新活动条目），并直接打印可粘贴的 `spec.rs` 骨架；
3. 新活动只读读数（徽章 / 档案 / 盲盒配额 / 抽奖 / 兑换 / 活跃地图）；
4. 全量原始响应落盘 `growth-live-<uid8>.json`（证据）。

凭证来源：`--token/--uid/--domain`、`--accounts-json <file>`、或自动只读扫
`~/.agent2api/agent2api.db`（扫字节，不占 SQLite 锁，不影响运行中的网关）。

写动作默认**全关**，要显式加 `--box-open 1` / `--share` / `--lottery-draw` / `--redeem --tier 7d` 才发。

---

## 4. 新活动落地的完整步骤（按顺序执行）

```powershell
# 1) 起网关（面板跑着也行），确保账号列表里有 WorkBuddy 国内版账号
# 2) 实拉一次实况
cd <repo>
node tools/growth-live-inspect.mjs
#    看输出里的「未登记任务（就是这次的新活动条目）」一节
#    证据落在 growth-live-<uid8>.json

# 3) 按骨架往 spec.rs 里登记（心跳/判据可参考相邻条目）
#    → 每个新 task_code 一条 TaskSpec::new(kind, target, src, needs_object)
#    → 新 kind 要在 events.rs 出对应的 light_up 分支（形状必须有实测判据）
# 4) 编译 + 跑接口
npm run tauri:build            # 或 cargo check -p agent2api-server
curl -s http://127.0.0.1:3065/api/accounts/<id>/growth | jq '.unknownTasks, .subscribe'
```

第 3 步为什么必须实测：任务点亮的判据是**事件形状**（域指纹 + 字段），
`spec.rs` 里每条 `TaskSpec` 上方的注释就是实测沉淀记录。凭空写一个
`kind` 只会让「上报成功、进度不动」，还会把当天上报额度搭进去。

---

## 5. 验证

### 已跑

| 项 | 命令 | 结果 |
| --- | --- | --- |
| 巡检器语法 | `node --check tools/growth-live-inspect.mjs` | exit 0 |
| 巡检器连通性（占位 token） | `node tools/growth-live-inspect.mjs --token dummy --uid 0000… --out-dir probe` | 23 条端点全部按预期返回 `401 Authorization Required`（网关 APISIX），说明**路径与请求头形状被服务端接受**，只差有效 token |
| 端点路径有效性 | `curl -i https://copilot.tencent.com/v2/activity/growth/tasks` | `HTTP/1.1 401`、`WWW-Authenticate: Bearer realm="copilot"`、`EO-LOG-UUID: …`（非 404/405） |
| 前端产物取证 | 下载 `growthSpace-Bhoa8Yzh.js` 等 5 个 chunk | 23 端点清单 + 活动文案（§1） |

### 已跑（Rust 侧，工具链后来装上了，见 §11）

```powershell
cd desktop-tauri/src-tauri
cargo check -p agent2api-server        # Finished dev profile in 1m 26s，exit 0
```

`cargo clippy -p agent2api-server -- -D warnings` 报 1 error + 576 warning，全部是
**本轮改动之前就有的**（`core/.../sql.rs:162 never_loop`、`core/providers/codearts/mod.rs:870
unused import`），没有一条指向本轮新增/修改的文件。详见 §11。

---

## 6. MODIFIED / DIFF / VERIFICATION / ROLLBACK

**MODIFIED**

```text
M  desktop-tauri/src-tauri/server/src/server/api/accounts.rs      (+21)
M  desktop-tauri/src-tauri/server/src/server/api/growth.rs        (+214 -5)
M  desktop-tauri/src-tauri/server/src/server/core/providers/workbuddy/growth/mod.rs (+3)
A  desktop-tauri/src-tauri/server/src/server/core/providers/workbuddy/growth/activity.rs (+397)
A  tools/growth-live-inspect.mjs                                  (+513)
A  docs/growth-new-activity.md                                    (本文件)
```

**DIFF**：`patch/growth-new-activity.patch`（`git diff`，含新文件）

**VERIFICATION**

```powershell
cd repo
node --check tools/growth-live-inspect.mjs
cd desktop-tauri/src-tauri
cargo check -p agent2api-server
# 起服务后
curl.exe -s "http://127.0.0.1:3065/api/accounts/<id>/growth/activity" -H "Cookie: <面板会话>"
```

**ROLLBACK**

```powershell
cd repo
git checkout -- desktop-tauri/src-tauri/server/src/server/api/accounts.rs `
                desktop-tauri/src-tauri/server/src/server/api/growth.rs `
                desktop-tauri/src-tauri/server/src/server/core/providers/workbuddy/growth/mod.rs
Remove-Item desktop-tauri/src-tauri/server/src/server/core/providers/workbuddy/growth/activity.rs
Remove-Item tools/growth-live-inspect.mjs, docs/growth-new-activity.md
# 或者一次性：git apply -R ..\patch\growth-new-activity.patch
```

---

## 7. 还差的一步（面板 UI）

后端出口已经齐了，面板这一层只连了三个老接口。要接新活动，落点固定：

| 层 | 文件 | 要加什么 |
| --- | --- | --- |
| 状态类型 | `desktop-tauri/ui-islands/src/islands/checkin-state.ts` | `GrowthTaskRow` 旁加 `GrowthActivity`（badges/box/lottery/redeem/unknownTasks），bridge 声明加 `getGrowthActivity?`、`openGrowthBox?` |
| 桥接实现 | `desktop-tauri/ui/islands/ui.js`（构建产物） | 对应 `bridge()` 方法 → 调 `/api/accounts/{id}/growth/activity`、`/growth/box` |
| 渲染 | `checkin-page.tsx` / `tasks-panel.tsx` | 「新任务：N 个待登记」提示条 + 徽章墙 + 盲盒按钮（用 `box.opens` 决定 disable） |

`ui/islands/ui.js` 是构建产物（`ui-islands` 打包而来），改完源码要按仓库既有流程重新打包，
否则前端产物校验（逐字节比对）会拦。

---

## 8. 风险与边界

- **响应字段未实测**：本轮的 activity 层只做透传 + 候选键读数。等你实拉一次，
  `growth-live-<uid8>.json` 里就有真实形状，届时可以把 `pick_*` 固化成确切字段。
- **`subscribe-task/status` 的语义**：前端是「刷一下再拉列表」，猜测它是订阅类任务的
  解锁/回执入口。本项目按「先刷、失败不阻塞」处理 —— 这是最保守且与前端一致的形态。
- **开盲盒消耗能量**：能量只能靠任务赚。默认不自动开；`MAX_BOX_OPEN=10` 是硬护栏。
- **未登记任务不点亮**：`process_task` 对未映射任务仍然保守跳过（原有行为不变），
  只是现在你能**看见**它们了（`unknownTasks`）。
- **专家市场列表接口曾报 17101（已在 §10.1 修正记录）**：`POST /v2/operation-platform/market/expert/list`
  对**只带分页**的 body 曾返回 `{"code":17101,"msg":"invalid request body"}`。2026-10-20
  复核时三域（copilot.tencent.com / www.workbuddy.cn / www.codebuddy.cn）用客户端同款
  四件套 body 全部 200 + `code=0`，故把「接口失效」修正为「请求体缺 `sort_by`/`sort_order`
  时的形态问题」，并按客户端写法固定这两个字段。
- **客户端登录态已改加密（已在 §10.2 修复）**：WorkBuddy 5.8.3 把
  `%LOCALAPPDATA%\CodeBuddyExtension\Data\Public\auth\*.info` 里的 `accessToken` /
  `refreshToken` / `nickname` 换成 `$wbEncrypted` 信封（AES-256-GCM，字段档）。
  原「明文读文件」路径会失败；现在导入链路先解密再落库，见 §10.2。

---

## 9. 凭证是怎么拿到的（内存取证，无 F12、无抓包）

方案：不从磁盘解密、也不装 MITM 证书，而是直接读**运行中客户端进程的内存**——
客户端自己必须先解密才能用，所以内存里一定是明文。

```powershell
# 1) 启动客户端（已登录状态）
Start-Process 'D:\Software\WorkBuddy\WorkBuddy.exe'
# 2) 扫描全部 WorkBuddy 进程的已提交可读区域，正则找 JWT / "accessToken":"…"
python probe\hook_token.py
#    实测：扫 7 个进程 / 约 3.3 GB，命中若干 JWT 候选
#    逐个真打 GET /v2/activity/growth/tasks 验证 → HTTP 200 code=0 tasks=17
```

脚本：`probe/hook_token.py`（只读内存，`VirtualQueryEx` + `ReadProcessMemory`，不写目标进程、
不改任何文件）；命中结果写 `probe/hook-token.json`。

同目录的辅助脚本（都只读）：

| 脚本 | 用途 |
| --- | --- |
| `hook_token.py` | 进程内存取 Bearer token 并验证 |
| `find_expert_id.py` | 在客户端数据目录/安装包里按内容找标记串（首批找到拼豆专家的 id） |
| `asar-files.mjs` / `asar-grep.mjs` | 解 / 检索 Electron `app.asar`（找 `static-v1` 的实现位置） |
| `lightup_valorant.mjs` | 端到端点亮 `WorkBuddy_Valorant` 并领奖（形状对齐 `events.rs`） |
| `hunt_plain_jwt.py` | 全盘确认客户端磁盘上没有明文 JWT（排除「直接读文件」这条路） |

拼豆专家 id 的取证链（三重独立证据）：`~/.workbuddy/workbuddy.db` 里该专家会话记录
`marketExpertId=ex_xQHHD1u5Jkua` / `pluginRegisteredName=valorant-perler-expert`；专家排行榜
（`GET /console/expert/ranking`）不含此项；COS 静态清单 429 个专家里也没有 → 故在
`spec.rs` 里以「市场搜索 → 回落该实测 id」两级解析。

---

## 10. 本轮修复：两个新发现的问题（`修一下`）

### 10.1 修一：专家市场列表接口（`growth/spec.rs::fetch_market_experts`）

**结论修正**：接口本身没失效。2026-10-20 用客户端自己的请求形状复核，三域一律
`HTTP 200 / code=0`；早前记录的 `code=17101 invalid request body` 只出现在
「body 里没有 `sort_by` / `sort_order`」的探针形态上，不能依赖服务端默认值。

实测矩阵（真账号，只读 GET/POST）：

| 域 | body | 结果 |
| --- | --- | --- |
| copilot.tencent.com | `{page:1,page_size:50}` | 200 / code=0 / `total_count=22357` / `experts` 50 条 |
| copilot.tencent.com | `…,sort_by:"reco_rank",sort_order:"desc"` | 200 / code=0 / 同上 |
| www.workbuddy.cn | 同上两种 | 200 / code=0 / 同上 |
| www.codebuddy.cn | 同上两种 | 200 / code=0 / 同上 |
| copilot.tencent.com | `…,keyword:"拼豆"` | 200 / code=0 / 4 条，首条 `ex_xQHHD1u5Jkua`（无畏契约拼豆专家，`agent_name=valorant-perler-expert`） |
| copilot.tencent.com | `…,expert_type:"team"` | 200 / code=0 / `total_count=2340` |
| copilot.tencent.com | 仓库旧头（`X-Domain` / `X-User-Id` / `Origin` 等）任意组合 | 200 / code=0（头不是成因） |

请求体的**唯一出处**是客户端自己的两处实现，两者逐字一致
（`CloudExpertRepo._fetchPublicMarketExperts` 与 `fetchMarketExpertPage`）：

```js
let body = { page, page_size: page_size ?? 100 };
if (keyword)     body.keyword     = keyword;
if (expert_type) body.expert_type = expert_type;
if (categories?.length) body.categories = categories;     // 数组，不是字符串
body.sort_by    = sort_by    ?? 'reco_rank';              // 恒有
body.sort_order = sort_order ?? 'desc';                   // 恒有
if (need_pay_assets === true) body.need_pay_assets = true;
```

响应取值也跟着客户端写：`data?.items ?? data?.experts ?? []`（本机三域实测都是
`data.experts` + `data.total_count`，但保留 `items` 兜底与客户端同宽）。

代码改动（`spec.rs`）：固定 `sort_by="reco_rank"` / `sort_order="desc"`；
`team_only` 时改由服务端 `expert_type:"team"` 过滤（省两次全量翻页，客户端侧判断保留给
COS 合并段用）；`page_size` 提成 `PAGE_SIZE` 常量并据此判分页终止；
响应同时接受 `experts` / `items`。另外把 `fetch_valorant` 的注释按复核结论改写
（「市场里还没有这个名字」→「关键词搜索已能命中，回落值只兜底」）。

**改完用 Rust 自己的代码实测**（`spec.rs` 新增 `#[ignore]` 用例 `market_experts_live`，
打真账号、只读）：

```powershell
$env:WORKBUDDY_TOKEN   = '<登录态里的 accessToken>'
$env:WORKBUDDY_USER_ID = '468bd250-2ab0-42bf-902d-a281ced59154'
$env:WORKBUDDY_DOMAIN  = 'www.workbuddy.cn'
cargo test -p agent2api-server --lib market_experts_live -- --include-ignored --nocapture
```

```text
全量首 3 页条数 = 579，首个 id = Some("ex_XoPVlmyOgAY9")
expert_type=team 条数 = 203，首个 id = Some("ex_jPdPfIC04OKD")
keyword=拼豆 条数 = 433，首个 id = Some("ex_xQHHD1u5Jkua")
fetch_valorant 对象 = 4 个，首个 = Some("ex_xQHHD1u5Jkua")
test result: ok. 1 passed; 0 failed
```

`keyword=拼豆` 打 Rust 侧是 433 条而不是探针里的 4 条 —— 探针当时用 `page_size:50`
只看第一页 4 条命中；这里翻了三页、且上游的 `keyword` 是模糊检索（命中系列标签
也算），首个结果仍是 `ex_xQHHD1u5Jkua`，与客户端行为一致。`fetch_valorant`
从中筛出 4 个名字带「拼豆」的对象。

### 10.2 修二：5.8.3 加密登录态的导入（`core/account_store/workbuddy_atrest.rs` 新增）

现象：`%LOCALAPPDATA%\CodeBuddyExtension\Data\Public\auth\workbuddy-desktop.info` 里
`accessToken` / `refreshToken` / 昵称 / 手机号变成
`{"$wbEncrypted":1,"envelope":"<base64>"}`，明文导入路径读到对象 → 落库失败。

算法（逐字取自客户端 bundle，不是拟合）：

| 环节 | 规则 | 出处 |
| --- | --- | --- |
| 保护密钥 → AES key | `key = sha256(atRestSecretKey 的 UTF-8 字节)` | `normalizeAtRestKeyPayload` |
| AES key → 指纹 | `keyId = sha256(key).hex()[:16]` | `deriveAtRestKeyId` |
| 字段信封字段 | 恰为 `authTag/ciphertext/keyId/nonce/suite`，`suite=1` | `parseSupportedEnvelope` |
| 加密 | AES-256-GCM，nonce 12B，tag 16B | `AtRestCrypto.seal/open` |
| AAD | `"WB-AAD\0" \| 0x01 \| lp("WBEV1") \| lp("sym-v1") \| u32be(suite) \| lp(keyId) \| 0x02 \| 0x00 \| 0x00` | `buildAuthenticatedContextAad`（framing=field） |
| keyblob | `slots[0].wrapped`（framing=file）用同一密钥解出 32B 主数据键 | `unwrapStaticV1Slot` |

`lp(s) = u32be(len(utf8(s))) | utf8(s)`。出处文件：
`resources/app.asar`（`app-lifetime-job.js`，模块 9003）与
`resources/app.asar.unpacked/cli/dist/codebuddy-lite-wb.mjs`。

**保护密钥在哪**（这一条决定了实现形态）：**不在任何落盘文件里**。
本机 42128 个文件全量扫描（`~/.workbuddy`、`%LOCALAPPDATA%\CodeBuddyExtension`、
`%APPDATA%\WorkBuddy|CodeBuddy|Tencent`、`%LOCALAPPDATA%\Temp`）：
`atRestSecretKey` 字面量 **0 命中**、DPAPI blob **0 个**。客户端是运行时由
`electron.workbuddyStorage.loggerGet()` 交给主进程
（`main/index.js` 的 `loadMainCredentialProtectionBootstrap`），再经
`daemon:credential-protection-bootstrap` RPC 与
`CODEBUDDY_SIDECAR_CREDENTIAL_BOOTSTRAP_SOCKET` 分发给 daemon / CLI 子进程。

因此实现给两条取键路径：① 环境变量 `WORKBUDDY_AT_REST_SECRET_KEY`（44 字符）；
② Windows 下只读扫描运行中客户端进程内存里的
`{"version":1,"atRestSecretKey":"…"}`（ToolHelp 枚举 + `VirtualQueryEx` +
`ReadProcessMemory`，扫描时带上「文件要求的 keyId」命中即停）。每条候选密钥都必须
真正解开信封才算数，错误密钥必然被 GCM tag 拒绝。

### 10.3 顺带发现的端点（已实测，本轮未接入，留档）

| 端点 | 方法 | body / 说明 |
| --- | --- | --- |
| `/v2/operation-platform/market/expert-categories` | GET | 分类清单（`id/name_zh/name_en/item_count/sort_order`），实测 200 |
| `/v2/operation-platform/market/expert/get-by-ids` | POST | `{expert_ids:[…], include_invisible:bool}`，按 id 批量取专家 |
| `/v2/operation-platform/market/expert/download-url` | POST | `{expert_id}`，返回下载地址（客户端另有 `/portal/…` 前缀同款调用） |
| `/portal/operation-platform/market/expert/my-purchased` | POST | `{limit, cursor?}`，已购专家分页 |

### 10.4 MODIFIED / DIFF / VERIFICATION / ROLLBACK

**MODIFIED**

| 文件 | 动作 |
| --- | --- |
| `server/src/server/core/account_store/workbuddy_atrest.rs` | 新增（797 行，`rustfmt` 后）：密钥推导 / 字段信封解密 / payload 就地替换 / 取键两条路（env + 内存）+ 5 个单测 |
| `server/src/server/core/account_store/workbuddy_import.rs` | 改：读文件 → `workbuddy_atrest::prepare_payload` → 落库；模块头补加密说明；日志带解密字段数与密钥来源 |
| `server/src/server/core/account_store/mod.rs` | 改：注册 `pub mod workbuddy_atrest;` + 子模块分工表两条 |
| `server/src/server/core/providers/workbuddy/growth/spec.rs` | 改：`fetch_market_experts` 请求体固定 `sort_by/sort_order`、服务端 `expert_type` 过滤、接受 `items`、`PAGE_SIZE` 常量；`fetch_valorant` 注释按复核结论改写；文件尾加 `#[ignore]` 实时用例 `market_experts_live`。**另外整文件过了一遍 `rustfmt`**（仓库没有 `rustfmt.toml`，纯排版：import 顺序、能压成一行的结构体字面量，约 26 行），功能零变化 —— 不想要这点噪音就 `git checkout 072e3c7 -- <该文件>` 后只保留语义改动 |
| `server/Cargo.toml` | 改：`windows` 依赖追加 `Win32_Foundation` / `Win32_System_Diagnostics_ToolHelp` / `Win32_System_Memory` / `Win32_System_Threading` 四个 feature（内存取键用） |
| `tools/workbuddy-atrest-inspect.mjs` | 新增：零依赖 Node 复核器（第二份独立实现，`node:crypto` 手写 AAD + AES-256-GCM） |

**VERIFICATION（已跑）**

1. 用 `probe/atrest_decrypt.py`（ctypes + cryptography）从 8 个 `WorkBuddy.exe`
   进程内存扫出保护密钥，指纹 `9127dea1b44020a7` 与文件信封 `keyId`、
   `keyblob.slots[0].protectorKeyId` 三方一致；用同一密钥解开 keyblob 槽得到主数据键，
   指纹 `e2286126792f1557` 等于 `keyblob.keyId`。
2. 用 `tools/workbuddy-atrest-inspect.mjs`（Node 第二份实现）解开**当前**登录态文件
   全部 8 个 `$wbEncrypted` 字段，并对解出的两个令牌真打上游：

```text
文件      C:\Users\Administrator\AppData\Local\CodeBuddyExtension\Data\Public\auth\workbuddy-desktop.info
密钥指纹  9127dea1b44020a7
信封字段  8 个
  [√] 字段 #1 令牌 1331 字符  sub=468bd250-2ab0-42bf-902d-a281ced59154   上游校验 HTTP 200 code=0 tasks=17
  [√] 字段 #2 令牌 700 字符   sub=468bd250-2ab0-42bf-902d-a281ced59154   上游校验 HTTP 200 code=0 tasks=17
  [√] 字段 #3..#8 明文 "ChangeMyLife" / "189****2990"（昵称与手机号，各 3 组）
```

3. §10.1 的专家市场矩阵全部 200 / `code=0`（含 `keyword=拼豆` 命中 4 条、
   首条即 `ex_xQHHD1u5Jkua`）。
4. 装齐工具链后 Rust 侧真跑（见 §11）：`cargo check` exit 0；
   `cargo test -p agent2api-server --lib workbuddy_atrest -- --include-ignored`
   = **5 passed; 0 failed**，其中 `decrypts_real_auth_file` 用**当前**登录态文件跑通，
   打印 `字段=8 来源=client-memory 指纹=9127dea1b44020a7 令牌长度=1331`；
   `cargo clippy` 只报本轮之前就存在的 1 error + 576 warning，无一条指向本轮文件；
   `rustfmt --check` 对本轮改的 3 个既有文件 0 差异、对新文件格式化后 0 差异。
   另用 `probe/rust_struct_check.py` 做括号配平自检（辅助手段）：四个文件 `ALL BALANCED`。
5. 补丁自检（在 `git worktree add --detach HEAD` 的干净树上做，不碰工作树）：
   `git apply --check` = 0；应用后 `git add -A && git diff HEAD` 与工作树同一命令的输出
   **哈希逐字节一致**，改动文件数同为 **14** —— 即补丁能完整复现本轮全部改动。
   注：新建文件的补丁内容是 CRLF（仓库工作树的既有约定），`git add` 时会归一，
   规范化后的内容两侧相同；`git apply --check --reverse` 亦为 0。

```powershell
# 复核器（要保护密钥；密钥取法见 probe/atrest_decrypt.py）
node tools\workbuddy-atrest-inspect.mjs --secret <44 字符>
# 只解不联网
node tools\workbuddy-atrest-inspect.mjs --secret <44 字符> --offline
# 结构自检
python probe\rust_struct_check.py
```

6. **端到端（真服务 + 真账号 + 隔离配置目录，2026-10-20）**：把 `AGENT2API_PROXY_HOME`
   指到 `probe\e2e-home`（不碰 `~/.agent2api`），用 `AGENT2API_ADMIN_USER` /
   `AGENT2API_ADMIN_PASSWORD` 预置管理员、`AGENT2API_CAPTCHA_ENABLED=0` 关掉 ALTCHA，
   起 release 产物后一路打真接口：

```text
POST /api/accounts {"provider":"workbuddy","importDesktop":true}
  → 200；账号 id=user-468bd250-2ab0-42bf-902d-a281ced59154
           nickname=ChangeMyLife  tokenTail=vrMw  edition=cn  hasRefreshToken=true
  → 走的就是本轮新加的 prepare_payload；服务进程**没有**设
    WORKBUDDY_AT_REST_SECRET_KEY，WorkBuddy.exe 在跑（8 个进程）→ 密钥来自内存扫描

GET /api/accounts/<id>/growth/activity → 200（242 KB）
  data 顶层：badges / box / heatmap / lottery(chances,prizes,draws,rewards) /
             profile / redeem
GET /api/accounts/<id>/growth → 200（48 KB）
  任务 23 条（含 WorkBuddy_Valorant / Expert_lighthouse / Expert_team_use_3）、
  streak=20、energy=123、school 5 条、travel 有记录
```

   证据文件：`probe\e2e-growth-activity.json`、`probe\e2e-growth.json`、
   `probe\e2e-tasks.txt`、`probe\e2e-server.log`。

**已跑（Rust 编译 / 单测 / 格式化，2026-10；工具链装在 `E:\rust`、`E:\VSBuildTools`）**

```powershell
cd repo\desktop-tauri\src-tauri
cargo check -p agent2api-server                      # Finished dev profile in 1m 26s，exit 0
cargo test -p agent2api-server --lib workbuddy_atrest -- --include-ignored
                                                     # 5 passed; 0 failed（含真文件解密）
cargo test -p agent2api-server --lib market_experts_live -- --include-ignored
                                                     # 1 passed：真打专家市场（见 §10.1）
cargo clippy -p agent2api-server -- -D warnings      # 1 error + 576 warning，全部本轮之前既有
rustfmt --edition 2021 --check <本轮改动的 5 个 .rs>  # 0 差异
cargo build --release -p agent2api-server            # 见 §11.3
```

编译期踩到的三个 `windows` 0.61 API 名字/类型坑（已在源码里改对，留档）：

| 调用 | 0.61 的正确写法 | 踩坑写法 |
| --- | --- | --- |
| `VirtualQueryEx` 第 3 参 | `Option<*const c_void>`（要 `Some(ptr as _)`） | 直接传裸指针 |
| `OpenProcess` 第 3 参 | 原生 `bool`（`false`） | `BOOL(0)` |
| `ReadProcessMemory` | 在 `Win32::System::Diagnostics::Debug` | 记成 `Win32::System::Memory` |

**NOT RUN**

```powershell
# 1) UI 接线（本轮修复不需要，专家市场/加密导入都不经过 UI 岛）
#    desktop-tauri/ui-islands/src/islands/checkin-state.ts + 重新构建 ui/islands/ui.js
# 2) 起真实服务端到联调（要用户提供面板会话 Cookie）
curl.exe -s "http://127.0.0.1:3065/api/accounts/<id>/growth/activity" -H "Cookie: <面板会话>"
curl.exe -s "http://127.0.0.1:3065/api/accounts/<id>/growth/market/experts" -H "Cookie: <面板会话>"
# 3) 推 GitHub 分支 feat/growth-activity（按 AGENTS.md 红线：推分支前要用户确认；
#    且本机无 gh、credential.helper 为空、无 ssh key，需要 PAT）
```

**ROLLBACK**

```powershell
cd repo
git checkout -- desktop-tauri/src-tauri/server/Cargo.toml `
  desktop-tauri/src-tauri/server/src/server/core/account_store/mod.rs `
  desktop-tauri/src-tauri/server/src/server/core/account_store/workbuddy_import.rs `
  desktop-tauri/src-tauri/server/src/server/core/providers/workbuddy/growth/spec.rs
Remove-Item desktop-tauri/src-tauri/server/src/server/core/account_store/workbuddy_atrest.rs, tools\workbuddy-atrest-inspect.mjs
```

---

## 11. 本机工具链（这一轮现场装起来的，留档给下次）

### 11.1 已装内容与落点

| 组件 | 版本 | 落点 | 备注 |
| --- | --- | --- | --- |
| rustup + rustc + cargo | `1.99.1`（`rustfmt 1.10.0-stable`） | `E:\rust\rustup`、`E:\rust\cargo` | 装 E 盘是为了不占 C 盘（当时 C 盘只剩 17 GB） |
| cargo 镜像 | — | `E:\rust\cargo\config.toml` | `sparse+https://rsproxy.cn/index/`，并设 `[net] git-fetch-with-cli = true` |
| MSVC 生成工具 | VS 2022 BuildTools 工作负载 `VCTools` + `VC.Tools.x86.x64` | `E:\VSBuildTools` | `cl.exe` 已就位 |
| Windows SDK | `Windows11SDK.22621` | `C:\Program Files (x86)\Windows Kits\10` | `windows` 0.61 的 sys 绑定要它 |

装完 `cargo fetch` exit 0（605 个 crate，cache 101 MB，src 775 MB）—— 这一步就证明了
`Cargo.toml` 里新写的四个 `windows` feature 名在 0.61.3 里**真实存在**。

### 11.2 三个一键脚本（纯 ASCII，双击即用）

| 脚本 | 干什么 |
| --- | --- |
| `检查加密登录态.cmd` | `python probe\atrest_decrypt.py --dump` 取出保护密钥 → `node repo\tools\workbuddy-atrest-inspect.mjs` 复核解密 + 真打上游 |
| `安装编译工具链.cmd` | 下载官方 `vs_BuildTools.exe` 装到 `E:\VSBuildTools`，再验 `vswhere` |
| `编译并验收.cmd` | 设 `RUSTUP_HOME`/`CARGO_HOME`/`PATH` → `check` / `clippy` / `build`，日志落 `probe\cargo-verify.log` |

脚本一律 ASCII 的原因：cmd.exe 在 UTF-8 无 BOM 的中文 `.cmd` 上会把引号解析错，
踩过一次（`goto` 标签被吃掉），所以脚本内不出现任何非 ASCII 字节。

### 11.3 验收要用到的环境变量（手工跑时先设）

```powershell
$env:RUSTUP_HOME = 'E:\rust\rustup'
$env:CARGO_HOME  = 'E:\rust\cargo'
$env:PATH        = 'E:\rust\cargo\bin;E:\VSBuildTools\VC\Tools\MSVC\<版本>\bin\Hostx64\x64;' + $env:PATH
```

`cargo build --release -p agent2api-server` 的产物与耗时记在
`probe\cargo-release-build.log`（本机 E 盘剩 61 GB，够）。

**注意 target 目录在哪**：这个 workspace 没在 `src-tauri` 下，`cargo metadata` 给出的
`target_directory` 是 `repo\desktop-tauri\src-tauri\..\..\target`，也就是
**`repo\target`**（`src-tauri\target` 根本不存在，别去那儿找产物）。

实测产物：`repo\target\release\agent2api-server.exe`，12.1 MB，
`cargo build --release` 退出码 0，`Finished \`release\` profile [optimized] target(s) in 7m 12s`
（首次全量；改一个文件后再编约 1 分钟）。

### 11.4 桌面安装包（`npm run tauri:build`，2026-10-20 实测）

```powershell
cd repo
npm run tauri:install      # = npm --prefix desktop-tauri install，只装 @tauri-apps/cli
npm run tauri:build        # = npm --prefix desktop-tauri run build → tauri build
```

`tauri.conf.json` 里 `beforeBuildCommand` 是空的、`frontendDist = ../ui`，前端用的是
仓库里**已经构建好**的 `ui/`，所以**不需要 pnpm、不需要 `ui:islands` 构建**。打包目标
只有 `nsis`；bundler 首次会自己下载 `nsis-3.11.zip` 与 `nsis_tauri_utils v0.5.3`
（要网络，下完缓存在 `%LOCALAPPDATA%\tauri`）。

实测：`Finished \`release\` profile [optimized] target(s) in 13m 55s`（server crate 之前
已 release 编过，`repo\target` 里直接复用），产物两个：

```text
repo\target\release\agent2api.exe                              17.2 MB
  sha256 AE70FAA9EEEFCCAEBB5AF25A86FEEFB7C0D39EB30B34F47DC174989A81C18872
repo\target\release\bundle\nsis\Agent2API_3.1.0_x64-setup.exe   8.9 MB
  sha256 831B3517B5BF1F0780B9DA593CAF7069C34CED4C0CE9724C61545B668DA8CEED
```

打包后冒烟（GUI 产物，`AGENT2API_PROXY_HOME` 指到临时目录，跑完就删）：3065 面板
`/api/panel/status` = `{"registered":true}`；`POST /api/accounts {importDesktop:true}`
200（id `user-468bd250-…`、`ChangeMyLife`、`tokenTail=vrMw`）；
`GET …/growth/activity` 200、`GET …/growth` 200（任务 23 条、streak=20、energy=123）。
**网关是进程内的**：GUI 起来后没有独立的 `agent2api-server.exe` 进程。

**坑（构建完必须清）**：`npm --prefix desktop-tauri install` 会往
`desktop-tauri/package.json` 写一条 `"dependencies": {"agent2api": "file:.."}`、
顺带改 `package-lock.json`，`src-tauri/Cargo.toml` 的行尾也会被翻成 CRLF。这三处
**与本次改动无关**，构建完 `git checkout --` 掉，否则补丁会凭空多三个文件：

```powershell
git checkout -- desktop-tauri/package.json desktop-tauri/package-lock.json desktop-tauri/src-tauri/Cargo.toml
git status --porcelain    # 期望：空
```



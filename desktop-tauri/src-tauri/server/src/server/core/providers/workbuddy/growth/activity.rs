//! WorkBuddy 成长计划「新活动」客户端（growth 域第二批端点）。
//!
//! ── 为什么单独一个文件 ──────────────────────────────────────
//! `mod.rs` 那份是**任务树**（列表 / accept / 点亮 / claim / 领养），本文件是
//! 活动里的**周边机制**：订阅任务解锁口、徽章墙、成长档案、盲盒（开/换/配额/
//! 图鉴）、活跃地图分享、抽奖扩展（奖池/次数/记录/收货地址）、兑换汇总。
//! 两者共用 `super::{chat_call, GrowthCall, GrowthAccount}`，不碰事件构造器。
//!
//! ── 端点清单的来源（不是猜的）───────────────────────────────
//! 逐条抄自**线上前端产物**的 API 层：
//! ```text
//! https://assets.workbuddy.cn/web/usercenter/ad5683203da67c1912ca00c315e90aa584433068/assets/growthSpace-Bhoa8Yzh.js
//! ```
//! 该 chunk 里 `e.get/e.post` 的 23 条路径即成长中心页面的全部数据源；本文件
//! 覆盖其中还没进本仓的那些（已进本仓的：tasks/accept/claim、energy、streak、
//! heatmap、lottery/summary、lottery/draw、makeup-cards/use、redeem、
//! buddy/info、buddy/agreement、buddy/first、buddy/travel/*）。
//!
//! ── 关于响应字段：原样透传 ──────────────────────────────────
//! 这些端点的**响应形状尚未逐条实测**（前端产物只能证明请求路径与请求体，
//! 证明不了响应字段）。所以本文件一律把 `data` 段原样回给调用方，**不发明
//! 字段名**；只有确实需要判读的地方用 [`pick_i64`] / [`pick_str`] 这类
//! 「候选键探测」——候选键写在注释里，命不中就返回 None，绝不编造默认值。
//!
//! ── 写动作的分寸 ────────────────────────────────────────────
//! 只读方法随便调；写方法（开盲盒 / 切换展示 / 分享 / 改可见性）都有调用方
//! 显式传参，且开盲盒带次数护栏（[`MAX_BOX_OPEN`]）—— 一个账号的能量是活动
//! 内唯一硬通货，脚本没有权力替用户花掉它。
//!
//! ── 硬约束 ──────────────────────────────────────────────────
//! release 是 `panic=abort`：本文件零 unwrap/expect/panic。

use serde_json::{json, Value};

use super::{chat_call, GrowthAccount, GrowthCall};

// ─── 端点（路径与线上前端逐字一致）────────────────────────────

/// 订阅任务状态（GET）——**拉任务列表前必须先走一次**：订阅类任务靠它解锁。
pub const SUBSCRIBE_STATUS_PATH: &str = "/v2/activity/growth/subscribe-task/status";
/// 成长档案（GET）。
pub const PROFILE_PATH: &str = "/v2/activity/growth/profile";
/// 徽章墙（GET）。
pub const BADGES_PATH: &str = "/v2/activity/growth/badges";
/// 盲盒：配额 / 图鉴 / 模板 / 开启 / 切换展示 / 可见性。
pub const BOX_QUOTA_PATH: &str = "/activity/growth/buddy/quota";
pub const BOX_LIST_PATH: &str = "/activity/growth/buddy/list";
pub const BOX_TEMPLATES_PATH: &str = "/activity/growth/buddy/templates";
pub const BOX_OPEN_PATH: &str = "/activity/growth/buddy/open";
pub const BOX_SWITCH_PATH: &str = "/activity/growth/buddy/switch";
pub const BOX_VISIBLE_PATH: &str = "/activity/growth/buddy/visible";
/// 活跃地图分享（POST）。
pub const HEATMAP_SHARE_PATH: &str = "/activity/growth/heatmap/share";
/// 活跃地图读数（GET）—— 连登管家用它判昨日漏签（`streak.rs`），新活动盘点也读它。
pub const HEATMAP_PATH: &str = "/activity/growth/heatmap";
/// 兑换汇总（GET）。
pub const REDEEM_SUMMARY_PATH: &str = "/activity/growth/redeem/summary";
/// 抽奖汇总（GET）—— 与 `streak.rs` 用的同一条路径，此处供只读盘点复用。
pub const LOTTERY_SUMMARY_PATH: &str = "/activity/growth/lottery/summary";
/// 抽奖扩展：奖池 / 次数 / 抽奖记录 / 中奖记录。
pub const LOTTERY_PRIZES_PATH: &str = "/activity/growth/lottery/prizes";
pub const LOTTERY_CHANCES_PATH: &str = "/activity/growth/lottery/chances";
pub const LOTTERY_DRAWS_PATH: &str = "/activity/growth/lottery/draws";
pub const LOTTERY_REWARDS_PATH: &str = "/activity/growth/lottery/rewards";

/// 单次调用的开盲盒次数上限。
///
/// 线上页面一次只开一个（`count` 默认 1），上游是否接受批量未实测；这里既
/// 不假设能批量（默认 1），也不让调用方一次把能量花光（上限 10）。
pub const MAX_BOX_OPEN: i64 = 10;

/// 列表类端点的默认页大小（与线上前端 `page_size: 20` 同口径）。
pub const DEFAULT_PAGE_SIZE: i64 = 20;

// ─── 信封与判读工具 ─────────────────────────────────────────

/// 把一次 growth 域调用收敛成 `Result<Value, String>`：成功取 `data` 段原样。
fn data_of(call: GrowthCall, what: &str) -> Result<Value, String> {
    if !call.ok() {
        return Err(format!(
            "{what}失败（HTTP {} code={} {}）",
            call.status,
            call
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "-".to_string()),
            call.msg()
        ));
    }
    Ok(call.data().clone())
}

async fn get_data(account: &GrowthAccount, path: &str, what: &str) -> Result<Value, String> {
    let call = chat_call(account, "GET", path, None, &[]).await?;
    data_of(call, what)
}

async fn post_data(
    account: &GrowthAccount,
    path: &str,
    body: &Value,
    what: &str,
) -> Result<Value, String> {
    let call = chat_call(account, "POST", path, Some(body), &[]).await?;
    data_of(call, what)
}

/// 候选键探测（i64）：按顺序试 `keys`，命中第一个数值就返回。
///
/// **为什么是「候选键」而不是「字段名」**：这些端点的响应形状还没实测，
/// 写死一个字段名等于替上游编协议（活动换期时会静默变空）。这里只把
/// 前端文案证明「一定存在」的量（能量值、消耗、次数、天数）用多候选兜住，
/// 全不命中就返回 None，由调用方如实显示「未读数」。
pub fn pick_i64(data: &Value, keys: &[&str]) -> Option<i64> {
    for key in keys {
        if let Some(value) = data.get(*key).and_then(Value::as_i64) {
            return Some(value);
        }
        // 允许一层嵌套（如 data.quota.energy / data.summary.chances）
        if let Some(object) = data.as_object() {
            for (_, nested) in object {
                if let Some(value) = nested.get(*key).and_then(Value::as_i64) {
                    return Some(value);
                }
            }
        }
    }
    None
}

/// 候选键探测（字符串），语义同 [`pick_i64`]。
pub fn pick_str<'a>(data: &'a Value, keys: &[&str]) -> Option<&'a str> {
    for key in keys {
        if let Some(value) = data.get(*key).and_then(Value::as_str) {
            return Some(value);
        }
    }
    None
}

/// 候选键探测（数组长度）：`data.<key>` 是数组时返回元素个数。
pub fn pick_len(data: &Value, keys: &[&str]) -> Option<i64> {
    for key in keys {
        if let Some(array) = data.get(*key).and_then(Value::as_array) {
            return Some(array.len() as i64);
        }
    }
    None
}

// ─── 订阅任务 / 徽章 / 成长档案（只读）───────────────────────

/// 刷新订阅任务状态（GET）。
///
/// 线上前端的调用顺序是「先 `subscribe-task/status` 再 `tasks`」，且前者的
/// 失败**不阻塞**后者（前端 `.catch()` 后照样拉列表）。本函数照抄这个分寸：
/// 失败只回错误串，由调用方决定是否继续 —— 但顺序上必须先试一次。
pub async fn subscribe_status(account: &GrowthAccount) -> Result<Value, String> {
    get_data(account, SUBSCRIBE_STATUS_PATH, "订阅任务状态").await
}

/// 成长档案（GET）。
pub async fn profile(account: &GrowthAccount) -> Result<Value, String> {
    get_data(account, PROFILE_PATH, "成长档案").await
}

/// 徽章墙（GET）。
pub async fn badges(account: &GrowthAccount) -> Result<Value, String> {
    get_data(account, BADGES_PATH, "徽章墙").await
}

/// 徽章墙读数（已得 / 总数 / 清单）——字段名未实测，故用候选键，全不命中给 None。
///
/// 前端文案证据：「用户获得的徽章将统一展示在用户…的徽章墙中」「任务徽章」
/// 「暂无徽章」，说明返回里必然有「全部徽章」与「已获得」两组；这里把常见
/// 命名都试一遍，读不出就老实返回 None（界面显示未读数，而不是显示 0）。
pub struct BadgeSummary {
    pub total: Option<i64>,
    pub earned: Option<i64>,
    pub raw: Value,
}

impl BadgeSummary {
    pub fn from_data(data: Value) -> Self {
        let total = pick_len(&data, &["badges", "list", "items", "all"])
            .or_else(|| pick_i64(&data, &["total", "total_count", "badge_total"]));
        let earned = pick_i64(
            &data,
            &["earned", "earned_count", "obtained", "obtained_count", "completed"],
        )
        .or_else(|| pick_len(&data, &["earned_badges", "obtained_badges"]));
        Self { total, earned, raw: data }
    }
}

/// 拉徽章墙并给出读数。
pub async fn badge_summary(account: &GrowthAccount) -> Result<BadgeSummary, String> {
    let data = badges(account).await?;
    Ok(BadgeSummary::from_data(data))
}

// ─── 盲盒（能量开猫）─────────────────────────────────────────

/// 盲盒配额（GET）：能量余额与单次消耗。失败给错误串，由调用方决定是否禁按钮。
pub async fn box_quota(account: &GrowthAccount) -> Result<Value, String> {
    get_data(account, BOX_QUOTA_PATH, "盲盒配额").await
}

/// 猫猫图鉴（GET）。
pub async fn box_list(account: &GrowthAccount) -> Result<Value, String> {
    get_data(account, BOX_LIST_PATH, "猫猫图鉴").await
}

/// 猫猫模板（GET）。
pub async fn box_templates(account: &GrowthAccount) -> Result<Value, String> {
    get_data(account, BOX_TEMPLATES_PATH, "猫猫模板").await
}

/// 盲盒配额读数：能量、单次消耗、可开次数。
///
/// 前端文案证据：「每次开启盲盒需要消耗 N 能量值」「当前仅有 M 能量值」
/// 「能量不足，请参与任务获取更多能量」——前端是**自己算**能开几次的，
/// 所以这里也自己算：可开次数 = 能量 / 单次消耗（任一读不出就给 None）。
pub struct BoxQuota {
    pub energy: Option<i64>,
    pub cost_per_open: Option<i64>,
    pub opens: Option<i64>,
    pub raw: Value,
}

impl BoxQuota {
    pub fn from_data(data: Value) -> Self {
        let energy = pick_i64(&data, &["energy", "energy_balance", "balance", "energy_value"]);
        let cost_per_open = pick_i64(
            &data,
            &["cost", "cost_per_open", "open_cost", "energy_cost", "consume"],
        );
        let opens = match (energy, cost_per_open) {
            (Some(energy), Some(cost)) if cost > 0 => Some((energy / cost).max(0)),
            _ => pick_i64(&data, &["opens", "open_times", "available", "count"]),
        };
        Self { energy, cost_per_open, opens, raw: data }
    }
}

/// 拉盲盒配额并给出读数。
pub async fn box_quota_summary(account: &GrowthAccount) -> Result<BoxQuota, String> {
    let data = box_quota(account).await?;
    Ok(BoxQuota::from_data(data))
}

/// 开盲盒（POST `/activity/growth/buddy/open`，body `{count}`）。
///
/// 写动作：消耗能量换猫。`count` 由调用方显式给定并夹在 `1..=MAX_BOX_OPEN`。
/// 返回上游原始载荷（新猫的实例信息形状未实测，透传）。
pub async fn box_open(account: &GrowthAccount, count: i64) -> Result<Value, String> {
    let count = count.clamp(1, MAX_BOX_OPEN);
    post_data(account, BOX_OPEN_PATH, &json!({ "count": count }), "开启盲盒").await
}

/// 切换展示的猫（POST `/activity/growth/buddy/switch`，body `{instance_id}`）。
pub async fn box_switch(account: &GrowthAccount, instance_id: &str) -> Result<Value, String> {
    post_data(
        account,
        BOX_SWITCH_PATH,
        &json!({ "instance_id": instance_id }),
        "切换展示猫猫",
    )
    .await
}

/// 猫猫可见性（GET）。
pub async fn box_visible(account: &GrowthAccount) -> Result<Value, String> {
    get_data(account, BOX_VISIBLE_PATH, "猫猫可见性").await
}

/// 设置猫猫可见性（POST `/activity/growth/buddy/visible`，body `{visible}`）。
pub async fn box_set_visible(account: &GrowthAccount, visible: bool) -> Result<Value, String> {
    post_data(
        account,
        BOX_VISIBLE_PATH,
        &json!({ "visible": visible }),
        "设置猫猫可见性",
    )
    .await
}

// ─── 活跃地图分享 / 兑换汇总（只读 + 一个分享写动作）─────────

/// 活跃地图分享（POST `/activity/growth/heatmap/share`）。
///
/// 线上前端 `heatmap/share` 的可选 body 是 `t ?? {}`（分享参数按需带），
/// 所以本函数也接受一个可选 body：`None` 时发空对象 —— 与前端形状一致。
pub async fn heatmap_share(account: &GrowthAccount, body: Option<&Value>) -> Result<Value, String> {
    let empty = json!({});
    let payload = body.unwrap_or(&empty);
    post_data(account, HEATMAP_SHARE_PATH, payload, "活跃地图分享").await
}

/// 兑换汇总（GET）：本月已兑换档位与历史。
pub async fn redeem_summary(account: &GrowthAccount) -> Result<Value, String> {
    get_data(account, REDEEM_SUMMARY_PATH, "兑换汇总").await
}

/// 活跃地图（GET）：全年活跃记录（前端「全年活跃记录」「热力墙」区域的唯一数据源）。
pub async fn heatmap(account: &GrowthAccount) -> Result<Value, String> {
    get_data(account, HEATMAP_PATH, "活跃地图").await
}

// ─── 抽奖扩展（只读）────────────────────────────────────────

/// 抽奖奖池（GET）。
pub async fn lottery_prizes(account: &GrowthAccount) -> Result<Value, String> {
    get_data(account, LOTTERY_PRIZES_PATH, "抽奖奖池").await
}

/// 抽奖汇总（GET `/activity/growth/lottery/summary`）：剩余次数与本期概况。
pub async fn lottery_summary(account: &GrowthAccount) -> Result<Value, String> {
    get_data(account, LOTTERY_SUMMARY_PATH, "抽奖汇总").await
}

/// 抽奖次数明细（GET）。
pub async fn lottery_chances(account: &GrowthAccount) -> Result<Value, String> {
    get_data(account, LOTTERY_CHANCES_PATH, "抽奖次数").await
}

/// 抽奖次数流水（GET `/activity/growth/lottery/chances/logs?page&page_size`）。
pub async fn lottery_chances_logs(
    account: &GrowthAccount,
    page: i64,
    page_size: i64,
) -> Result<Value, String> {
    let page = page.max(1);
    let page_size = page_size.clamp(1, 100);
    let path = format!("{LOTTERY_CHANCES_PATH}/logs?page={page}&page_size={page_size}");
    get_data(account, &path, "抽奖次数流水").await
}

/// 抽奖记录（GET `/activity/growth/lottery/draws?page&page_size`）。
pub async fn lottery_draws(
    account: &GrowthAccount,
    page: i64,
    page_size: i64,
) -> Result<Value, String> {
    let page = page.max(1);
    let page_size = page_size.clamp(1, 100);
    let path = format!("{LOTTERY_DRAWS_PATH}?page={page}&page_size={page_size}");
    get_data(account, &path, "抽奖记录").await
}

/// 中奖记录（GET `/activity/growth/lottery/rewards?page&page_size`）。
pub async fn lottery_rewards(
    account: &GrowthAccount,
    page: i64,
    page_size: i64,
) -> Result<Value, String> {
    let page = page.max(1);
    let page_size = page_size.clamp(1, 100);
    let path = format!("{LOTTERY_REWARDS_PATH}?page={page}&page_size={page_size}");
    get_data(account, &path, "中奖记录").await
}

/// 实物奖品收货地址（GET `/activity/growth/lottery/rewards/{id}/address`）。
pub async fn lottery_reward_address(account: &GrowthAccount, reward_id: &str) -> Result<Value, String> {
    let path = format!("{LOTTERY_REWARDS_PATH}/{reward_id}/address");
    get_data(account, &path, "奖品收货地址").await
}

// ─── 未登记任务盘点（新活动的发现口）────────────────────────

/// 从任务列表里挑出**映射表尚未登记**的任务码（= 新活动的新任务）。
///
/// `mod.rs::process_task` 对未登记任务是保守跳过的（不发明事件形状），界面
/// 上也只标一行「非映射任务」。但「上游加了新任务」这件事本身要让用户看见：
/// 本函数把它们的码 / 标题 / 说明 / 状态 / 进度原样聚出来，交给界面显示，
/// 也是登记 `spec.rs` 时的输入（配合 `tools/growth-live-inspect.mjs` 的骨架生成）。
pub fn unknown_tasks(tasks: &[Value]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for task in tasks {
        let code = task.get("task_code").and_then(Value::as_str).unwrap_or("");
        if code.is_empty() || super::spec::spec_of(code).is_some() {
            continue;
        }
        let progress = task.get("progress").cloned().unwrap_or(Value::Null);
        out.push(json!({
            "code": code,
            "title": task.get("title").and_then(Value::as_str).unwrap_or(""),
            "description": task.get("description").and_then(Value::as_str).unwrap_or(""),
            "status": task.get("accept_status").and_then(Value::as_str).unwrap_or(""),
            "current": progress.get("current").and_then(Value::as_i64).unwrap_or(0),
            "target": progress.get("target").and_then(Value::as_i64).unwrap_or(0),
            "locked": task.get("locked").and_then(Value::as_bool).unwrap_or(false),
            "unlockAt": task.get("unlock_at").cloned().unwrap_or(Value::Null),
        }));
    }
    out
}

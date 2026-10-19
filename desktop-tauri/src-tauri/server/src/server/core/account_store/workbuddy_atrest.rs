//! WorkBuddy 5.8.x「字段级落盘加密」（`$wbEncrypted` 信封）的**取键 + 解密**。
//!
//! ── 现象 ────────────────────────────────────────────────────
//! 官方客户端从 5.8.3 起，把
//! `%LOCALAPPDATA%\CodeBuddyExtension\Data\Public\auth\workbuddy-desktop*.info`
//! 里的 `accessToken` / `refreshToken` / 昵称 / 手机号换成了对象信封：
//!
//! ```json
//! {"auth":{"accessToken":{"$wbEncrypted":1,"envelope":"eyJzdWl0ZSI6MSwia2V5…"}}}
//! ```
//!
//! 信封本体是 `base64(JSON)`，套一层 AES-256-GCM。于是老版本的「原样读文件 +
//! 落库」会拿到一个对象而不是字符串，导入直接失败。
//!
//! ── 算法出处（不是猜的）─────────────────────────────────────
//! 逐字来自客户端 bundle 的同一份实现：`resources/app.asar` 里的
//! `app-lifetime-job.js`（模块 9003，负责 keyblob / 信封解析 / 模式判定）与
//! `resources/app.asar.unpacked/cli/dist/codebuddy-lite-wb.mjs`
//! （CLI 侧同一套 crypto 与 `$wbEncrypted` 字段编解码器）。
//!
//! ── 密钥链 ──────────────────────────────────────────────────
//! ```text
//! atRestSecretKey（44 字符 canonical base64，由客户端 native 运行时给出）
//!   └─ key    = sha256(atRestSecretKey 的 UTF-8 字节)     # normalizeAtRestKeyPayload
//!        └─ keyId = sha256(key).hex()[:16]                # deriveAtRestKeyId
//! ```
//! 注意第一跳的输入是**那 44 个字符本身**，不是它 base64 解码出来的 32 字节
//! （源实现写的是 `createHash('sha256').update(payload.atRestSecretKey, 'utf8')`）。
//!
//! 本机实测链条（2026-10-20）：`atRestSecretKey` → `key =
//! c74aaf9df09104a5e0b1fd02d1c2b1ea586e4066844933f3279721c688d44055` →
//! `keyId = 9127dea1b44020a7`。同一个 keyId 出现在三处，互为佐证：
//!   * 登录态文件里每个信封的 `keyId`；
//!   * `~/.workbuddy/keyblob` 的 `slots[0].protectorKeyId`；
//!   * 用该密钥解开 `slots[0].wrapped`（framing=file）得到的 32 字节主数据键，
//!     其 `sha256(...)[:16]` 恰等于 keyblob 自己的 `keyId`（`e2286126792f1557`）。
//!
//! ── AAD（GCM 认证的附加数据）────────────────────────────────
//! 源实现 `buildAuthenticatedContextAad(keyId, suite, context, scheme)`，
//! 字段档传的是 `context = {framing:"field"}`、`scheme = "sym-v1"`，拼出来是：
//!
//! ```text
//! "WB-AAD\0" | 0x01 | len8("WBEV1") | len8("sym-v1") | u32be(suite)
//!            | len8(keyId) | 0x02       | 0x00        | 0x00
//!             ^ 魔数          ^ 帧类型码  ^ 缺省 sequence  ^ 缺省 final
//! len8(s) = u32be(len(s 的 UTF-8 字节)) | s 的 UTF-8 字节
//! ```
//! 帧类型码取自源实现的 `{file:1, field:2, record:3, stream:4}`；
//! 魔数取自 `{file:"WBEF1", field:"WBEV1", record:"WBER1", stream:"WBES1"}`。
//! `suite` 目前恒为 1（AES-256-GCM v1）。
//!
//! ── 为什么保护密钥只能从**运行中的客户端内存**取 ────────────
//! 它不在任何落盘文件里：本机 42128 个文件（`~/.workbuddy`、
//! `%LOCALAPPDATA%\CodeBuddyExtension`、`%APPDATA%\WorkBuddy|CodeBuddy|Tencent`、
//! `%LOCALAPPDATA%\Temp`）全量扫 `atRestSecretKey` 字面量 **0 命中**、
//! DPAPI blob **0 个**；`~/.workbuddy/keyblob` 只放「被保护密钥包起来的主数据键」。
//! 客户端是**运行时**由 `electron.workbuddyStorage.loggerGet()` 交给主进程的
//! `loadMainCredentialProtectionBootstrap`（见 `main/index.js` 的
//! `WorkbuddyDesktopApp.main()`），再经 `daemon:credential-protection-bootstrap`
//! RPC 通道与 `CODEBUDDY_SIDECAR_CREDENTIAL_BOOTSTRAP_SOCKET` 分发给 daemon / CLI
//! 子进程。两条路都不是文件，所以外部只能：读内存，或让用户手填。
//!
//! 本模块因此给两条取键路径，按顺序试：
//!   1. 环境变量 [`SECRET_ENV`]（`WORKBUDDY_AT_REST_SECRET_KEY=<44 字符>`）
//!      —— 用户/CI 显式提供，也是非 Windows 上唯一的路径；
//!   2. 扫运行中的客户端进程内存里的 `{"version":1,"atRestSecretKey":"…"}`
//!      （仅 Windows；只读 `PROCESS_VM_READ`）。
//!
//! 每条候选密钥都**必须真把信封解开**才算数 —— 错误密钥在 GCM 的 tag 校验上
//! 必然失败，所以「取错密钥把账号写坏」这条路径不存在。
//! 另外扫描时会把「目标 keyId」带进去（它就在待解密文件的信封里），
//! 命中即返回：通常第一个进程的头几百 MB 内就命中，不必扫完所有进程。
//!
//! ── 硬约束（release 是 panic=abort）──────────────────────────
//! 零 unwrap / 零 expect / 零越界索引；`unsafe` 只出现在 `client_memory`
//! 的 Win32 调用处，每处都写明它能成立的理由。扫描全程只读，不改客户端
//! 进程一个字节、不写它的任何文件、不联网。

use aes_gcm::aead::consts::U12;
use aes_gcm::aead::Aead as _;
use aes_gcm::aead::Payload;
use aes_gcm::{Aes256Gcm, KeyInit as _, Nonce};
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use base64::engine::DecodePaddingMode;
use base64::Engine as _;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// 用户/CI 直接给保护密钥的环境变量（44 字符 canonical base64）。
pub const SECRET_ENV: &str = "WORKBUDDY_AT_REST_SECRET_KEY";

/// 保护密钥长度（canonical base64 形态，32 字节 → 44 个字符）。
const SECRET_LEN: usize = 44;

/// AES-256 密钥长度。
const AES_KEY_LEN: usize = 32;

/// GCM nonce 长度（96 bit）。
const NONCE_LEN: usize = 12;

/// GCM 认证标签长度（128 bit）。
const TAG_LEN: usize = 16;

/// 目前唯一支持的 suite（AES-256-GCM v1）。
const SUITE_V1: i64 = 1;

/// 对称方案的 scheme 名（进 AAD）。
const SCHEME_SYM_V1: &str = "sym-v1";

/// 字段档的魔数（源实现 `{file:"WBEF1", field:"WBEV1", …}`）。
const FIELD_MAGIC: &str = "WBEV1";

/// 字段档的帧类型码（源实现 `{file:1, field:2, record:3, stream:4}`）。
const FIELD_FRAMING_CODE: u8 = 2;

/// AAD 的域分隔前缀（源实现 `Buffer.from("WB-AAD\0", "ascii")`）。
const AAD_DOMAIN: &[u8] = b"WB-AAD\0";

/// 字段信封的标记字段名。
const WRAPPER_MARKER: &str = "$wbEncrypted";

/// base64 引擎：标准字母表；解码宽容度对齐 `Buffer.from(x, "base64")`
/// （客户端只做形态校验 `isCanonical32ByteBase64`，我们另有一道等值复核）。
const WB_BASE64: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true),
);

// ─── 密钥 ────────────────────────────────────────────────────

/// at-rest 保护密钥（AES-256-GCM 的 key + 它的指纹 keyId）。
pub struct ProtectorKey {
    key: [u8; AES_KEY_LEN],
    key_id: String,
}

impl ProtectorKey {
    /// 由 `atRestSecretKey`（44 字符）推出密钥：`key = sha256(secret 的 UTF-8)`。
    ///
    /// 非 canonical base64 / 长度不对 / 全零哨兵值都拒绝 —— 与源实现
    /// `parseKeyPayload` 的三条校验一一对应（`isCanonical32ByteBase64`、
    /// 长度 44、`isZeroKeySentinel`）。
    pub fn from_secret(secret: &str) -> Result<Self, String> {
        let trimmed = secret.trim();
        if !is_canonical_secret(trimmed) {
            return Err(
                "at-rest 保护密钥不是 32 字节的 canonical base64（应为 44 个字符）".to_string(),
            );
        }
        let decoded = WB_BASE64
            .decode(trimmed)
            .map_err(|_| "at-rest 保护密钥 base64 解码失败".to_string())?;
        if decoded.iter().all(|byte| *byte == 0) {
            return Err("at-rest 保护密钥是全零哨兵值（源实现同样拒绝）".to_string());
        }
        let digest = Sha256::digest(trimmed.as_bytes());
        let mut key = [0u8; AES_KEY_LEN];
        key.copy_from_slice(&digest[..]);
        let key_id = hex_first8(&Sha256::digest(key));
        Ok(Self { key, key_id })
    }

    /// 密钥指纹（`sha256(key)` 前 8 字节的十六进制，16 个字符）。
    pub fn key_id(&self) -> &str {
        &self.key_id
    }
}

/// `canonical base64(32 字节)` 的形态判定：长度 44、解得 32 字节、重编码等值。
fn is_canonical_secret(value: &str) -> bool {
    if value.len() != SECRET_LEN {
        return false;
    }
    match WB_BASE64.decode(value) {
        Ok(bytes) => bytes.len() == AES_KEY_LEN && WB_BASE64.encode(&bytes) == value,
        Err(_) => false,
    }
}

/// `sha256(bytes)` 前 8 字节的小写十六进制（源实现 `digest('hex').slice(0,16)`）。
fn hex_first8(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(16);
    for byte in bytes.iter().take(8) {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

// ─── 信封解析与解密 ──────────────────────────────────────────

/// 信封对象（base64(JSON) 解开后的形状）。字段恰好五个，顺序无关。
struct Envelope {
    key_id: String,
    nonce: Vec<u8>,
    auth_tag: Vec<u8>,
    ciphertext: Vec<u8>,
}

/// 解开一条 `$wbEncrypted` 信封，返回明文（UTF-8 文本）。
///
/// 调用链：base64 → JSON → 形状校验 → keyId 必须与密钥指纹一致（否则直接
/// 判定「拿错密钥」，不浪费一次 GCM 运算）→ AAD → AES-256-GCM 解密。
fn open_field_envelope(key: &ProtectorKey, envelope_base64: &str) -> Result<String, String> {
    let raw = WB_BASE64
        .decode(envelope_base64.trim())
        .map_err(|_| "字段信封不是合法 base64".to_string())?;
    let text = String::from_utf8(raw).map_err(|_| "字段信封不是 UTF-8 文本".to_string())?;
    let value: Value =
        serde_json::from_str(&text).map_err(|_| "字段信封不是合法 JSON".to_string())?;
    let envelope = parse_envelope(&value)?;
    if envelope.key_id != key.key_id() {
        return Err(format!(
            "信封的 keyId（{}）与保护密钥指纹（{}）不一致",
            envelope.key_id,
            key.key_id()
        ));
    }
    let aad = build_field_aad(&envelope.key_id, SUITE_V1);
    let mut message = Vec::with_capacity(envelope.ciphertext.len() + envelope.auth_tag.len());
    message.extend_from_slice(&envelope.ciphertext);
    message.extend_from_slice(&envelope.auth_tag);
    let cipher = Aes256Gcm::new_from_slice(&key.key)
        .map_err(|_| "保护密钥长度不受 AES-256 支持".to_string())?;
    let nonce = Nonce::<U12>::try_from(envelope.nonce.as_slice())
        .map_err(|_| format!("信封 nonce 长度异常（期望 {NONCE_LEN} 字节）"))?;
    let plaintext = cipher
        .decrypt(
            &nonce,
            Payload {
                msg: &message,
                aad: &aad,
            },
        )
        .map_err(|_| "AES-GCM 解密失败（保护密钥或信封不匹配）".to_string())?;
    String::from_utf8(plaintext).map_err(|_| "解密结果不是合法 UTF-8 文本".to_string())
}

/// 校验信封对象的五字段形状与长度（对照源实现 `parseSupportedEnvelope`）。
fn parse_envelope(value: &Value) -> Result<Envelope, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "字段信封必须是 JSON 对象".to_string())?;
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    if keys != ["authTag", "ciphertext", "keyId", "nonce", "suite"] {
        return Err(
            "字段信封的字段集合不符（应为 authTag/ciphertext/keyId/nonce/suite）".to_string(),
        );
    }
    let suite = object
        .get("suite")
        .and_then(Value::as_i64)
        .ok_or_else(|| "字段信封缺少 suite".to_string())?;
    if suite != SUITE_V1 {
        return Err(format!("不支持的字段信封 suite：{suite}"));
    }
    let key_id = object
        .get("keyId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if key_id.len() != 16 || !key_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("字段信封的 keyId 不是 16 位十六进制".to_string());
    }
    let decode = |name: &str| -> Result<Vec<u8>, String> {
        let text = object.get(name).and_then(Value::as_str).unwrap_or("");
        WB_BASE64
            .decode(text)
            .map_err(|_| format!("字段信封的 {name} 不是合法 base64"))
    };
    let nonce = decode("nonce")?;
    if nonce.len() != NONCE_LEN {
        return Err(format!(
            "字段信封的 nonce 长度异常（期望 {NONCE_LEN} 字节）"
        ));
    }
    let auth_tag = decode("authTag")?;
    if auth_tag.len() != TAG_LEN {
        return Err(format!(
            "字段信封的 authTag 长度异常（期望 {TAG_LEN} 字节）"
        ));
    }
    // 源实现把空字符串密文当空字节流处理，这里照抄
    let ciphertext = match object.get("ciphertext").and_then(Value::as_str) {
        Some("") | None => Vec::new(),
        Some(_) => decode("ciphertext")?,
    };
    Ok(Envelope {
        key_id,
        nonce,
        auth_tag,
        ciphertext,
    })
}

/// 字段档 AAD（源实现 `buildAuthenticatedContextAad(keyId, suite, {framing:"field"},
/// "sym-v1")`）。字节序列见模块头。
fn build_field_aad(key_id: &str, suite: i64) -> Vec<u8> {
    let mut aad = Vec::with_capacity(64);
    aad.extend_from_slice(AAD_DOMAIN);
    aad.push(1);
    push_length_prefixed(&mut aad, FIELD_MAGIC);
    push_length_prefixed(&mut aad, SCHEME_SYM_V1);
    aad.extend_from_slice(&(suite as u32).to_be_bytes());
    push_length_prefixed(&mut aad, key_id);
    aad.push(FIELD_FRAMING_CODE);
    aad.push(0);
    aad.push(0);
    aad
}

/// `encodeLengthPrefixed`：`u32be(len) | utf8`（长度上限 2^32-1，实践中触不到）。
fn push_length_prefixed(out: &mut Vec<u8>, text: &str) {
    let bytes = text.as_bytes();
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
}

// ─── payload 遍历与就地替换 ──────────────────────────────────

/// 这个值是不是一条 `$wbEncrypted` 信封（源实现 `hasEncryptedFieldMarker`）。
fn is_wrapper(value: &Value) -> bool {
    value
        .as_object()
        .and_then(|object| object.get(WRAPPER_MARKER))
        .and_then(Value::as_i64)
        == Some(1)
}

/// payload 里是否出现任何信封（决定走不走解密链）。
pub fn has_encrypted_field(value: &Value) -> bool {
    match value {
        Value::Object(object) => is_wrapper(value) || object.values().any(has_encrypted_field),
        Value::Array(items) => items.iter().any(has_encrypted_field),
        _ => false,
    }
}

/// 取 payload 里第一条信封的 keyId（用于「只认这一把钥匙」的快速判据）。
fn envelope_key_id(value: &Value) -> Option<String> {
    match value {
        Value::Object(object) => {
            if is_wrapper(value) {
                let encoded = object.get("envelope").and_then(Value::as_str)?;
                let raw = WB_BASE64.decode(encoded.trim()).ok()?;
                let text = String::from_utf8(raw).ok()?;
                let parsed: Value = serde_json::from_str(&text).ok()?;
                return Some(parse_envelope(&parsed).ok()?.key_id);
            }
            object.values().find_map(envelope_key_id)
        }
        Value::Array(items) => items.iter().find_map(envelope_key_id),
        _ => None,
    }
}

/// 就地解开 payload 里所有对称信封，返回解开的字段数。
///
/// 解开的值按 UTF-8 文本**替换掉整个信封对象** —— 于是 payload 的形状
/// （`{auth:{…},account:{…}}`）与老明文版本逐字一致，落库链路不用改。
/// `asym-v1`（非对称 / 高级档）不在支持范围：它要开发者私钥，本机落盘也不用它。
fn decrypt_walk(value: &mut Value, key: &ProtectorKey, count: &mut usize) -> Result<(), String> {
    // 先判信封（不可变借用），再拿可变借用 —— 两步分开，借用检查器才放行
    if is_wrapper(value) {
        let Some(object) = value.as_object_mut() else {
            return Ok(());
        };
        if object.contains_key("scheme") {
            return Err("字段信封是 asym-v1（非对称档），本模块只解对称档".to_string());
        }
        let envelope = object
            .get("envelope")
            .and_then(Value::as_str)
            .ok_or_else(|| "字段信封缺少 envelope".to_string())?
            .to_string();
        let plaintext = open_field_envelope(key, &envelope)?;
        *count += 1;
        *value = Value::String(plaintext);
        return Ok(());
    }
    match value {
        Value::Object(object) => {
            // 先收集键名再进递归：借用检查下不能一边遍历一边改
            let names: Vec<String> = object.keys().cloned().collect();
            for name in names {
                if let Some(child) = object.get_mut(&name) {
                    decrypt_walk(child, key, count)?;
                }
            }
            Ok(())
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                decrypt_walk(item, key, count)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

// ─── 取键：环境变量 + 运行中客户端内存 ───────────────────────

/// 收集候选保护密钥（来源名, 密钥字符串）。**顺序即优先级**。
///
/// `expected_key_id` 非空时，内存扫描命中即返回（不扫完所有进程）。
fn secret_candidates(expected_key_id: Option<&str>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    if let Ok(value) = std::env::var(SECRET_ENV) {
        let trimmed = value.trim().to_string();
        if !trimmed.is_empty() {
            out.push(("env".to_string(), trimmed));
        }
    }
    for secret in client_memory::scan_for_secrets(expected_key_id) {
        if out.iter().any(|(_, known)| *known == secret) {
            continue;
        }
        out.push(("client-memory".to_string(), secret));
    }
    out
}

// ─── 一站式入口 ──────────────────────────────────────────────

/// 交给落库链路的成果。
pub struct PreparedPayload {
    /// 明文 payload（信封已替换成字符串；本来就明文时原样返回）
    pub payload: Value,
    /// 解开的字段数（0 = 文件本来就是明文）
    pub decrypted_fields: usize,
    /// 密钥来源说明（`env` / `client-memory` / 空串）
    pub key_source: String,
    /// 保护密钥指纹（明文文件为空串）
    pub key_id: String,
}

/// 把官方客户端的登录态文件文本变成「可以直接落库的明文 payload」。
///
/// 三步：解析 JSON → 没有信封就原样返回（兼容 5.8.2 及更早的明文文件）→
/// 有信封就按「环境变量 → 客户端内存」的顺序找密钥并解开。
///
/// 全部候选都失败时返回可操作的中文错误（含两条取证提示），而不是静默降级：
/// 导入一个只有 uid、没有 token 的账号对用户毫无价值。
pub fn prepare_payload(text: &str) -> Result<PreparedPayload, String> {
    let payload: Value =
        serde_json::from_str(text).map_err(|error| format!("登录态文件不是合法 JSON：{error}"))?;
    if !has_encrypted_field(&payload) {
        return Ok(PreparedPayload {
            payload,
            decrypted_fields: 0,
            key_source: String::new(),
            key_id: String::new(),
        });
    }
    let expected = envelope_key_id(&payload);
    let candidates = secret_candidates(expected.as_deref());
    if candidates.is_empty() {
        return Err(format!(
            "登录态文件是加密形态（$wbEncrypted），但取不到 at-rest 保护密钥。\
             请让官方 WorkBuddy 客户端保持登录并运行后重试；\
             或在 `{SECRET_ENV}` 里直接填 44 字符的保护密钥（base64）。"
        ));
    }
    let mut last_error = String::from("没有候选密钥通过校验");
    for (source, secret) in candidates {
        let key = match ProtectorKey::from_secret(&secret) {
            Ok(key) => key,
            Err(error) => {
                last_error = format!("候选密钥（{source}）不合法：{error}");
                continue;
            }
        };
        if let Some(expected) = expected.as_deref() {
            if key.key_id() != expected {
                last_error = format!(
                    "候选密钥（{source}）指纹 {} 与文件要求的 {expected} 不一致",
                    key.key_id()
                );
                continue;
            }
        }
        let mut plain = payload.clone();
        let mut count = 0usize;
        match decrypt_walk(&mut plain, &key, &mut count) {
            Ok(()) if count > 0 => {
                return Ok(PreparedPayload {
                    payload: plain,
                    decrypted_fields: count,
                    key_source: source,
                    key_id: key.key_id().to_string(),
                });
            }
            Ok(()) => {
                last_error = format!("候选密钥（{source}）没有解开任何字段");
            }
            Err(error) => {
                last_error = format!("候选密钥（{source}）解密失败：{error}");
            }
        }
    }
    Err(format!(
        "登录态文件的 $wbEncrypted 字段解不开：{last_error}。\
         请确认客户端已登录、且与网关跑在同一个 Windows 用户下；\
         或在 `{SECRET_ENV}` 里直接填保护密钥（44 字符 base64）。"
    ))
}

/// 只取密钥、不解密（诊断用：打印指纹，方便与文件里的 keyId 对照）。
pub fn probe_key() -> Result<(String, String), String> {
    let candidates = secret_candidates(None);
    let Some((source, secret)) = candidates.into_iter().next() else {
        return Err(format!(
            "没找到候选保护密钥（既没有 `{SECRET_ENV}`，运行中的客户端内存里也没有）"
        ));
    };
    let key = ProtectorKey::from_secret(&secret)?;
    Ok((source, key.key_id().to_string()))
}

// ─── Windows：从运行中的客户端进程内存取键 ──────────────────

/// 关键：`OpenProcess` / `VirtualQueryEx` / `ReadProcessMemory` / 进程枚举
/// 只在 Windows 有；非 Windows 是一份返回空表的桩（保证 CI / 交叉编译能过）。
#[cfg(windows)]
mod client_memory {
    use super::{is_canonical_secret, ProtectorKey, SECRET_LEN};

    use std::ffi::c_void;

    use windows::Win32::Foundation::{CloseHandle, MAX_PATH};
    use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Memory::{
        VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    };

    /// 单个进程最多扫多少字节（防御性上限：客户端主进程实测 ~700 MB 出头）。
    const SCAN_LIMIT_BYTES: usize = 2 * 1024 * 1024 * 1024;

    /// 每次 `ReadProcessMemory` 的块大小。
    const SCAN_CHUNK_BYTES: usize = 4 * 1024 * 1024;

    /// 跨块命中的重叠长度：密钥字面量可能刚好落在块边界上。
    const SCAN_OVERLAP_BYTES: usize = 64;

    /// 目标进程名关键词（可执行文件 basename 小写后包含其一即扫）。
    const PROCESS_NEEDLES: [&str; 2] = ["workbuddy", "codebuddy"];

    /// 内存里密钥载荷的字面量前缀（客户端 `JSON.stringify` 出来的键序：
    /// `{"version":1,"atRestSecretKey":"…"}`）。
    const SECRET_KEY_LITERAL: &[u8] = b"\"atRestSecretKey\":\"";

    /// 扫所有「名字像 WorkBuddy / CodeBuddy」的进程，返回内存里出现过的
    /// 保护密钥候选（已去重、已过 canonical base64 形态校验）。
    ///
    /// `expected_key_id` 非空时，一旦某个候选的指纹对上就**立刻返回**：
    /// 实测第一个进程的第 4.7 秒就命中，没必要把 8 个进程扫完。
    pub fn scan_for_secrets(expected_key_id: Option<&str>) -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        let self_pid = std::process::id();
        for pid in target_pids() {
            if pid == self_pid {
                continue;
            }
            scan_process(pid, &mut found, expected_key_id);
            if matches_expected(&found, expected_key_id) {
                return found;
            }
        }
        found
    }

    /// 候选里是否已经有一把钥匙的指纹等于目标 keyId。
    fn matches_expected(found: &[String], expected_key_id: Option<&str>) -> bool {
        let Some(expected) = expected_key_id else {
            return !found.is_empty();
        };
        found.iter().any(|secret| {
            ProtectorKey::from_secret(secret)
                .map(|key| key.key_id() == expected)
                .unwrap_or(false)
        })
    }

    /// 枚举进程 pid（`CreateToolhelp32Snapshot` + `Process32FirstW/NextW`）。
    fn target_pids() -> Vec<u32> {
        let mut pids: Vec<u32> = Vec::new();
        // SAFETY：快照句柄由本函数拥有并在末尾关闭；`entry` 是出参缓冲，
        // 调用前已按契约写好 `dwSize`。
        let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
            return pids;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        // SAFETY：snapshot 是有效快照句柄；entry 已初始化（dwSize 必填）。
        if unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok() {
            loop {
                let name = exe_name(&entry).to_lowercase();
                if PROCESS_NEEDLES.iter().any(|needle| name.contains(needle)) {
                    pids.push(entry.th32ProcessID);
                }
                // SAFETY：同上；返回 Err 即枚举结束（ERROR_NO_MORE_FILES）。
                if unsafe { Process32NextW(snapshot, &mut entry) }.is_err() {
                    break;
                }
            }
        }
        // SAFETY：句柄来自 CreateToolhelp32Snapshot，只在这里关闭一次。
        let _ = unsafe { CloseHandle(snapshot) };
        pids
    }

    /// 进程可执行文件名（`szExeFile` 是定长宽字符数组，遇 0 截断）。
    fn exe_name(entry: &PROCESSENTRY32W) -> String {
        let end = entry
            .szExeFile
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(MAX_PATH as usize);
        String::from_utf16_lossy(&entry.szExeFile[..end])
    }

    /// 扫一个进程的已提交可读内存；命中目标指纹即刻收工。
    fn scan_process(pid: u32, found: &mut Vec<String>, expected_key_id: Option<&str>) {
        // SAFETY：只申请「查询 + 读内存」权限；失败（权限不足 / 进程已退）返回 Err。
        // 注意 `windows` 0.61 这里第三个参数是**原生 bool**（内部转 `BOOL`），不是 `BOOL(0)`。
        let Ok(handle) =
            (unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid) })
        else {
            return;
        };
        let mut buffer = vec![0u8; SCAN_CHUNK_BYTES];
        // window = 上一块的尾部 + 本块，专门治「密钥字面量跨块」
        let mut window: Vec<u8> = Vec::with_capacity(SCAN_CHUNK_BYTES + SCAN_OVERLAP_BYTES);
        let mut carry: Vec<u8> = Vec::with_capacity(SCAN_OVERLAP_BYTES);
        let mut region = MEMORY_BASIC_INFORMATION::default();
        let mut address = 0usize;
        let mut total = 0usize;
        while address < 0x7fff_ffff_ffff && total < SCAN_LIMIT_BYTES {
            // SAFETY：handle 有效；region 是出参；长度按结构体大小给。
            // `windows` 0.61 的 `lpaddress` 是 `Option<*const c_void>`，必须包一层 `Some`。
            let written = unsafe {
                VirtualQueryEx(
                    handle,
                    Some(address as *const c_void),
                    &mut region,
                    std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
                )
            };
            if written == 0 {
                break;
            }
            let base = region.BaseAddress as usize;
            let size = region.RegionSize;
            if size == 0 {
                break;
            }
            if region.State == MEM_COMMIT
                && (region.Protect.0 & PAGE_GUARD.0) == 0
                && is_readable(region.Protect.0)
            {
                let mut cursor = base;
                let end = base.saturating_add(size);
                while cursor < end && total < SCAN_LIMIT_BYTES {
                    let take = SCAN_CHUNK_BYTES.min(end - cursor);
                    let mut read = 0usize;
                    // SAFETY：目标区间来自 VirtualQueryEx 的「已提交可读」描述；
                    // buffer 容量 >= take；实际读取字节数由 API 回填到 read。
                    let ok = unsafe {
                        ReadProcessMemory(
                            handle,
                            cursor as *const c_void,
                            buffer.as_mut_ptr() as *mut c_void,
                            take,
                            Some(&mut read as *mut usize),
                        )
                    };
                    if ok.is_err() || read == 0 || read > buffer.len() {
                        break;
                    }
                    window.clear();
                    window.extend_from_slice(&carry);
                    window.extend_from_slice(&buffer[..read]);
                    collect_secrets_in(&window, found);
                    if matches_expected(found, expected_key_id) {
                        // SAFETY：句柄来自本函数的 OpenProcess，这里关闭一次。
                        let _ = unsafe { CloseHandle(handle) };
                        return;
                    }
                    let keep = window.len().min(SCAN_OVERLAP_BYTES);
                    carry.clear();
                    carry.extend_from_slice(&window[window.len() - keep..]);
                    total += read;
                    cursor += take;
                }
            }
            address = base.saturating_add(size);
        }
        // SAFETY：同上，唯一关闭点。
        let _ = unsafe { CloseHandle(handle) };
    }

    /// 该保护位可读吗（对照 `PAGE_READONLY / READWRITE / EXECUTE_READ /
    /// EXECUTE_READWRITE / WRITECOPY`）。
    fn is_readable(protect: u32) -> bool {
        matches!(protect & 0xff, 0x02 | 0x04 | 0x20 | 0x40 | 0x80)
    }

    /// 从一段内存窗口里挑出所有形如 `"atRestSecretKey":"<44 字符>"` 的候选。
    ///
    /// 不依赖正则：字面量前缀 + 固定长度 + `is_canonical_secret` 三道约束，
    /// 误报率低到可以忽略（且命中还要真解开信封才算数）。
    fn collect_secrets_in(blob: &[u8], out: &mut Vec<String>) {
        let mut index = 0usize;
        while index + SECRET_KEY_LITERAL.len() < blob.len() {
            let Some(offset) = find_subslice(&blob[index..], SECRET_KEY_LITERAL) else {
                return;
            };
            let start = index + offset + SECRET_KEY_LITERAL.len();
            let end = start.saturating_add(SECRET_LEN);
            if end > blob.len() {
                return;
            }
            if let Ok(candidate) = std::str::from_utf8(&blob[start..end]) {
                if is_canonical_secret(candidate) && !out.iter().any(|known| known == candidate) {
                    out.push(candidate.to_string());
                }
            }
            index = start;
        }
    }

    /// 朴素子串查找。
    fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        if needle.is_empty() || haystack.len() < needle.len() {
            return None;
        }
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }
}

/// 非 Windows 桩：读别的进程内存是 Win32 专有（`VirtualQueryEx` /
/// `ReadProcessMemory` 都在 kernel32）。返回空列表 → 上层只剩
/// `WORKBUDDY_AT_REST_SECRET_KEY` 一条取键路径，错误文案会说清楚。
#[cfg(not(windows))]
mod client_memory {
    /// 永远返回空（非 Windows 上没有取键路径）。
    pub fn scan_for_secrets(_expected_key_id: Option<&str>) -> Vec<String> {
        Vec::new()
    }
}

// ─── 单测 ────────────────────────────────────────────────────
//
// 这里允许 `unwrap` / `expect`（整个 `tests` 模块只在 `cfg(test)` 下编译，
// 不进任何产物）—— 模块正文那条「零 unwrap」的硬约束针对 lib 本体。
#[cfg(test)]
mod tests {
    use super::*;

    /// 合成密钥用的固定 secret（`base64(32 个 0x01)`）——避免用全零哨兵值，
    /// 那份会被 `from_secret` 按设计拒绝。
    const SYNTH_SECRET: &str = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";

    /// AAD 逐字节比对：期望值由**独立实现**（`probe/atrest_reference_values.py`，
    /// 按客户端 `buildAuthenticatedContextAad` 的 field 档手拼）算出：
    /// `57422d4141440001 | 00000005 5742455631 | 00000006 73796d2d7631 |
    ///  00000001 | 00000010 39313237646561316234343032306137 | 02 00 00`
    /// （第二段是 ASCII `sym-v1` —— 写成 `7379796d2d7631` 就是 "syym-v1"，
    ///   本用例第一版正是这么抄错的，跑一次就被逐字节比对抓出来了。）
    #[test]
    fn field_aad_matches_client_layout() {
        let aad = build_field_aad("9127dea1b44020a7", SUITE_V1);
        let expected = "57422d4141440001000000055742455631\
                        0000000673796d2d7631\
                        000000010000001039313237646561316234343032306137020000";
        let expected = expected.replace('\n', "").replace(' ', "");
        assert_eq!(aad.len(), 54, "AAD 长度应为 54 字节");
        assert_eq!(hex_of(&aad), expected, "AAD 字节序列与客户端实现不一致");
    }

    /// 密钥推导：`key = sha256(secret 字符串)`、`keyId = sha256(key).hex()[:16]`。
    /// 期望值同样来自独立实现（Python 的 `hashlib`）。这一条专治「把 secret
    /// 先 base64 解码再哈希」这个最容易踩的错。
    #[test]
    fn key_derivation_matches_client() {
        let key = ProtectorKey::from_secret(SYNTH_SECRET).expect("合成密钥应当合法");
        assert_eq!(key.key_id(), "9636e5f19b1399d9");
        assert_eq!(
            hex_of(&key.key),
            "c57cc3b55d39f9a6f772f508e3821e0a280822352c0d0f0d259fba63b74da9a0"
        );
    }

    /// 端到端：用合成密钥解一条**独立实现封出来**的字段信封，明文必须逐字对得上。
    /// 期望信封由 Python 的 `cryptography` 生成（AAD 同上），所以这轮同时覆盖了
    /// 解析信封 → 校验 keyId → AAD → GCM → UTF-8 整条链。
    #[test]
    fn opens_envelope_sealed_by_independent_implementation() {
        let key = ProtectorKey::from_secret(SYNTH_SECRET).expect("合成密钥应当合法");
        let envelope = "eyJhdXRoVGFnIjoiM3M5dDNnY3MzR2RvSXpTRmt2UzRXUT09IiwiY2lwaGVydGV4dCI6Ik5iak92b3ZPOFp2RFJjREtsbHpNIiwia2V5SWQiOiI5NjM2ZTVmMTliMTM5OWQ5Iiwibm9uY2UiOiJBQUVDQXdRRkJnY0lDUW9MIiwic3VpdGUiOjF9";
        let plaintext = open_field_envelope(&key, envelope).expect("解封应当成功");
        assert_eq!(plaintext, "hello-workbuddy");
    }

    /// 允许申明类的形状判定：明文 payload 不触发解密链，信封 payload 走解密链。
    #[test]
    fn detects_wrapped_payload() {
        let plain: Value = serde_json::from_str(r#"{"auth":{"accessToken":"eyJx"}}"#)
            .expect("测试用 JSON 应当合法");
        assert!(!has_encrypted_field(&plain));
        assert!(envelope_key_id(&plain).is_none());

        let wrapped: Value = serde_json::from_str(
            r#"{"auth":{"accessToken":{"$wbEncrypted":1,"envelope":"eyJzdWl0ZSI6MSwia2V5SWQiOiI5NjM2ZTVmMTliMTM5OWQ5Iiwibm9uY2UiOiJBQUVDQXdRRkJnY0lDUW9MIiwiYXV0aFRhZyI6IjNzOXQzZ2NzM0dkb0l6U0ZrdlM0V1E9PSIsImNpcGhlcnRleHQiOiJOYmpPdm92TzhadkRSY0RLbGx6TSJ9"}}}"#,
        )
        .expect("测试用 JSON 应当合法");
        assert!(has_encrypted_field(&wrapped));
        assert_eq!(
            envelope_key_id(&wrapped).as_deref(),
            Some("9636e5f19b1399d9")
        );
    }

    /// **真文件端到端**（默认 ignore，需要客户端在跑或设了 `SECRET_ENV`）：
    /// 读本机登录态文件 → 取键 → 解开全部 `$wbEncrypted` 字段 → 断言拿到 JWT。
    ///
    /// ```text
    /// cargo test -p agent2api-server --lib workbuddy_atrest -- --include-ignored --nocapture
    /// ```
    #[test]
    #[ignore = "需要官方客户端在运行（或设 WORKBUDDY_AT_REST_SECRET_KEY）"]
    fn decrypts_real_auth_file() {
        let base = std::env::var("LOCALAPPDATA").unwrap_or_default();
        let dir = std::path::PathBuf::from(base)
            .join("CodeBuddyExtension")
            .join("Data")
            .join("Public")
            .join("auth");
        let candidate = dir.join("workbuddy-desktop.info");
        let path = if candidate.is_file() {
            candidate
        } else {
            let mut backups: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
                .expect("登录态目录应当可读")
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|item| {
                    item.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with("workbuddy-desktop."))
                })
                .collect();
            backups.sort();
            backups.into_iter().next_back().expect("应当存在登录态文件")
        };
        let text = std::fs::read_to_string(&path).expect("登录态文件应当可读");
        let prepared = prepare_payload(&text).expect("解密应当成功");
        assert!(prepared.decrypted_fields > 0, "应当解开至少一个字段");
        assert!(!prepared.key_id.is_empty(), "应当记录保护密钥指纹");
        let token = prepared
            .payload
            .get("auth")
            .and_then(|auth| auth.get("accessToken"))
            .and_then(Value::as_str)
            .unwrap_or("");
        assert!(
            token.starts_with("eyJ"),
            "解出来的 accessToken 应当是 JWT 文本"
        );
        println!(
            "文件={} 字段={} 来源={} 指纹={} 令牌长度={}",
            path.display(),
            prepared.decrypted_fields,
            prepared.key_source,
            prepared.key_id,
            token.len()
        );
    }

    /// 字节切片转小写十六进制（只在测试里用）。
    fn hex_of(bytes: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0f) as usize] as char);
        }
        out
    }
}

// ─── VERIFICATION（2026-10-20，本机实测，Windows 11 / WorkBuddy 5.8.3）──
//
// 1. 保护密钥取自运行中客户端内存（8 个 `WorkBuddy.exe`，只读扫 3.7 GB）；
//    指纹 `9127dea1b44020a7` 与登录态文件信封的 `keyId`、
//    `~/.workbuddy/keyblob` 的 `slots[0].protectorKeyId` 三方一致。
// 2. 同一密钥解开 keyblob 的 `slots[0].wrapped`（framing=file）得到主数据键，
//    其指纹等于 keyblob 的 `keyId`（`e2286126792f1557`）。
// 3. 本文件的字段档 AAD + AES-256-GCM 解开登录态文件里全部 8 个
//    `$wbEncrypted` 字段：昵称、手机号各 3 条、accessToken（1331 字符）、
//    refreshToken（700 字符）。
// 4. 解出的 accessToken 真打上游 `GET /v2/activity/growth/tasks`：
//    HTTP 200 / `code=0` / 17 个任务 —— 证明解出来的就是可用的登录态。
//
// 复现命令（本机无 cargo / rustc，未在本仓编译，见 `docs/growth-new-activity.md`
// 的 MODIFIED / NOT RUN 段）：`cargo check -p agent2api-server`。

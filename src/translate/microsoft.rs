//! 微软翻译（内置，无需 API Key、无需鉴权）。
//!
//! 两种请求模式：
//! - `edge`（默认）：POST `edge.microsoft.com/translate/translatetext`，完全免鉴权。
//!   响应格式还是 `[{"translations":[{"text":"..."}]}]`，跟老的 Azure 端点一致。
//! - `signature`：自己算 `X-MT-Signature`（HMAC-SHA256 + 硬编码私钥，抄自 GTranslate），
//!   打 `api.cognitive.microsofttranslator.com`。
//!
//! 历史背景：老实现是「先 GET `edge.microsoft.com/translate/auth` 拿 JWT，再打 api-edge」，
//! 但微软在 2026-07-30 关停了 Edge 翻译服务，`/translate/auth` 对所有 UA/路径变体一律 404
//! （`/translate/*` 整个命名空间都没了）。现在只剩 `/translate/translatetext` 这条免鉴权路径。

use crate::config::{AppConfig, MS_MODE_SIGNATURE};
use base64::Engine as _;
use hmac::{Hmac, Mac};
use serde_json::json;
use sha2::Sha256;
use time::format_description::FormatItem;
use time::macros::format_description;
use time::OffsetDateTime;

type HmacSha256 = Hmac<Sha256>;

const LEGACY_ENDPOINT: &str = "api.cognitive.microsofttranslator.com";
const EDGE_ENDPOINT: &str = "edge.microsoft.com";
const API_VERSION: &str = "3.0";

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                  (KHTML, like Gecko) Chrome/127.0.0.0 Safari/537.36 Edg/127.0.0.0";

/// 抄自 GTranslate 的私钥，属于非官方接口，随时可能失效。
const PRIVATE_KEY: [u8; 64] = [
    0xa2, 0x29, 0x3a, 0x3d, 0xd0, 0xdd, 0x32, 0x73, 0x97, 0x7a, 0x64, 0xdb, 0xc2, 0xf3, 0x27, 0xf5,
    0xd7, 0xbf, 0x87, 0xd9, 0x45, 0x9d, 0xf0, 0x5a, 0x09, 0x66, 0xc6, 0x30, 0xc6, 0x6a, 0xaa, 0x84,
    0x9a, 0x41, 0xaa, 0x94, 0x3a, 0xa8, 0xd5, 0x1a, 0x6e, 0x4d, 0xaa, 0xc9, 0xa3, 0x70, 0x12, 0x35,
    0xc7, 0xeb, 0x12, 0xf6, 0xe8, 0x23, 0x07, 0x9e, 0x47, 0x10, 0x95, 0x91, 0x88, 0x55, 0xd8, 0x17,
];

/// `ddd, dd MMM yyyy HH:mm:ssG\MT`（C# 里那个，注意 GMT 前没有空格）
const DATE_FMT: &[FormatItem] = format_description!(
    "[weekday repr:short], [day padding:zero] [month repr:short] [year] [hour]:[minute]:[second]GMT"
);

pub fn translate(
    client: &reqwest::blocking::Client,
    cfg: &AppConfig,
    text: &str,
) -> Result<String, String> {
    use crate::translate::lang;

    let from = lang::microsoft(&cfg.from)
        .ok_or_else(|| format!("微软翻译不支持该语言: {}", cfg.from))?;
    let to = lang::microsoft(&cfg.to)
        .ok_or_else(|| format!("微软翻译不支持该语言: {}", cfg.to))?;

    let raw = if cfg.microsoft_mode == MS_MODE_SIGNATURE {
        translate_with_signature(client, text, from, to)?
    } else {
        translate_with_edge(client, text, from, to)?
    };

    extract(raw)
}

/// 免鉴权的新 Edge 端点。
fn translate_with_edge(
    client: &reqwest::blocking::Client,
    text: &str,
    from: &str,
    to: &str,
) -> Result<String, String> {
    // from 留空表示让服务端自动检测；写 auto 反而会被当成语言码
    let from_param = if from == "auto" { "" } else { from };
    let url = format!(
        "https://{EDGE_ENDPOINT}/translate/translatetext?from={from_param}&to={to}&isEnterpriseClient=false"
    );

    client
        .post(url)
        .header("User-Agent", UA)
        // 注意：这个端点收的是字符串数组，不是 `[{"Text": ...}]`
        .json(&json!([text]))
        .send()
        .map_err(|e| format!("请求失败: {e}"))?
        .text()
        .map_err(|e| format!("读取响应失败: {e}"))
}

/// 老的 Azure 端点 + 自算签名。
fn translate_with_signature(
    client: &reqwest::blocking::Client,
    text: &str,
    from: &str,
    to: &str,
) -> Result<String, String> {
    let path = build_path(from, to);
    let url = format!("https://{path}");

    client
        .post(url)
        .header("User-Agent", UA)
        .header("X-MT-Signature", signature(&path))
        .json(&json!([{ "Text": text }]))
        .send()
        .map_err(|e| format!("请求失败: {e}"))?
        .text()
        .map_err(|e| format!("读取响应失败: {e}"))
}

fn build_path(from: &str, to: &str) -> String {
    let mut path = format!("{LEGACY_ENDPOINT}/translate?api-version={API_VERSION}&to={to}");
    if !from.is_empty() && from != "auto" {
        path.push_str(&format!("&from={from}"));
    }
    path
}

/// `[{"translations":[{"text":"..."}]}]` -> 译文
fn extract(raw: String) -> Result<String, String> {
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("解析响应失败: {e}\n原始: {raw}"))?;

    value
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("translations"))
        .and_then(|t| t.as_array())
        .and_then(|t| t.first())
        .and_then(|t| t.get("text"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("响应里没有译文\n原始: {raw}"))
}

/// `X-MT-Signature` 头。
fn signature(path: &str) -> String {
    let guid = uuid::Uuid::new_v4().simple().to_string();
    let escaped = urlencoding::encode(path);
    let date = OffsetDateTime::now_utc()
        .format(DATE_FMT)
        .unwrap_or_default();

    let message = format!("MSTranslatorAndroidApp{escaped}{date}{guid}").to_lowercase();

    let mut mac = HmacSha256::new_from_slice(&PRIVATE_KEY).expect("HMAC 接受任意长度密钥");
    mac.update(message.as_bytes());
    let digest = mac.finalize().into_bytes();
    let signature = base64::engine::general_purpose::STANDARD.encode(digest);

    format!("MSTranslatorAndroidApp::{signature}::{date}::{guid}")
}

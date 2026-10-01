//! Google 翻译（内置，无需 API Key）。
//!
//! 两种请求模式，移植自 `Plugins/STranslate.Plugin.Translate.GoogleBuiltIn/Main.cs`：
//! - `direct`（默认）：GET `translate.google.com/translate_a/single?client=gtx&dt=t&dj=1&...`，
//!   拼 `sentences[].trans`。能否连通交给系统代理，不在应用里另设代理。
//! - `custom`：POST `google_api_url`，请求体 `{text, source_lang, target_lang}`，响应取 `data`。
//!   适合换成你自己搭的 Worker / Deno 代理（作者的 Deno 代理已因超配额暂停）。

use crate::config::{AppConfig, GOOGLE_MODE_DIRECT};
use serde_json::json;

const GOOGLE_TRANSLATE_URL: &str = "https://translate.google.com/translate_a/single";

pub fn translate(
    client: &reqwest::blocking::Client,
    cfg: &AppConfig,
    text: &str,
) -> Result<String, String> {
    use crate::translate::lang;

    let from = lang::google(&cfg.from)
        .ok_or_else(|| format!("Google 翻译不支持该语言: {}", cfg.from))?;
    let to = lang::google(&cfg.to)
        .ok_or_else(|| format!("Google 翻译不支持该语言: {}", cfg.to))?;

    if cfg.google_mode == GOOGLE_MODE_DIRECT {
        translate_direct(client, text, from, to)
    } else {
        translate_custom(client, cfg, text, from, to)
    }
}

/// 走自定义翻译服务（`custom` 模式）。
fn translate_custom(
    client: &reqwest::blocking::Client,
    cfg: &AppConfig,
    text: &str,
    from: &str,
    to: &str,
) -> Result<String, String> {
    let url = cfg.google_api_url.trim();
    if url.is_empty() {
        return Err("Google 自定义服务地址为空，请在设置里填写".to_string());
    }

    let raw = client
        .post(url)
        .json(&json!({
            "text": text,
            "source_lang": from,
            "target_lang": to,
        }))
        .send()
        .map_err(|e| format!("请求失败: {e}"))?
        .text()
        .map_err(|e| format!("读取响应失败: {e}"))?;

    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("解析响应失败: {e}\n原始: {raw}"))?;

    value
        .get("data")
        .and_then(|d| d.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("响应里没有译文\n原始: {raw}"))
}

/// 直连 Google 官方接口（出网走系统代理）。
fn translate_direct(
    client: &reqwest::blocking::Client,
    text: &str,
    from: &str,
    to: &str,
) -> Result<String, String> {
    let sl = from.to_lowercase();
    let tl = to.to_lowercase();

    let raw = client
        .get(GOOGLE_TRANSLATE_URL)
        .query(&[
            ("client", "gtx"),
            ("dt", "t"),
            ("dj", "1"),
            ("ie", "UTF-8"),
            ("oe", "UTF-8"),
            ("sl", sl.as_str()),
            ("tl", tl.as_str()),
            ("q", text),
        ])
        .send()
        .map_err(|e| format!("请求失败: {e}"))?
        .text()
        .map_err(|e| format!("读取响应失败: {e}"))?;

    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("解析响应失败: {e}\n原始: {raw}"))?;

    // dj=1 时是结构化 JSON：sentences[].trans
    let translated: String = value
        .get("sentences")
        .and_then(|s| s.as_array())
        .ok_or_else(|| format!("响应里没有 sentences\n原始: {raw}"))?
        .iter()
        .map(|s| s.get("trans").and_then(|t| t.as_str()).unwrap_or_default())
        .collect();

    if translated.trim().is_empty() {
        return Err(format!("响应里没有译文\n原始: {raw}"));
    }

    Ok(translated)
}

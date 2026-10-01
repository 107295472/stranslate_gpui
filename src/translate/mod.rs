//! 翻译引擎：微软 + Google，都是内置、免 Key 的。

pub mod google;
pub mod lang;
pub mod microsoft;

use crate::config::AppConfig;

/// 统一入口。`cfg` 里带着引擎、语言和请求模式。
pub fn translate(cfg: &AppConfig, text: &str) -> Result<String, String> {
    if text.trim().is_empty() {
        return Ok(String::new());
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(
            cfg.http_timeout_secs.max(1) as u64
        ))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;

    match cfg.engine.as_str() {
        "google" => google::translate(&client, cfg, text),
        _ => microsoft::translate(&client, cfg, text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 这几个都要联网，默认跳过。
    /// 手动跑：`cargo test -- --ignored --nocapture`
    fn probe(cfg: &AppConfig, text: &str) {
        match translate(cfg, text) {
            Ok(out) => println!("OK  -> {out}"),
            Err(e) => panic!("失败: {e}"),
        }
    }

    #[test]
    #[ignore = "需要联网"]
    #[allow(clippy::field_reassign_with_default)]
    fn microsoft_edge() {
        let mut cfg = AppConfig::default();
        cfg.engine = "microsoft".into();
        cfg.microsoft_mode = "edge".into();
        cfg.from = "en".into();
        cfg.to = "zh-Hans".into();
        probe(&cfg, "Hello, world!");
    }

    #[test]
    #[ignore = "需要联网"]
    #[allow(clippy::field_reassign_with_default)]
    fn microsoft_signature() {
        let mut cfg = AppConfig::default();
        cfg.engine = "microsoft".into();
        cfg.microsoft_mode = "signature".into();
        cfg.from = "en".into();
        cfg.to = "zh-Hans".into();
        probe(&cfg, "Good morning.");
    }

    #[test]
    #[ignore = "需要联网"]
    #[allow(clippy::field_reassign_with_default)]
    fn google_direct() {
        let mut cfg = AppConfig::default();
        cfg.engine = "google".into();
        cfg.google_mode = "direct".into();
        cfg.from = "en".into();
        cfg.to = "zh-Hans".into();
        probe(&cfg, "See you tomorrow.");
    }

    /// custom 模式没填地址时要直接报错，而不是发一个空 URL 的请求。
    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn google_custom_without_url_errors() {
        let mut cfg = AppConfig::default();
        cfg.engine = "google".into();
        cfg.google_mode = "custom".into();
        cfg.google_api_url = String::new();

        let err = translate(&cfg, "hello").unwrap_err();
        assert!(err.contains("为空"), "错误信息不对: {err}");
    }
}

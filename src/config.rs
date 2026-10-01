//! 应用配置：默认引擎、请求模式、划词开关等。
//!
//! 对应原项目 `src/STranslate/Core/Settings.cs` 中被保留下来的那一小撮字段。

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::OnceLock;

/// Google 默认走作者自建的代理，因为直连 translate.google.com 在国内不可达。
///
/// 注意：这个代理 2026-09 实测已因 Deno Deploy 超配额被暂停（503 USAGE_EXCEEDED），
/// 所以 `google_api_url` 默认留空，要用自定义服务时自己填。
pub const DEFAULT_GOOGLE_API_URL: &str = "";

/// 微软请求模式。
pub const MS_MODE_EDGE: &str = "edge"; // POST edge.microsoft.com/translate/translatetext，免鉴权，推荐
pub const MS_MODE_SIGNATURE: &str = "signature"; // 自算 X-MT-Signature，打 api.cognitive.microsofttranslator.com
// 注：老实现的 "先取 edge.microsoft.com/translate/auth 的 JWT" 模式已失效——
// 微软 2026-07-30 关停 Edge 翻译服务，该端点对所有请求一律 404。

/// Google 请求模式。
pub const GOOGLE_MODE_DIRECT: &str = "direct"; // 直连 translate.google.com，出网交给系统代理
pub const GOOGLE_MODE_CUSTOM: &str = "custom"; // POST 到 google_api_url（自建/第三方翻译服务）
// 兼容：老配置里写的是 "proxy"，语义等同 custom

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// "microsoft" | "google"
    pub engine: String,
    /// 语言 id，见 `translate::lang`
    pub from: String,
    pub to: String,

    pub microsoft_mode: String,

    pub google_mode: String,
    pub google_api_url: String,

    /// 划词开关（开启后注册 Alt+D 全局热键）
    pub selection_enabled: bool,
    /// 除热键外，是否还开启「鼠标自动划词」（拖选/双击后自动弹窗）
    pub selection_auto_mouse: bool,
    /// 取词超时（原项目默认 500ms）
    pub selection_fetch_timeout_ms: u32,
    /// HTTP 超时（原项目默认 30s）
    pub http_timeout_secs: u32,

    /// 界面/译文使用的字体族。空串表示跟随系统默认字体。
    pub font_family: String,
    /// 界面/译文字号（pt）。
    pub font_size: u32,
    /// 内容字重，取 Flutter 的 FontWeight.value（100..900，常用 400/500/600）。
    pub font_weight: u32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            engine: "microsoft".to_string(),
            from: "auto".to_string(),
            to: "zh-Hans".to_string(),
            microsoft_mode: MS_MODE_EDGE.to_string(),
            google_mode: GOOGLE_MODE_DIRECT.to_string(),
            google_api_url: DEFAULT_GOOGLE_API_URL.to_string(),
            selection_enabled: true,
            selection_auto_mouse: false,
            selection_fetch_timeout_ms: 500,
            http_timeout_secs: 30,
            font_family: String::new(),
            font_size: 14,
            font_weight: 400,
        }
    }
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("STranslate")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

static CFG: OnceLock<RwLock<AppConfig>> = OnceLock::new();

fn cell() -> &'static RwLock<AppConfig> {
    CFG.get_or_init(|| RwLock::new(load_from_disk()))
}

fn load_from_disk() -> AppConfig {
    match std::fs::read_to_string(config_path()) {
        Ok(text) => toml::from_str(&text).unwrap_or_default(),
        Err(_) => AppConfig::default(),
    }
}

/// 读取当前配置（首次调用时从磁盘加载）。
pub fn get() -> AppConfig {
    cell().read().clone()
}

/// 覆盖并落盘。
pub fn set(cfg: AppConfig) -> Result<(), String> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建配置目录失败: {e}"))?;

    let text = toml::to_string_pretty(&cfg).map_err(|e| format!("序列化配置失败: {e}"))?;
    // 先写同目录临时文件再改名，避免写一半把配置写坏。
    let tmp = dir.join("config.toml.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("写入配置失败: {e}"))?;
    std::fs::rename(&tmp, config_path()).map_err(|e| format!("保存配置失败: {e}"))?;

    *cell().write() = cfg;
    Ok(())
}

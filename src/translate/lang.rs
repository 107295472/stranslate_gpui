//! 语言表：一张表同时给出微软和 Google 的语言代码。
//!
//! 两张映射表分别抄自：
//! - `STranslate.Plugin.Translate.MicrosoftBuiltIn/Main.cs`
//! - `STranslate.Plugin.Translate.GoogleBuiltIn/Main.cs`

/// 暴露给 Dart 的语言项。
#[derive(Debug, Clone)]
pub struct LangInfo {
    /// 统一 id（本项目内部使用，也是配置文件里存的写法）
    pub id: String,
    /// 显示名
    pub name: String,
}

struct LangDef {
    id: &'static str,
    name: &'static str,
    /// 微软的语言代码，None 表示该引擎不支持
    microsoft: Option<&'static str>,
    /// Google 的语言代码，None 表示该引擎不支持
    google: Option<&'static str>,
}

const DEFS: &[LangDef] = &[
    LangDef { id: "auto", name: "自动检测", microsoft: Some("auto"), google: Some("auto") },
    LangDef { id: "zh-Hans", name: "简体中文", microsoft: Some("zh-Hans"), google: Some("zh-CN") },
    LangDef { id: "zh-Hant", name: "繁体中文", microsoft: Some("zh-Hant"), google: Some("zh-TW") },
    LangDef { id: "yue", name: "粤语", microsoft: None, google: Some("yue") },
    LangDef { id: "en", name: "英语", microsoft: Some("en"), google: Some("en") },
    LangDef { id: "ja", name: "日语", microsoft: Some("ja"), google: Some("ja") },
    LangDef { id: "ko", name: "韩语", microsoft: Some("ko"), google: Some("ko") },
    LangDef { id: "fr", name: "法语", microsoft: Some("fr"), google: Some("fr") },
    LangDef { id: "es", name: "西班牙语", microsoft: Some("es"), google: Some("es") },
    LangDef { id: "ru", name: "俄语", microsoft: Some("ru"), google: Some("ru") },
    LangDef { id: "de", name: "德语", microsoft: Some("de"), google: Some("de") },
    LangDef { id: "it", name: "意大利语", microsoft: Some("it"), google: Some("it") },
    LangDef { id: "tr", name: "土耳其语", microsoft: Some("tr"), google: Some("tr") },
    LangDef { id: "pt-pt", name: "葡萄牙语(葡萄牙)", microsoft: Some("pt-pt"), google: Some("pt") },
    LangDef { id: "pt", name: "葡萄牙语(巴西)", microsoft: Some("pt"), google: Some("pt") },
    LangDef { id: "vi", name: "越南语", microsoft: Some("vi"), google: Some("vi") },
    LangDef { id: "id", name: "印尼语", microsoft: Some("id"), google: Some("id") },
    LangDef { id: "th", name: "泰语", microsoft: Some("th"), google: Some("th") },
    LangDef { id: "ms", name: "马来语", microsoft: Some("ms"), google: Some("ms") },
    LangDef { id: "ar", name: "阿拉伯语", microsoft: Some("ar"), google: Some("ar") },
    LangDef { id: "hi", name: "印地语", microsoft: None, google: Some("hi") },
    LangDef { id: "mn-Cyrl", name: "蒙古语(西里尔)", microsoft: Some("mn-Cyrl"), google: Some("mn") },
    LangDef { id: "mn-Mong", name: "蒙古语(传统)", microsoft: Some("mn-Mong"), google: Some("mn") },
    LangDef { id: "km", name: "高棉语", microsoft: Some("km"), google: Some("km") },
    LangDef { id: "nb", name: "挪威语", microsoft: Some("nb"), google: Some("no") },
    LangDef { id: "fa", name: "波斯语", microsoft: Some("fa"), google: Some("fa") },
    LangDef { id: "sv", name: "瑞典语", microsoft: Some("sv"), google: Some("sv") },
    LangDef { id: "pl", name: "波兰语", microsoft: Some("pl"), google: Some("pl") },
    LangDef { id: "nl", name: "荷兰语", microsoft: Some("nl"), google: Some("nl") },
    LangDef { id: "uk", name: "乌克兰语", microsoft: Some("uk"), google: Some("uk") },
    LangDef { id: "uz", name: "乌兹别克语", microsoft: Some("uz"), google: Some("uz") },
    LangDef { id: "ug", name: "维吾尔语", microsoft: Some("ug"), google: Some("ug") },
];

pub fn all() -> Vec<LangInfo> {
    DEFS.iter()
        .map(|d| LangInfo { id: d.id.to_string(), name: d.name.to_string() })
        .collect()
}

fn find(id: &str) -> Option<&'static LangDef> {
    DEFS.iter().find(|d| d.id == id)
}

/// 统一 id -> 微软语言代码；None 表示不支持。
pub fn microsoft(id: &str) -> Option<&'static str> {
    find(id).and_then(|d| d.microsoft)
}

/// 统一 id -> Google 语言代码；None 表示不支持。
pub fn google(id: &str) -> Option<&'static str> {
    find(id).and_then(|d| d.google)
}

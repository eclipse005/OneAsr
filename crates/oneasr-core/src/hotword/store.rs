//! 热词数据的**独立持久化**：`{data}/hotword.json`。
//!
//! 为什么不跟 `settings.json` 挤在一份里：热词是**用户内容**（词表、组名），
//! `settings.json` 是**机器配置**（模型目录、开关、格式）。两者生命周期完全不同——
//! 重置设置不该清空用户辛辛苦苦攒的词表，反过来用户整份删掉热词也不该动 ASR 引擎
//! 的选择。合在一份时，「重置」要么误删词表，要么得为它开特例，两头不讨好。
//!
//! 落盘字段：词组 / 默认组 / **新任务是否默认用热词** / 每段注入条数 / 召回阈值。
//! **不在这里**的：SenseVoice 模型目录（它和 `demucs_model_dir` 一样是模型路径，
//! 属于 `settings.json`），以及每次运行临时选中的组（见
//! [`crate::settings::Settings::hotword_run`]，那是运行期状态、不落盘）。
//!
//! 自愈（[`Self::normalize`]）保证三件事：至少留一个组、默认组指向真实存在的组、
//! **组 id 不重复**。最后一条不是洁癖：开发期用进程级计数器发 id，重启后会与已
//! 持久化的 id 撞号，而查找一律 `find` 命中第一个——删一组连带删掉同号的那组、
//! 编辑 A 写进 B、召回用的是 A 的词。自愈会把已经写坏的文件修回来。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::count_words;
use crate::i18n::UiLang;

/// 默认热词组的保留 id（也是「用户还没起名」时显示的占位名）。
pub const DEFAULT_HOTWORD_GROUP_ID: &str = "default";

/// 默认每段注入的条数。上游在 60 s 切片上实测量过 30 条仍「可用」
/// （prefill 378 ms），20 更保守；而且位置越靠前越有效，条数堆多了收益递减。
pub const DEFAULT_TOP_K: usize = 20;

/// 默认召回阈值。0.85 是上游用 SenseVoice 粗转实测的拐点
/// （召回 85.3% / 精度 96.7%）。
pub const DEFAULT_THRESHOLD: f64 = 0.85;

/// 一个热词组。
///
/// `id` 是稳定标识，任务里记的是它——**改名不影响任务已选的组**。`words` 就是
/// [`super::parse_word_list`] 认的纯文本（逗号或换行分隔、只写正确写法），
/// 不落盘任何已解析的结构：展示 = 存储 = 用户输入，UI 边界不做格式转换。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotwordGroup {
    /// 稳定标识（`default`，或 `g{n}` 形式的新组 id）。改名不动它。
    pub id: String,
    /// 组名。空 = 还没起名，界面显示本地化的占位名。
    #[serde(default)]
    pub name: String,
    /// 词表：逗号 / 中英文逗号 / 换行分隔，一行一个词。
    #[serde(default)]
    pub words: String,
}

/// 提示 / 报错里显示的名字：起过名就用名字；没起名用**本地化占位名**
/// （「默认」/「未命名」、`Default` / `Untitled`），绝不显示 `default` / `g1`
/// 这类机器 id。
///
/// 纯格式化，显式收 `lang`：单测不读全局语言，也就不会随 CI 机器的系统语言
/// 忽绿忽红（AGENTS.md 的规矩）。
pub fn display_name(group: &HotwordGroup, lang: UiLang) -> String {
    if !group.name.trim().is_empty() {
        return group.name.clone();
    }
    crate::i18n::hotword_group_placeholder(lang, group.id == DEFAULT_HOTWORD_GROUP_ID).to_string()
}

/// 词表里有几个词（与 [`super::parse_word_list`] 同一口径）。
///
/// 界面上的「N 条」必须等于粗转真正会召回的词条数，所以这里**转发**而不是重写。
pub fn count_group_words(words: &str) -> usize {
    count_words(words)
}

/// 一次运行真正要用到的热词：**已经解析好的**词表文本 + 两个调参。
///
/// 之所以把 id 换成解析结果再往下传：管线只认这个结构，不再回头查组表。删掉一个
/// 已被任务绑定的组之后，`resolve_run` 返回 `None`，管线因此拿不到悬空 id——
/// 也就不会出现「任务行写着「无热词」、开跑却报「粗转模型缺失」」这种自相矛盾。
///
/// 词表为空时**仍然返回 `Some`**（`text` 为空串）：这样
/// [`crate::settings::Settings::can_start`] 还能分清「选了个空组」（该报错）和
/// 「压根没选组」（该安静跑），前者比「选了却什么都没发生」好得多。
#[derive(Debug, Clone, PartialEq)]
pub struct HotwordRun {
    /// 组 id（报错与日志用；管线不据此查找）。
    pub group_id: String,
    /// 组显示名（报错文案用）。
    pub group_name: String,
    /// 词表原文（逗号 / 换行分隔）。为空 = 这组还没填词。
    pub text: String,
    /// 每段最多注入多少条。
    pub top_k: usize,
    /// 粗筛分数下限。
    pub threshold: f64,
}

impl HotwordRun {
    /// 词表真的有词（值得跑一趟粗转）。
    pub fn is_usable(&self) -> bool {
        !self.text.trim().is_empty()
    }

    /// 解析后的词条数（界面计数与日志用）。
    pub fn word_count(&self) -> usize {
        count_words(&self.text)
    }
}

/// 热词库：词组 + 默认组 + 调参，自己落盘到 `hotword.json`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HotwordStore {
    /// 至少一个（由 [`Self::normalize`] 保证）。
    #[serde(default)]
    pub groups: Vec<HotwordGroup>,
    /// **新任务**默认绑定的组 id（`None` = 没标默认组）。
    ///
    /// 注意这只是「哪一组是默认组」，新任务到底用不用它由
    /// [`Self::default_group_enabled`] 决定——区别见那个字段。
    #[serde(default)]
    pub default_group: Option<String>,
    /// 「新任务默认用热词」开关。
    ///
    /// **只决定新加入的任务**默认绑不绑 [`Self::default_group`]：关掉 = 新任务是
    /// 「无热词」，但**记住**默认组的选择，重新打开接着用。已有任务、以及每个任务
    /// 自己的选择都不受影响（任务行上仍可单独改）。
    #[serde(default)]
    pub default_group_enabled: bool,
    /// 每段最多注入多少条热词。
    #[serde(default = "default_top_k")]
    pub top_k: usize,
    /// 召回分数下限（`0.0~=1.0`）。
    #[serde(default = "default_threshold")]
    pub threshold: f64,
}

fn default_top_k() -> usize {
    DEFAULT_TOP_K
}

fn default_threshold() -> f64 {
    DEFAULT_THRESHOLD
}

impl Default for HotwordStore {
    fn default() -> Self {
        Self {
            groups: vec![HotwordGroup::default()],
            default_group: None,
            // 全新安装：还没标默认组，开关也默认关着（打开时会顺手把当前组标为默认）。
            default_group_enabled: false,
            top_k: default_top_k(),
            threshold: default_threshold(),
        }
    }
}

impl Default for HotwordGroup {
    /// 那个空的「默认组」。
    fn default() -> Self {
        Self {
            id: DEFAULT_HOTWORD_GROUP_ID.to_string(),
            name: String::new(),
            words: String::new(),
        }
    }
}

impl HotwordGroup {
    /// 同 [`Default`]，但显式一点：读作「补一个占位组」而不是「造一个随机组」。
    pub fn placeholder() -> Self {
        Self::default()
    }
}

impl HotwordStore {
    // ── 落点 ──────────────────────────────────────────────────────────

    /// `hotword.json` 的**写**路径（数据目录里）。解析不出可写数据目录时 `None`。
    pub fn config_path() -> Option<PathBuf> {
        crate::paths::data_root()
            .is_ok()
            .then(crate::paths::hotwords_path)
    }

    /// 读 `hotword.json`；读不到就落一份带一个空组的默认库。
    ///
    /// 单次读取失败（文件坏 / 读不了）**不**静默丢数据：和 `settings.json` 一样
    /// 走 `normalize` 的自愈，并把原因带进 [`HotwordLoadReport`] 供日志说明。
    pub fn load() -> (Self, HotwordLoadReport) {
        let mut report = HotwordLoadReport {
            config_path: Self::config_path(),
            ..HotwordLoadReport::default()
        };
        // 先看开发期留在 settings.json 里的老词表。搬成功就直接用**内存里的那份**
        // 返回——不回头去读刚写的文件：写失败（数据目录只读、磁盘满）时那条读回来
        // 是 NotFound，会一路掉进「落默认库」的分支，把用户刚攒的词表静默抹掉。
        if let Some(store) = Self::take_legacy(&mut report) {
            let mut store = store;
            store.normalize(&mut report.repairs);
            // 落盘失败只影响「下次启动还要再搬一次」，不影响本次运行。
            if let Err(e) = store.save() {
                report.save_error = Some(e);
            } else {
                strip_legacy_fields(&crate::paths::settings_path(), &mut report);
            }
            return (store, report);
        }
        let Some(path) = report.config_path.clone() else {
            let mut store = Self::default();
            store.normalize(&mut report.repairs);
            return (store, report);
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<HotwordStore>(&text) {
                Ok(mut store) => {
                    store.normalize(&mut report.repairs);
                    (store, report)
                }
                Err(e) => {
                    report.parse_error = Some(e.to_string());
                    let mut store = Self::default();
                    store.normalize(&mut report.repairs);
                    (store, report)
                }
            },
            // 缺失不是错误：这是第一次用热词。
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let mut store = Self::default();
                store.normalize(&mut report.repairs);
                (store, report)
            }
            Err(e) => {
                report.read_error = Some(e.to_string());
                let mut store = Self::default();
                store.normalize(&mut report.repairs);
                (store, report)
            }
        }
    }

    /// 开发期热词曾经寄存在 `settings.json` 里。`hotword.json` 还不存在、而老
    /// 设置里确实有词表时，把它们**读出来**（不落盘——落盘由 [`Self::load`] 统一做，
    /// 这样写失败也不会把已经读到的词表弄丢）。
    fn take_legacy(report: &mut HotwordLoadReport) -> Option<Self> {
        let target = Self::config_path()?;
        if target.exists() {
            return None;
        }
        let text = std::fs::read_to_string(crate::paths::settings_path()).ok()?;
        let store = Self::from_legacy_settings_json(&text)?;
        report.migrated_from_settings = true;
        Some(store)
    }

    /// 老 `settings.json` 文本 → 热词库。纯函数（读文件那部分在调用方）。
    ///
    /// 只认「有组」这一种形状：`hotword_groups` 缺失或为空都返回 `None`，
    /// 不为一个从没开过热词的用户凭空造一次迁移。两个调参字段是搭车读的，
    /// 缺失就取默认值。
    pub fn from_legacy_settings_json(text: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(text).ok()?;
        let legacy_groups = value.get("hotword_groups")?.as_array()?;
        if legacy_groups.is_empty() {
            return None;
        }
        Some(Self {
            groups: serde_json::from_value(serde_json::Value::Array(legacy_groups.clone()))
                .unwrap_or_default(),
            default_group: value
                .get("hotword_default_group")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            // 老配置里「有个默认组」就等于「新任务会用它」（那时还没有开关），迁移
            // 时把开关一并打开：行为不变，用户想关随时关。
            default_group_enabled: value
                .get("hotword_default_group")
                .and_then(|v| v.as_str())
                .is_some(),
            top_k: value
                .get("hotword_top_k")
                .and_then(serde_json::Value::as_u64)
                .map_or(DEFAULT_TOP_K, |n| n as usize),
            threshold: value
                .get("hotword_threshold")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(DEFAULT_THRESHOLD),
        })
    }

    /// 归一化后写回 `hotword.json`。
    pub fn save(&mut self) -> Result<PathBuf, String> {
        self.normalize_notes();
        let path = Self::config_path().ok_or_else(|| {
            let reason = crate::paths::data_root()
                .as_ref()
                .err()
                .map(|e| e.message())
                .unwrap_or("data directory unavailable");
            crate::i18n::settings_no_writable_dir(reason)
        })?;
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        crate::media::write_atomic(&path, text).map_err(|e| e.to_string())?;
        Ok(path)
    }

    // ── 自愈 ──────────────────────────────────────────────────────────

    fn normalize(&mut self, notes: &mut Vec<String>) {
        if self.groups.is_empty() {
            self.groups.push(HotwordGroup::placeholder());
            notes.push("no hotword groups -> created the default one".into());
        }
        dedupe_ids(&mut self.groups, notes);
        let exists = self
            .groups
            .iter()
            .any(|g| Some(g.id.as_str()) == self.default_group.as_deref());
        if self.default_group.is_some() && !exists {
            self.default_group = None;
            notes.push("hotword default group no longer exists -> none".into());
        }
        // 不变式：**开着开关 ⇒ 一定有一个默认组**。默认组被删掉之后还开着开关，
        // 只会得到「开关开着、却没有任何组会被绑定」这种自相矛盾的状态（用户看到
        // 的是开关亮着但新任务仍是「无」），所以这里把开关一起关掉；用户重新打开
        // 时会顺手把当前组标为默认（见 `set_default_group_enabled`）。
        if self.default_group_enabled && self.default_group.is_none() {
            self.default_group_enabled = false;
            notes.push("hotword default group gone -> new tasks won't use hotwords".into());
        }
        self.top_k = self.top_k.clamp(1, 100);
        if !self.threshold.is_finite() {
            self.threshold = DEFAULT_THRESHOLD;
            notes.push("hotword threshold was not a number -> 0.85".into());
        }
        self.threshold = self.threshold.clamp(0.0, 1.0);
    }

    fn normalize_notes(&mut self) {
        let mut notes = Vec::new();
        self.normalize(&mut notes);
    }

    // ── 查询 ──────────────────────────────────────────────────────────

    pub fn group(&self, id: &str) -> Option<&HotwordGroup> {
        self.groups.iter().find(|g| g.id == id)
    }

    /// 界面显示用的组名（没起名 → 本地化占位名，见 [`display_name`]）；组不存在时
    /// `None`。
    pub fn label(&self, id: &str, lang: UiLang) -> Option<String> {
        self.group(id).map(|g| display_name(g, lang))
    }

    /// 下拉选项：`(稳定 id, 显示名)`。名字跟着改名实时变，id 不变。
    pub fn menu_options(&self, lang: UiLang) -> Vec<(String, String)> {
        self.groups
            .iter()
            .map(|g| (g.id.clone(), display_name(g, lang)))
            .collect()
    }

    /// 词条数（组不存在时为 0）。
    pub fn word_count(&self, id: &str) -> usize {
        self.group(id).map_or(0, |g| count_words(&g.words))
    }

    /// 哪一组被标成了「默认组」（不看开关；界面用它决定 chip 上的「默认」标记、
    /// 以及打开热词页时先编辑哪一组）。
    pub fn default_group_id(&self) -> Option<String> {
        self.default_group.clone()
    }

    /// **新加入的任务**该绑哪一组：开关关着就是 `None`（无热词）。
    pub fn new_task_group(&self) -> Option<String> {
        self.default_group_enabled
            .then(|| self.default_group.clone())
            .flatten()
    }

    /// 本次运行该带的参数：选了哪个组就给哪个组的词表 + 当前调参。
    ///
    /// 组被删了 → `None`（调用方据此**不跑粗转**，也不该报错）。
    /// 组还在但没词 → `Some` 且 `text` 为空，由
    /// [`crate::settings::Settings::can_start`] 当错误报出来——「选了却什么都没
    /// 发生」比直接说清楚更糟。
    ///
    /// `lang` 只影响 `group_name`（报错文案里要显示的名字），组 id 与词表都跟语言无关。
    pub fn resolve_run(&self, group_id: Option<&str>, lang: UiLang) -> Option<HotwordRun> {
        let group = self.group(group_id?)?;
        Some(HotwordRun {
            group_id: group.id.clone(),
            group_name: display_name(group, lang),
            text: group.words.clone(),
            top_k: self.top_k,
            threshold: self.threshold,
        })
    }

    // ── 纯操作（都可单测） ────────────────────────────────────────────

    /// 新建一个热词组（名字留空 = 界面显示占位名），返回它的稳定 id。
    pub fn add_group(&mut self, name: &str) -> String {
        let id = self.next_free_id();
        self.groups.push(HotwordGroup {
            id: id.clone(),
            name: name.trim().to_string(),
            words: String::new(),
        });
        id
    }

    /// 删除一个热词组。删的是默认组时把默认清成 `None`（不替用户另挑）。
    pub fn remove_group(&mut self, id: &str) {
        self.groups.retain(|g| g.id != id);
        self.normalize_notes();
    }

    /// 改组名。**不动 id**——任务绑的是 id，所以改名后下拉项跟着变、绑定不散。
    ///
    /// 名字原样存（不 trim）：面板里正在输入、末尾带空格的中间态不该被抹掉；
    /// 显示时由 [`display_name`] 处理「全空白 = 占位名」。
    pub fn rename_group(&mut self, id: &str, name: &str) {
        if let Some(group) = self.groups.iter_mut().find(|g| g.id == id) {
            group.name = name.to_string();
        }
    }

    /// 换掉一个组的词表（原样存，格式转换只发生在解析那一步）。
    pub fn set_group_words(&mut self, id: &str, words: &str) {
        if let Some(group) = self.groups.iter_mut().find(|g| g.id == id) {
            group.words = words.to_string();
        }
    }

    /// 指定默认组；`None` = 没有默认组（开关随之关掉，见 [`Self::normalize`] 的
    /// 不变式）。
    ///
    /// 指向不存在的组是空操作——调用方通常来自界面上一份可能已过期的列表，
    /// 静默忽略比写进一个悬空 id 更安全。
    pub fn set_default_group(&mut self, id: Option<&str>) {
        match id {
            Some(id) if self.groups.iter().any(|g| g.id == id) => {
                self.default_group = Some(id.to_string());
            }
            Some(_) => {}
            None => {
                self.default_group = None;
                self.default_group_enabled = false;
            }
        }
    }

    /// 开关「新任务默认用热词」。
    ///
    /// 打开时若还没有默认组，就把 `fallback`（界面上正在编辑的那一组）标为默认：
    /// 否则开关打开却没有任何组会被绑定，等于没打开。`fallback` 不存在（列表过期）
    /// 就只是打开开关——`normalize` 的不变式会把这种没有默认组的空转状态关回去。
    pub fn set_default_group_enabled(&mut self, enabled: bool, fallback: Option<&str>) {
        self.default_group_enabled = enabled;
        if enabled && self.default_group.is_none() {
            self.default_group = fallback
                .filter(|id| self.groups.iter().any(|g| g.id == *id))
                .map(str::to_string);
        }
        let mut notes = Vec::new();
        self.normalize(&mut notes);
    }

    /// 下一个没被占用的 `g{n}`。
    ///
    /// **纯函数，只依赖已有的组**——这正是它比「进程级自增计数器」强的地方：
    /// 计数器重启就归 1，必然与上次落盘的 `g1` 撞号；而这里永远从现存 id 的最大
    /// 后缀往上找，跨进程、跨版本都唯一。
    fn next_free_id(&self) -> String {
        let mut next = self
            .groups
            .iter()
            .filter_map(|g| g.id.strip_prefix('g'))
            .filter_map(|rest| rest.parse::<u64>().ok())
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        loop {
            let candidate = format!("g{next}");
            if !self.groups.iter().any(|g| g.id == candidate) {
                return candidate;
            }
            next = next.saturating_add(1);
        }
    }
}

/// 把重复的组 id 改掉，后来的那些换个新号。
///
/// 已经写坏的文件（开发期进程级计数器撞号）靠这里修回来：id 重复时查找一律
/// `find` 命中第一个，于是删一组会连带删掉同号的那组、编辑 A 写进 B、召回用的是
/// A 的词。第一个保留原号，后来者换号——顺序即用户看到的顺序，改动最小。
fn dedupe_ids(groups: &mut [HotwordGroup], notes: &mut Vec<String>) {
    let mut seen: Vec<String> = Vec::new();
    let mut repaired = 0usize;
    for group in groups.iter_mut() {
        if !seen.contains(&group.id) {
            seen.push(group.id.clone());
            continue;
        }
        let mut next = 1u64;
        let id = loop {
            let candidate = format!("g{next}");
            if !seen.contains(&candidate) {
                break candidate;
            }
            next += 1;
        };
        repaired += 1;
        group.id = id.clone();
        seen.push(id);
    }
    if repaired > 0 {
        notes.push(format!(
            "duplicate hotword group ids -> renumbered {repaired}"
        ));
    }
}

/// 把老 `settings.json` 里的热词字段删掉（迁移后调用）。
///
/// 只在**成功写完** `hotword.json` 之后做：写失败就保持原样，下次启动还能再搬一次。
/// 读失败一律静默——这是清理动作，不该因为它把用户挡在门外。
fn strip_legacy_fields(settings_path: &Path, report: &mut HotwordLoadReport) {
    let Ok(text) = std::fs::read_to_string(settings_path) else {
        return;
    };
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return;
    };
    let Some(obj) = value.as_object_mut() else {
        return;
    };
    let mut removed = false;
    for key in [
        "hotword_groups",
        "hotword_default_group",
        "hotword_top_k",
        "hotword_threshold",
    ] {
        removed |= obj.remove(key).is_some();
    }
    if !removed {
        return;
    }
    match serde_json::to_string_pretty(&value) {
        Ok(pretty) => {
            if crate::media::write_atomic(settings_path, pretty).is_err() {
                report.migrated_from_settings = false;
            }
        }
        Err(_) => report.migrated_from_settings = false,
    }
}

/// 为什么读到的不是磁盘上的原样：坏在哪、被修好了什么。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HotwordLoadReport {
    /// 实际读到的文件（`None` = 没有 `hotword.json`，落的默认库）。
    pub config_path: Option<PathBuf>,
    /// 自愈动作说明。
    pub repairs: Vec<String>,
    pub parse_error: Option<String>,
    pub read_error: Option<String>,
    /// 迁移到 `hotword.json` 时落盘失败（词还在内存里，下次启动会再搬一次）。
    pub save_error: Option<String>,
    /// 本次是否从老 `settings.json` 搬过来的。
    pub migrated_from_settings: bool,
}

impl HotwordLoadReport {
    /// 有值得进日志的事吗（迁移 / 读失败 / 解析失败 / 落盘失败 / 自愈）。
    ///
    /// 判据与结构体住在一起，调用方只问这一句。写在调用方（把字段一个个
    /// `is_some() ||` 起来）必然漏——`save_error` 就是这么漏的：「迁移写完盘
    /// 失败、下次启动还得再搬一次」这件事，日志里完全看不见。新增字段只改这里。
    pub fn is_notable(&self) -> bool {
        self.migrated_from_settings
            || self.parse_error.is_some()
            || self.read_error.is_some()
            || self.save_error.is_some()
            || !self.repairs.is_empty()
    }

    /// 一行可贴进日志的摘要。
    pub fn summary(&self) -> String {
        let path = self
            .config_path
            .as_ref()
            .map_or_else(|| "<none>".to_string(), |p| p.display().to_string());
        let mut parts = vec![format!("hotword.json: {path}")];
        if self.migrated_from_settings {
            parts.push("migrated from settings.json".to_string());
        }
        if let Some(e) = &self.parse_error {
            parts.push(format!("parse error: {e}"));
        }
        if let Some(e) = &self.read_error {
            parts.push(format!("read error: {e}"));
        }
        if let Some(e) = &self.save_error {
            parts.push(format!("save error: {e}"));
        }
        if !self.repairs.is_empty() {
            parts.push(format!("repairs: {}", self.repairs.join("; ")));
        }
        parts.join(" | ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(id: &str, name: &str, words: &str) -> HotwordGroup {
        HotwordGroup {
            id: id.into(),
            name: name.into(),
            words: words.into(),
        }
    }

    // ── 老 settings.json 迁移 ─────────────────────────────────────────

    /// 开发期住在 `settings.json` 里的词表要**一个字不丢**地搬过来。
    #[test]
    fn legacy_hotwords_in_settings_json_migrate_whole() {
        let legacy = r#"{
            "language": "zh",
            "hotword_groups": [
                {"id": "default", "name": "产品", "words": "OneAsr\n模型"},
                {"id": "g1", "name": "", "words": "窦神归来"}
            ],
            "hotword_default_group": "g1",
            "hotword_top_k": 12,
            "hotword_threshold": 0.7
        }"#;
        let mut store =
            HotwordStore::from_legacy_settings_json(legacy).expect("应当识别出待迁移的词表");
        store.normalize_notes();
        assert_eq!(store.groups.len(), 2);
        assert_eq!(store.groups[0].name, "产品");
        assert_eq!(store.groups[0].words, "OneAsr\n模型");
        assert_eq!(store.word_count("default"), 2);
        assert_eq!(store.default_group.as_deref(), Some("g1"));
        assert!(
            store.default_group_enabled,
            "老配置里有默认组 = 新任务会用它，迁移时开关要一并打开"
        );
        assert_eq!(store.top_k, 12);
        assert_eq!(store.threshold, 0.7);
    }

    /// 没开过热词的老配置不该被当成「待迁移」——否则每个老用户首启都会多写一个
    /// `hotword.json`，只为了存一个空的默认组。
    #[test]
    fn settings_without_hotwords_is_not_migrated() {
        assert!(HotwordStore::from_legacy_settings_json(r#"{"language":"zh"}"#).is_none());
        assert!(HotwordStore::from_legacy_settings_json(r#"{"hotword_groups":[]}"#).is_none());
        assert!(HotwordStore::from_legacy_settings_json("not json").is_none());
    }

    /// 只带组、不带两个调参字段时，调参取默认值（而不是丢整个迁移）。
    #[test]
    fn legacy_migration_falls_back_to_default_knobs() {
        let store = HotwordStore::from_legacy_settings_json(
            r#"{"hotword_groups":[{"id":"default","name":"甲","words":"甲词"}]}"#,
        )
        .expect("只有组也该迁移");
        assert_eq!(store.top_k, DEFAULT_TOP_K);
        assert_eq!(store.threshold, DEFAULT_THRESHOLD);
        assert_eq!(store.default_group, None);
    }

    // ── 报告：哪些事值得进日志 ────────────────────────────────────────

    /// 「值得进日志」的判据必须覆盖**每一种**异常，一种都不能漏。
    ///
    /// 断骨验证：把 [`HotwordLoadReport::is_notable`] 里的 `save_error` 去掉，
    /// 最后一条立刻变红。它曾经就是这样漏的——判据写在调用方，字段一个个
    /// `is_some() ||` 起来，加字段时漏掉了「迁移写完盘失败」。
    #[test]
    fn every_load_event_is_notable() {
        assert!(
            !HotwordLoadReport::default().is_notable(),
            "干净读入不该往日志里灌东西"
        );

        // 给报告打上一种「值得进日志」的标记。
        type Mark = fn(&mut HotwordLoadReport);
        let cases: [(&str, Mark); 5] = [
            ("从老 settings.json 迁移", |r| {
                r.migrated_from_settings = true
            }),
            ("解析失败", |r| r.parse_error = Some("bad json".into())),
            ("读失败", |r| {
                r.read_error = Some("permission denied".into())
            }),
            ("迁移落盘失败", |r| {
                r.save_error = Some("disk full".into())
            }),
            ("自愈过一次", |r| {
                r.repairs.push("renumbered g1".into())
            }),
        ];
        for (what, mark) in cases {
            let mut report = HotwordLoadReport::default();
            mark(&mut report);
            assert!(report.is_notable(), "{what} 必须进日志：{report:?}");
        }
    }

    /// 摘要是一行贴进日志的东西：路径要有，每一种事也要有。
    #[test]
    fn summary_carries_the_path_and_every_event() {
        let report = HotwordLoadReport {
            config_path: Some(PathBuf::from("C:/data/hotword.json")),
            migrated_from_settings: true,
            parse_error: Some("bad json".into()),
            read_error: Some("permission denied".into()),
            save_error: Some("disk full".into()),
            repairs: vec!["renumbered g1".into()],
        };
        let line = report.summary();
        for needle in [
            "hotword.json",
            "migrated from settings.json",
            "parse error",
            "read error",
            "save error",
            "renumbered g1",
        ] {
            assert!(line.contains(needle), "摘要漏了 {needle:?}：{line}");
        }
    }

    // ── id 分配（断骨锚点：换成进程级计数器，本组测试立刻变红）──────────

    /// 连续新建的组必须互不相同，且都撞不上保留 id。
    #[test]
    fn added_groups_get_distinct_ids() {
        let mut s = HotwordStore::default();
        let a = s.add_group("甲");
        let b = s.add_group("乙");
        assert_ne!(a, b);
        assert_ne!(a, DEFAULT_HOTWORD_GROUP_ID);
        assert_ne!(b, DEFAULT_HOTWORD_GROUP_ID);
        assert_eq!(s.groups.len(), 3);
        assert_eq!(s.groups[1].name, "甲");
        assert!(s.groups[1].words.is_empty(), "新组不带词");
    }

    /// **跨重启**：新进程里没有状态，但**落盘的旧库里已经有 `g1`**。此时新建的
    /// 组必须让开它——这正是「进程级计数器重启归 1」会撞上的那一个。
    ///
    /// 断骨验证（实测）：把 [`HotwordStore::next_free_id`] 换回原来的
    /// 「进程级 static 自增、从 1 开始、无成员检查」后，本测试失败——断言拿到
    /// 的不是 `g2`。断言写成**确切 id**而不是「不等于 g1」是有意的：进程级计数器
    /// 在同一次测试运行里会被前面的用例推着走，只断言「不相等」会时灵时不灵，
    /// 而真重启场景下它必然撞号——这正是它当初能溜过 CI 的原因。
    #[test]
    fn a_new_id_never_collides_with_ids_already_persisted() {
        let mut reloaded = HotwordStore {
            groups: vec![
                group(DEFAULT_HOTWORD_GROUP_ID, "", ""),
                group("g1", "上次建的", "甲"),
            ],
            ..HotwordStore::default()
        };
        let fresh = reloaded.add_group("这次建的");
        assert_eq!(
            fresh, "g2",
            "重启后新建的组撞上了已落盘的 id：{:?}",
            reloaded.groups
        );
        assert_eq!(reloaded.groups.len(), 3);
        assert_eq!(reloaded.groups[1].name, "上次建的", "旧组没被动过");
        assert_eq!(reloaded.groups[2].name, "这次建的");
    }

    /// id 分配只看现存组，与它们的名字 / 词表无关；跳号是允许的（不回收）。
    #[test]
    fn next_id_is_derived_from_the_existing_max_suffix() {
        let mut s = HotwordStore {
            groups: vec![
                group(DEFAULT_HOTWORD_GROUP_ID, "", ""),
                group("g4", "", ""),
                group("g9", "", ""),
                group("custom", "", ""), // 非 g 前缀：不该干扰计数
            ],
            ..HotwordStore::default()
        };
        assert_eq!(s.add_group(""), "g10");
        assert_eq!(s.add_group(""), "g11");
    }

    // ── 自愈 ──────────────────────────────────────────────────────────

    /// 删到空列表不是合法状态：补回一个默认组。
    #[test]
    fn removing_every_group_leaves_exactly_one_default_group() {
        let mut s = HotwordStore::default();
        s.remove_group(DEFAULT_HOTWORD_GROUP_ID);
        assert_eq!(s.groups.len(), 1);
        assert_eq!(s.groups[0].id, DEFAULT_HOTWORD_GROUP_ID);
    }

    /// 删掉默认组时把默认清成 `None`，不替用户另挑一个。
    #[test]
    fn removing_the_default_group_clears_the_default() {
        let mut s = HotwordStore::default();
        let id = s.add_group("甲");
        s.set_default_group(Some(&id));
        s.remove_group(&id);
        assert_eq!(s.default_group, None);
        assert!(!s.groups.iter().any(|g| g.id == id));
    }

    /// 有效的默认组不许被误伤。
    #[test]
    fn normalize_keeps_a_default_group_that_still_exists() {
        let mut s = HotwordStore::default();
        let id = s.add_group("甲");
        s.set_default_group(Some(&id));
        s.top_k = 5;
        s.normalize_notes();
        assert_eq!(
            s.default_group.as_deref(),
            Some(id.as_str()),
            "清理悬空默认的判据必须是「这个 id 还在不在」，不是无条件清空"
        );
        assert_eq!(s.top_k, 5, "顺带夹一下参数不该动别的");
    }

    /// 调参夹到合法区间；NaN 阈值回落默认值。
    #[test]
    fn normalize_clamps_tuning_knobs() {
        let mut s = HotwordStore {
            top_k: 0,
            threshold: f64::NAN,
            ..HotwordStore::default()
        };
        s.normalize_notes();
        assert_eq!(s.top_k, 1);
        assert_eq!(s.threshold, DEFAULT_THRESHOLD);

        let mut s = HotwordStore {
            top_k: 9999,
            threshold: 4.2,
            ..HotwordStore::default()
        };
        s.normalize_notes();
        assert_eq!(s.top_k, 100);
        assert_eq!(s.threshold, 1.0);
    }

    /// 已经写坏的文件（重复 id）必须被修回来——否则查找一律命中第一个，
    /// 删一组会连带删掉同号的那组。
    ///
    /// 断骨验证：去掉 [`dedupe_ids`] 调用后本测试失败（两组的 id 仍是 `g1`）。
    #[test]
    fn normalize_renumbers_duplicate_ids_and_keeps_the_first() {
        let mut s = HotwordStore {
            groups: vec![
                group(DEFAULT_HOTWORD_GROUP_ID, "", ""),
                group("g1", "甲", "甲词"),
                group("g1", "乙", "乙词"),
            ],
            ..HotwordStore::default()
        };
        s.normalize_notes();
        let ids: Vec<&str> = s.groups.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(ids, ["default", "g1", "g2"], "后出现的那个被换号");
        assert_eq!(s.groups[1].name, "甲", "第一个保留原号");
        assert_eq!(s.groups[2].name, "乙");
        // 换号之后两个组必须能被分别找到，且各自的词表没串。
        assert_eq!(s.group("g1").map(|g| g.words.as_str()), Some("甲词"));
        assert_eq!(s.group("g2").map(|g| g.words.as_str()), Some("乙词"));
    }

    // ── 运行期解析 ────────────────────────────────────────────────────

    /// 选了组 → 带出词表 + 调参 + 组名。
    #[test]
    fn resolve_run_carries_the_words_and_the_knobs() {
        let mut s = HotwordStore::default();
        let id = s.add_group("甲");
        s.set_group_words(&id, "窦神归来\n柳宗元");
        s.top_k = 7;
        s.threshold = 0.42;
        let run = s
            .resolve_run(Some(&id), UiLang::Zh)
            .expect("选中的组必须解析出参数");
        assert_eq!(run.group_id, id);
        assert_eq!(run.group_name, "甲");
        assert_eq!(run.text, "窦神归来\n柳宗元");
        assert_eq!(run.top_k, 7);
        assert_eq!(run.threshold, 0.42);
        assert_eq!(run.word_count(), 2);
        assert!(run.is_usable());
    }

    /// 选了一个**已被删除**的组：解析不出来，且**不报错**。
    ///
    /// 这是修复前那条「任务行显示『无』、开跑却报『粗转模型缺失』」的根：
    /// 管线侧现在只可能看到 `None`（不跑）或一个真的 [`HotwordRun`]，
    /// 拿不到悬空 id，也就不会再去要求粗转模型。
    #[test]
    fn a_deleted_group_resolves_to_nothing() {
        let mut s = HotwordStore::default();
        let id = s.add_group("甲");
        s.set_group_words(&id, "甲词");
        s.remove_group(&id);
        assert_eq!(s.resolve_run(Some(&id), UiLang::Zh), None);
    }

    /// 组还在但没词：**仍然返回 `Some`**，只是不可用——留给 `can_start` 报错。
    /// 与「压根没选组」（`None`）必须能区分开。
    #[test]
    fn an_empty_group_resolves_to_an_unusable_run_not_to_none() {
        let mut s = HotwordStore::default();
        let id = s.add_group("空组");
        let run = s
            .resolve_run(Some(&id), UiLang::Zh)
            .expect("选中的组要解析出来");
        assert!(!run.is_usable(), "空组不能被当成「可用」");
        assert_eq!(run.group_name, "空组");
        assert_eq!(run.word_count(), 0);
        assert_eq!(s.resolve_run(None, UiLang::Zh), None, "没选组才是 None");
    }

    // ── 「新任务默认用热词」开关 ────────────────────────────────────────

    /// 开关只管「**新加入的任务**默认绑谁」：关掉不丢默认组的选择，重新打开接着用；
    /// 打开时若还没标默认组，就把 fallback（界面上正在编辑的那一组）标上。
    ///
    /// 断骨验证：让 `new_task_group` 无视开关直接返回 `default_group`，第三条断言
    /// 立刻变红（关掉开关后新任务仍然会被绑上组）。
    #[test]
    fn the_switch_only_decides_what_new_tasks_bind() {
        let mut s = HotwordStore::default();
        let id = s.add_group("甲");
        s.set_group_words(&id, "甲词");

        // 全新安装：开关关着 → 新任务「无热词」
        assert!(!s.default_group_enabled);
        assert_eq!(s.new_task_group(), None);

        // 打开：顺手把当前这一组标为默认，于是新任务立刻用上它
        s.set_default_group_enabled(true, Some(&id));
        assert_eq!(s.default_group.as_deref(), Some(id.as_str()));
        assert_eq!(s.new_task_group().as_deref(), Some(id.as_str()));

        // 关掉：新任务回到「无热词」，但默认组的选择留着
        s.set_default_group_enabled(false, None);
        assert_eq!(s.new_task_group(), None);
        assert_eq!(
            s.default_group.as_deref(),
            Some(id.as_str()),
            "关开关不等于丢掉默认组"
        );

        // 重新打开：接着用原来那一组，不用重新标记
        s.set_default_group_enabled(true, None);
        assert_eq!(s.new_task_group().as_deref(), Some(id.as_str()));
    }

    /// 默认组没了（被删 / 被取消标记）时开关自己关掉：不留「开关亮着、却没有任何
    /// 组会被绑定」的自相矛盾状态。
    #[test]
    fn the_switch_cannot_stay_on_without_a_default_group() {
        let mut s = HotwordStore::default();
        let id = s.add_group("甲");
        s.set_default_group_enabled(true, Some(&id));

        // ① 删掉默认组
        s.remove_group(&id);
        assert!(!s.default_group_enabled, "默认组被删，开关必须跟着关");
        assert_eq!(s.new_task_group(), None);

        // ② 取消默认标记（chip 上那颗「默认」）
        let other = s.add_group("乙");
        s.set_default_group_enabled(true, Some(&other));
        s.set_default_group(None);
        assert!(!s.default_group_enabled);
        assert_eq!(s.new_task_group(), None);
    }

    /// 打开开关时给的 fallback 必须是真实存在的组（界面列表可能已过期）：悬空 id
    /// 不写进配置，空转的开关也就自己关回去。
    #[test]
    fn enabling_with_a_stale_fallback_does_not_mark_it() {
        let mut s = HotwordStore::default();
        s.set_default_group_enabled(true, Some("nope"));
        assert_eq!(s.default_group, None);
        assert!(!s.default_group_enabled);
        assert_eq!(s.new_task_group(), None);
    }

    // ── 改名不掉绑定 / 计数口径 ───────────────────────────────────────

    /// 改名只动 `name`、不动 `id`——这就是「改名后任务绑定不掉」的依据。
    #[test]
    fn renaming_keeps_the_id_and_only_changes_the_name() {
        let mut s = HotwordStore::default();
        let id = s.add_group("旧名");
        s.rename_group(&id, "新名");
        assert_eq!(s.group(&id).map(|g| g.name.as_str()), Some("新名"));
        assert_eq!(s.group(&id).map(|g| g.id.as_str()), Some(id.as_str()));
        // 名字原样存：末尾空格这类中间态不被抹掉。
        s.rename_group(&id, "带空格 ");
        assert_eq!(s.group(&id).map(|g| g.name.as_str()), Some("带空格 "));
    }

    /// 词条计数与词表解析同口径：中英文逗号 / 换行都算分隔，空项不算。
    #[test]
    fn word_count_matches_the_parser() {
        let mut s = HotwordStore::default();
        let id = s.add_group("");
        for text in [
            "",
            "   \n\t\n",
            "甲\r\n\r\n 乙 \n",
            "a\nb\nc\n",
            "甲,乙，丙",
            ",甲,,乙,",
        ] {
            s.set_group_words(&id, text);
            assert_eq!(
                s.word_count(&id),
                super::super::parse_word_list(text).len(),
                "{text:?}"
            );
        }
        // 分隔符**只有** `,` / `，` / 换行——顿号 `、` 不算，它会留在词条里。
        // 这一条是刻意的：猜错分隔符等于静默改写用户的词表。
        assert_eq!(
            super::super::parse_word_list("甲，乙、丙"),
            ["甲", "乙、丙"]
        );
    }

    /// 下拉选项保留稳定 id、显示名跟着改名变；删掉中间一组不动别人的 id；
    /// **没起名的组显示本地化占位名，不是机器 id**。
    ///
    /// 断骨验证：把 [`display_name`] 的兜底改回 `group.id`，第一条断言立刻变红——
    /// 那正是用户看到的「组名叫 default」。
    #[test]
    fn menu_options_keep_stable_ids() {
        let mut s = HotwordStore {
            groups: vec![group("default", "", ""), group("g1", "", "")],
            default_group: None,
            default_group_enabled: false,
            top_k: DEFAULT_TOP_K,
            threshold: DEFAULT_THRESHOLD,
        };
        assert_eq!(
            s.menu_options(UiLang::Zh),
            vec![
                ("default".to_string(), "默认".to_string()),
                ("g1".to_string(), "未命名".to_string()),
            ],
            "空名给本地化占位名：保留组「默认」、用户建的组「未命名」"
        );
        assert_eq!(
            s.menu_options(UiLang::En),
            vec![
                ("default".to_string(), "Default".to_string()),
                ("g1".to_string(), "Untitled".to_string()),
            ],
            "英文界面同一套占位名"
        );
        // 起过名就照名字走，两种语言下都一样（名字是用户内容，不翻译）。
        s.rename_group("g1", "乙");
        assert_eq!(
            s.menu_options(UiLang::En)[1],
            ("g1".to_string(), "乙".to_string())
        );
    }

    /// 删掉**中间**一个组之后，剩下的组顺序不变、id 不变（任务绑定不会串到别的组上）。
    #[test]
    fn removing_a_middle_group_leaves_the_others_untouched() {
        let mut s = HotwordStore {
            groups: vec![
                group("default", "甲", ""),
                group("g1", "乙", ""),
                group("g2", "丙", ""),
            ],
            default_group: None,
            default_group_enabled: false,
            top_k: DEFAULT_TOP_K,
            threshold: DEFAULT_THRESHOLD,
        };
        s.remove_group("g1");
        let ids: Vec<String> = s
            .menu_options(UiLang::Zh)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert_eq!(ids, vec!["default", "g2"]);
        assert_eq!(s.groups[1].name, "丙");
    }

    /// 任务行那颗 chip 走 `label`：没起名的组同样给占位名，绝不显示 id；
    /// 组不存在（已被删）给 `None`——那一行显示「无热词」，与 `resolve_run` 的口径一致。
    #[test]
    fn label_uses_the_placeholder_for_unnamed_groups() {
        let s = HotwordStore::default();
        assert_eq!(
            s.label(DEFAULT_HOTWORD_GROUP_ID, UiLang::Zh).as_deref(),
            Some("默认")
        );
        assert_eq!(
            s.label(DEFAULT_HOTWORD_GROUP_ID, UiLang::En).as_deref(),
            Some("Default")
        );
        assert_eq!(s.label("g9", UiLang::Zh), None, "组不存在是 None");
    }

    /// 指向不存在的组是空操作（界面列表可能已过期）；`None` 清空默认。
    #[test]
    fn default_group_ignores_unknown_ids_and_none_clears_it() {
        let mut s = HotwordStore::default();
        let id = s.add_group("甲");
        s.set_default_group(Some("nope"));
        assert_eq!(s.default_group, None, "悬空 id 不写进配置");
        s.set_default_group(Some(&id));
        assert_eq!(s.default_group.as_deref(), Some(id.as_str()));
        s.set_default_group(None);
        assert_eq!(s.default_group, None);
    }
}

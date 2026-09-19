//! 丛雨动作词表 —— pet 前端事件面的最小结构化投影。
//!
//! pet 源码（`pet/src/app.rs`）没有单一的动作枚举，而是 Elm 风格的事件面：
//! `AppEvent::{Face(u32), ToggleDress, ToggleDiff, QuickSpeak, …}` +
//! `face_id()` 表情映射表（`["01","03","04","13","14","19","21","02","20"]`）。
//! 本模块把这一事件面蒸馏成 `#[derive(ValueEnum)]` 枚举 [`PetAction`]，
//! 供 `pet-act` 工具暴露给 Agent；执行 = 产出 [`PetAction::instruction`] 结构化
//! JSON 指令，由 pet 前端进程（或桥接宿主）转回 `AppEvent` 消费。
//!
//! **待对齐说明**：pet 的 IPC 协议尚未定稿（见 `docs/ECOSYSTEM_PET.md` P0 计划），
//! 此枚举是当前源码事件面的忠实投影；pet 侧协议落地后如有增删，以彼处为准，
//! 这里同步枚举变体即可（tests 锁定了变体清单，防静默漂移）。

use lilyco::prelude::*;

/// 丛雨可执行的动作（T0：只产出指令，不驱动真实世界）
///
/// 命名规则：`FaceXxx` 对应表情（face id 与 pet `face_id()` 表一致），
/// 其余对应 `AppEvent` 的非表情事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PetAction {
    /// 表情：默认（face 01，数字键 1）
    FaceDefault,
    /// 表情：微笑（face 02，数字键 8）
    FaceSmile,
    /// 表情：发懵（face 03，数字键 2）
    FaceConfused,
    /// 表情：惊讶（face 04，数字键 3）
    FaceSurprised,
    /// 表情：困扰（face 13，数字键 4）
    FaceTroubled,
    /// 表情：生气（face 14，数字键 5）
    FaceAngry,
    /// 表情：孩子气（face 19，数字键 6）
    FaceChildish,
    /// 表情：极度不满 A（face 20，数字键 9）
    FaceGloomy,
    /// 表情：极度不满 B（face 21，数字键 7）
    FaceFurious,
    /// 切换服装：私服 ↔ 洋装（D 键）
    ToggleDress,
    /// 切换姿势差分 1 ↔ 2（Space 键）
    ToggleDiff,
    /// 快速说一句（E 键 / 点击同款：轮播语音库 + 气泡）
    QuickSpeak,
}

impl PetAction {
    /// ValueEnum 规范名（snake_case，与 `variants()` 输出一致），
    /// 用于结果 JSON 与遥测上报
    pub fn name(self) -> &'static str {
        match self {
            PetAction::FaceDefault => "face_default",
            PetAction::FaceSmile => "face_smile",
            PetAction::FaceConfused => "face_confused",
            PetAction::FaceSurprised => "face_surprised",
            PetAction::FaceTroubled => "face_troubled",
            PetAction::FaceAngry => "face_angry",
            PetAction::FaceChildish => "face_childish",
            PetAction::FaceGloomy => "face_gloomy",
            PetAction::FaceFurious => "face_furious",
            PetAction::ToggleDress => "toggle_dress",
            PetAction::ToggleDiff => "toggle_diff",
            PetAction::QuickSpeak => "quick_speak",
        }
    }

    /// 表情 id（pet `face_id()` 表的字符串形态）；非表情动作返回 `None`
    pub fn face(self) -> Option<&'static str> {
        match self {
            PetAction::FaceDefault => Some("01"),
            PetAction::FaceSmile => Some("02"),
            PetAction::FaceConfused => Some("03"),
            PetAction::FaceSurprised => Some("04"),
            PetAction::FaceTroubled => Some("13"),
            PetAction::FaceAngry => Some("14"),
            PetAction::FaceChildish => Some("19"),
            PetAction::FaceGloomy => Some("20"),
            PetAction::FaceFurious => Some("21"),
            PetAction::ToggleDress | PetAction::ToggleDiff | PetAction::QuickSpeak => None,
        }
    }

    /// 数字键序号 1..=9（pet 前端 `AppEvent::Face(i)` 的入参，`face_id(i)` 反查可对上）；
    /// 非表情动作返回 `None`
    pub fn keypad(self) -> Option<u32> {
        match self.face()? {
            "01" => Some(1),
            "02" => Some(8),
            "03" => Some(2),
            "04" => Some(3),
            "13" => Some(4),
            "14" => Some(5),
            "19" => Some(6),
            "20" => Some(9),
            "21" => Some(7),
            _ => None,
        }
    }

    /// 对应 pet 前端 `AppEvent` 变体名（桥接宿主据此分发）
    pub fn event(self) -> &'static str {
        match self {
            PetAction::ToggleDress => "ToggleDress",
            PetAction::ToggleDiff => "ToggleDiff",
            PetAction::QuickSpeak => "QuickSpeak",
            _ => "Face",
        }
    }

    /// 结构化动作指令（pet 前端进程消费的稳定 JSON 形状）
    ///
    /// ```text
    /// { "event": "Face", "face": "02", "keypad": 8, "intensity": 1.0 }
    /// ```
    pub fn instruction(self, intensity: f64) -> serde_json::Value {
        let mut v = serde_json::json!({
            "event": self.event(),
            "intensity": intensity,
        });
        if let Some(face) = self.face() {
            v["face"] = serde_json::json!(face);
        }
        if let Some(key) = self.keypad() {
            v["keypad"] = serde_json::json!(key);
        }
        v
    }
}

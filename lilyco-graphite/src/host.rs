//! GraphiteHost — headless 驱动 Graphite 编辑器内核的宿主封装
//!
//! 复刻官方测试驱动模式（graphite `editor/src/test_utils.rs`），但不依赖其
//! `#[cfg(test)]` 门控：
//! - 用公开的 `Editor::new` 构造内核（`HashMapResourceStorage` 资源存储 + 默认平台 IO）
//! - 操作面 = 消息总线：所有操作都通过 `Editor::handle_message` 派发消息完成
//! - 结果面 = FrontendMessage：内核发往前端的消息全部被收集，供工具与测试断言
//!
//! 与官方 test_utils 的两处关键差异（spike 结论的一部分）：
//! 1. `Editor::new_local_executor` 是 `pub(crate) + #[cfg(test)]`，外部不可用；
//!    这里走 `Editor::new`，节点图执行挂到全局 `NODE_RUNTIME`（本 spike 不驱动渲染，
//!    文档结构操作不依赖图求值）
//! 2. `dispatcher.message_handlers` 的各 handler 字段是 `pub(crate)`，外部拿不到
//!    `active_document()` 等内部状态；因此断言只能经由 FrontendMessage 与
//!    序列化产物（`SaveDocument` → `FrontendMessage::TriggerSaveDocument`）

use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use glam::DVec2;
use graph_craft::application_io::resource::HashMapResourceStorage;
use graph_craft::application_io::PlatformApplicationIo;
use graphene_std::raster::color::Color;
use graphite_editor::application::{Editor, Environment, Host, Platform};
use graphite_editor::messages::input_mapper::utility_types::keyboard::ModifierKeys;
use graphite_editor::messages::input_mapper::utility_types::pointer::{
    EditorPointerState, MouseKeys,
};
use graphite_editor::messages::prelude::*;

/// 单次 save 轮询的最长等待（SaveDocument 的序列化在 MessageFuture 上异步执行）
const SAVE_TIMEOUT: Duration = Duration::from_secs(5);

/// headless Graphite 内核宿主
///
/// 封装 Editor 创建 + 消息派发 + FrontendMessage 收集。
/// 每进程只能创建一个实例（内核 `ENVIRONMENT` 为 OnceLock）——
/// 多工具/多测试请共享 [`shared_host`]。
pub struct GraphiteHost {
    editor: Editor,
}

impl Default for GraphiteHost {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphiteHost {
    /// 构造内核并完成初始化（`PortfolioMessage::Init`）。
    ///
    /// uuid 种子固定为 0，保证节点 id 序列确定性（与官方测试一致）。
    pub fn new() -> Self {
        let wake: Wake = Arc::new(|| {});
        // host 字段只影响 UI 文案分支，headless 下无意义；填 Linux 以便 CI 复现
        let environment = Environment {
            platform: Platform::Desktop,
            host: Host::Linux,
        };
        let editor = Editor::new(
            environment,
            0,
            Arc::new(HashMapResourceStorage::new()),
            None, // working_copy_root：不挂工作副本容器，SaveDocument 走 legacy .graphite 字节
            PlatformApplicationIo::default(),
            wake,
        );
        let mut host = Self { editor };
        host.send(PortfolioMessage::Init);
        host
    }

    /// 向内核消息总线派发一条消息，返回内核本次发往前端的全部消息
    pub fn send<T: Into<Message>>(&mut self, message: T) -> Vec<FrontendMessage> {
        self.editor.handle_message(message)
    }

    /// 新建文档（T0）。等价于官方测试的 `EditorTestUtils::new_document`
    pub fn new_document(&mut self, name: &str) -> Vec<FrontendMessage> {
        self.send(PortfolioMessage::NewDocumentWithName {
            name: name.to_string(),
        })
    }

    /// 设置主色（primary working color）。
    ///
    /// 注意编辑器默认路由：主色喂**描边**、副色喂**填充**（见 editor 的
    /// `color_selector.rs::fill_working_color`，colors_swapped=false 时填充取副色）。
    /// 想让后续绘制形状带填充色请用 [`Self::set_fill_color`]。
    pub fn set_primary_color(&mut self, color: Color) -> Vec<FrontendMessage> {
        self.send(ToolMessage::SelectWorkingColor {
            color,
            primary: true,
        })
    }

    /// 设置填充工作色（secondary working color）——默认路由下决定后续绘制形状的填充。
    pub fn set_fill_color(&mut self, color: Color) -> Vec<FrontendMessage> {
        self.send(ToolMessage::SelectWorkingColor {
            color,
            primary: false,
        })
    }

    /// 用矩形工具画一个矩形：模拟 "移动 → 按下 → 拖动 → 抬起" 的指针消息序列，
    /// 与官方 `test_utils::drag_tool(ToolType::Rectangle, …)` 完全一致。
    /// 落笔后新图层自动处于选中态，可直接接 `set_fill`。
    pub fn draw_rectangle(&mut self, x: f64, y: f64, w: f64, h: f64) -> Vec<FrontendMessage> {
        self.send(ToolMessage::ActivateToolShapeRectangle);
        let mut responses = Vec::new();
        responses.extend(self.pointer_move(x, y, MouseKeys::empty()));
        responses.extend(self.pointer_down(x, y, MouseKeys::LEFT));
        responses.extend(self.pointer_move(x + w, y + h, MouseKeys::LEFT));
        responses.extend(self.pointer_up(x + w, y + h));
        // 结构变化等前端更新被缓冲到下一帧（FRONTEND_UPDATE_MESSAGES），
        // IncrementFrameCounter 是官方 flush 时机，headless 下手动补一帧
        responses.extend(self.send(AnimationMessage::IncrementFrameCounter));
        responses
    }

    /// 修改选中图层的填充不透明度（0.0..=1.0，越界自动钳制）。
    /// 对应 `DocumentMessage::SetFillForSelectedLayers`（T1 示范操作）
    pub fn set_fill(&mut self, fill: f64) -> Vec<FrontendMessage> {
        self.send(DocumentMessage::SetFillForSelectedLayers {
            fill: fill.clamp(0., 1.),
        })
    }

    /// 序列化当前文档，返回 (建议文件名, .graphite/.gdd 字节)。
    ///
    /// 内核侧 `SaveDocument` 会把序列化工作包成 async 消息（内建 TokioSpawner 执行），
    /// 结果经 `FutureMessageHandler` 回流进 dispatcher——headless 下轮询 `NoOp`
    /// 消息即可驱动回收，无需 GUI 事件循环。
    pub fn save_content(&mut self) -> Result<(String, Vec<u8>), String> {
        self.send(DocumentMessage::SaveDocument);
        let start = Instant::now();
        while start.elapsed() < SAVE_TIMEOUT {
            for response in self.send(Message::NoOp) {
                if let FrontendMessage::TriggerSaveDocument { name, content, .. } = response {
                    return Ok((name, content.into_vec()));
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Err(format!(
            "save 超时（{SAVE_TIMEOUT:?}）：未收到 FrontendMessage::TriggerSaveDocument"
        ))
    }

    /// 当前活动文档的根网络节点数（经序列化产物统计，格式无关内部 API）
    pub fn node_count(&mut self) -> Result<usize, String> {
        let (_, bytes) = self.save_content()?;
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| format!("序列化产物不是合法 JSON: {e}"))?;
        Ok(value
            .pointer("/network_interface/network/nodes")
            .and_then(|nodes| nodes.as_array())
            .map(|nodes| nodes.len())
            .unwrap_or(0))
    }

    // ── 指针事件（与 test_utils 的 move/mousedown/mouseup 同构） ──

    fn pointer_move(&mut self, x: f64, y: f64, mouse_keys: MouseKeys) -> Vec<FrontendMessage> {
        self.send(InputPreprocessorMessage::PointerMove {
            editor_mouse_state: EditorPointerState {
                editor_position: DVec2::new(x, y),
                mouse_keys,
                ..Default::default()
            },
            modifier_keys: ModifierKeys::default(),
        })
    }

    fn pointer_down(&mut self, x: f64, y: f64, mouse_keys: MouseKeys) -> Vec<FrontendMessage> {
        self.send(InputPreprocessorMessage::PointerDown {
            editor_mouse_state: EditorPointerState {
                editor_position: DVec2::new(x, y),
                mouse_keys,
                ..Default::default()
            },
            modifier_keys: ModifierKeys::default(),
        })
    }

    fn pointer_up(&mut self, x: f64, y: f64) -> Vec<FrontendMessage> {
        self.send(InputPreprocessorMessage::PointerUp {
            editor_mouse_state: EditorPointerState {
                editor_position: DVec2::new(x, y),
                mouse_keys: MouseKeys::empty(),
                ..Default::default()
            },
            modifier_keys: ModifierKeys::default(),
        })
    }
}

/// 进程级共享宿主。
///
/// Graphite 的 `ENVIRONMENT` 是 OnceLock（`Editor::new` 二次调用会 panic），
/// 因此同一进程内（CLI 多命令 / MCP 多工具 / 测试）必须复用同一个内核实例。
pub fn shared_host() -> &'static Mutex<GraphiteHost> {
    static SHARED_HOST: OnceLock<Mutex<GraphiteHost>> = OnceLock::new();
    SHARED_HOST.get_or_init(|| Mutex::new(GraphiteHost::new()))
}

/// 解析 hex 颜色（`#RRGGBB` 或 `#RRGGBBAA`，sRGB）为 Graphite `Color`（线性光）
pub fn parse_hex_color(hex: &str) -> Result<Color, String> {
    let s = hex.trim().trim_start_matches('#');
    let channel = |index: usize| -> Result<f32, String> {
        let bytes = s.as_bytes();
        let hi = bytes
            .get(index * 2)
            .ok_or_else(|| format!("hex 颜色长度不足: {hex}"))?;
        let lo = bytes
            .get(index * 2 + 1)
            .ok_or_else(|| format!("hex 颜色长度不足: {hex}"))?;
        let text = String::from_utf8_lossy(&[*hi, *lo]).to_string();
        let value =
            u8::from_str_radix(&text, 16).map_err(|_| format!("hex 颜色含非法字符: {hex}"))?;
        Ok(value as f32 / 255.)
    };
    match s.len() {
        6 => Ok(Color::from_gamma_srgb_channels(
            channel(0)?,
            channel(1)?,
            channel(2)?,
            1.,
        )),
        8 => Ok(Color::from_gamma_srgb_channels(
            channel(0)?,
            channel(1)?,
            channel(2)?,
            channel(3)?,
        )),
        _ => Err(format!(
            "不支持的 hex 颜色格式（需 #RRGGBB 或 #RRGGBBAA）: {hex}"
        )),
    }
}

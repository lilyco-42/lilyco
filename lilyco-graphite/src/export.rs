//! 渲染导出桥（P1 出图决定性一枪）——消息总线文档 → DynamicExecutor → 图片文件
//!
//! 复刻官方 [graphene-cli](https://github.com/GraphiteEditor/Graphite) 的
//! `main.rs` + `export.rs` 渲染初始化序列（rev ab32a604，只读参考），
//! 打通 "消息总线建图 → 序列化 → 加载 → 编译 → 图求值 → 文件落盘"：
//!
//! 1. `save_content()` 拿到 `.graphite` 字节（`load_network` 只认 legacy JSON 格式）
//! 2. `PlatformApplicationIo::new()` 建 IO（内含 WgpuExecutor，**软失败**——
//!    `WgpuExecutor::new()` 返回 `Option`，无 GPU 时为 `None`，SVG 不受影响）
//! 3. `PlatformEditorApi`（application_io + 消息发送器 + 偏好）注入 `wrap_network_in_scope`
//! 4. `Preprocessor::preprocess`（展开注入 scope；legacy 文档无资源表 → 恒 `None`）
//! 5. `Compiler::compile_single` → `ProtoNetwork` → `DynamicExecutor::new`
//! 6. `executor.execute(RenderConfig{..}.into_context())` → `TaggedValue::RenderOutput`
//!    - `ExportFormat::Svg`：`render_svg` 纯 CPU 矢量路径（`renderer.rs` 的 `SvgRender`），
//!      **零 GPU 依赖**——`try_wgpu_executor` scope 返回 `Option`，SVG 分支从不解包它
//!    - `ExportFormat::Raster`：Vello/GPU 路径，需要 `WgpuExecutor` + 设备 poll 线程，
//!      GPU→CPU 纹理回读后按 PNG 编码
//!
//! 进程约束与 [`crate::host`] 相同：无全局单例，可重复构建会话；
//! 但同一测试进程内建议串行驱动（渲染编译耗时秒级）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures::executor::block_on;
use graph_craft::application_io::{EditorPreferences, PlatformApplicationIo, PlatformEditorApi};
use graph_craft::document::value::{RenderOutputType, TaggedValue, UVec2};
use graph_craft::document::NodeNetwork;
use graph_craft::graphene_compiler::{Compiler, Executor};
use graph_craft::proto::ProtoNetwork;
use graph_craft::util::load_network;
use graphene_std::application_io::{
    ApplicationIo, ExportFormat, NodeGraphUpdateMessage, NodeGraphUpdateSender, RenderConfig,
};
use interpreted_executor::dynamic_executor::DynamicExecutor;
use interpreted_executor::util::wrap_network_in_scope;
use preprocessor::Preprocessor;

/// GPU 设备 poll 线程的轮询间隔（官方 CLI 同款：纳秒级忙轮询，保证 map_async 回调及时驱动）
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_nanos(10);

/// 静默的节点图更新发送器——官方 CLI 用 UpdateLogger 打印，这里 headless 静默
struct NoopUpdateSender;

impl NodeGraphUpdateSender for NoopUpdateSender {
    fn send(&self, _message: NodeGraphUpdateMessage) {}
}

/// 一次渲染会话：编译好的执行器 + 持活的 ApplicationIo（scope 经它拿 WgpuExecutor）
struct RenderSession {
    executor: DynamicExecutor,
    application_io: Arc<PlatformApplicationIo>,
}

/// 探测本进程能否初始化 GPU 执行器（软探测：失败只返回 false，不 panic）。
///
/// PNG 导出依赖 GPU（Vello 光栅化）；SVG 导出完全不需要。
/// CI 无独显时装 Mesa 软件栈（lavapipe/llvmpipe）即可让探测通过。
pub fn gpu_available() -> bool {
    block_on(PlatformApplicationIo::new())
        .gpu_executor()
        .is_some()
}

/// 把 `.graphite` 字节编译成可执行的 DynamicExecutor（官方 graphene-cli 最短路径）
fn build_session(document_bytes: &[u8]) -> Result<RenderSession, String> {
    let document_string =
        std::str::from_utf8(document_bytes).map_err(|e| format!("文档字节不是合法 UTF-8: {e}"))?;
    // 官方 `load_network`：从 JSON 里抠出 network_interface.network 反序列化为 NodeNetwork
    let network: NodeNetwork = load_network(document_string);

    // GPU 初始化失败是软性的（WgpuExecutor::new -> Option）：SVG 导出不依赖 GPU
    let application_io = block_on(PlatformApplicationIo::new());
    let application_io = Arc::new(application_io);

    let preferences = EditorPreferences {
        max_render_region_size: EditorPreferences::default().max_render_region_size,
    };
    let editor_api = Arc::new(PlatformEditorApi {
        application_io: Some(application_io.clone()),
        node_graph_message_sender: Box::new(NoopUpdateSender),
        editor_preferences: Box::new(preferences),
    });

    // 官方 compile_graph：包 scope（注入 editor_api）→ 预处理展开 → 编译成 proto 网络
    let mut network = wrap_network_in_scope(network, editor_api);
    // legacy `.graphite` 无资源表，与官方 CLI 同款：恒 None
    Preprocessor::new()
        .preprocess(&mut network, &|_| None)
        .map_err(|e| format!("网络预处理失败: {e:?}"))?;

    let proto: ProtoNetwork = Compiler {}
        .compile_single(network)
        .map_err(|e| format!("图编译失败: {e:?}"))?;

    // 官方 create_executor：block_on 驱动（DynamicExecutor::new 本身不需要 GPU）
    let executor = block_on(DynamicExecutor::new(proto)).map_err(|errors| {
        errors
            .iter()
            .map(|e| format!("{e:?}"))
            .collect::<Vec<_>>()
            .join("\n")
    })?;

    Ok(RenderSession {
        executor,
        application_io,
    })
}

/// 渲染 SVG，返回 SVG 字节（纯 CPU 矢量路径，headless 无 GPU 也能跑）
pub fn render_svg(document_bytes: &[u8], scale: f64) -> Result<Vec<u8>, String> {
    let session = build_session(document_bytes)?;

    let render_config = RenderConfig {
        scale,
        export_format: ExportFormat::Svg,
        for_export: true,
        ..Default::default()
    };
    let result = block_on(session.executor.execute(render_config.into_context()))
        .map_err(|e| format!("SVG 渲染执行失败: {e:?}"))?;
    let svg = match result {
        TaggedValue::RenderOutput(output) => match output.data {
            RenderOutputType::Svg { svg, .. } => svg,
            RenderOutputType::Texture(_) | RenderOutputType::Buffer { .. } => {
                return Err("请求的是 SVG 输出，内核却返回了光栅数据（不该发生）".to_string());
            }
        },
        other => return Err(format!("期望 RenderOutput，实际: {other:?}")),
    };
    Ok(svg.into_bytes())
}

/// 渲染 PNG，返回 PNG 字节（GPU/Vello 路径——无 GPU 时返回带结论的错误）
///
/// - `width`/`height`：输出像素尺寸（都给才生效，与官方 CLI 一致）
/// - `transparent`：true 保留 alpha；false 合成不透明白底（官方 CLI 同款转 RGB）
pub fn render_png(
    document_bytes: &[u8],
    scale: f64,
    width: Option<u32>,
    height: Option<u32>,
    transparent: bool,
) -> Result<Vec<u8>, String> {
    let session = build_session(document_bytes)?;

    // PNG 走 Vello 光栅化，WgpuExecutor 是硬依赖（render 节点的 Raster 分支会
    // `.expect("GPU executor not available")`）——提前探测给出可行动的错误
    let Some(wgpu_executor) = session.application_io.gpu_executor() else {
        return Err(
            "PNG 导出需要 GPU 执行器，当前环境 WgpuExecutor 初始化失败（无独显且无软件 GPU 栈）。\
             SVG 导出不依赖 GPU 可正常使用；CI/容器可安装 Mesa（mesa-vulkan-drivers/libegl1）提供软件 GPU"
                .to_string(),
        );
    };

    // 官方 CLI 同款：GPU 纹理回读的 map_async 回调需要手动 poll 设备，spawn 轮询线程
    let device = wgpu_executor.context().device.clone();
    let stop_poll = Arc::new(AtomicBool::new(false));
    let poll_handle = {
        let stop = stop_poll.clone();
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                device.poll(wgpu::PollType::Poll).unwrap();
                std::thread::sleep(POLL_INTERVAL);
            }
        })
    };

    let render_result = (|| {
        let mut render_config = RenderConfig {
            scale,
            export_format: ExportFormat::Raster,
            for_export: true,
            ..Default::default()
        };
        if let (Some(w), Some(h)) = (width, height) {
            render_config.viewport.resolution = UVec2::new(w, h);
        }
        let result = block_on(session.executor.execute(render_config.into_context()))
            .map_err(|e| format!("PNG 渲染执行失败: {e:?}"))?;
        match result {
            TaggedValue::RenderOutput(output) => match output.data {
                RenderOutputType::Texture(texture) => {
                    // GPU 纹理 → CPU 缓冲（官方 export.rs 同款转换）
                    use graphene_std::core_types::ops::Convert;
                    use graphene_std::core_types::transform::Footprint;
                    use graphene_std::raster_types::{Raster, CPU, GPU};
                    let gpu_raster = Raster::<GPU>::new_gpu(texture);
                    let cpu_raster: Raster<CPU> =
                        block_on(gpu_raster.convert(Footprint::BOUNDLESS, wgpu_executor));
                    let (data, width, height) = cpu_raster.to_flat_u8();
                    encode_png(data, width, height, transparent)
                }
                RenderOutputType::Buffer {
                    data,
                    width,
                    height,
                } => encode_png(data, width, height, transparent),
                RenderOutputType::Svg { .. } => {
                    Err("请求的是光栅输出，内核却返回了 SVG（不该发生）".to_string())
                }
            },
            other => Err(format!("期望 RenderOutput，实际: {other:?}")),
        }
    })();

    stop_poll.store(true, Ordering::Relaxed);
    let _ = poll_handle.join();
    render_result
}

/// RGBA 平面缓冲 → PNG 字节（官方 write_raster_image 的 PNG 分支）
fn encode_png(
    data: Vec<u8>,
    width: u32,
    height: u32,
    transparent: bool,
) -> Result<Vec<u8>, String> {
    use image::{ImageFormat, RgbaImage};

    let image =
        RgbaImage::from_raw(width, height, data).ok_or("图像缓冲尺寸与宽高不符".to_string())?;
    let mut cursor = std::io::Cursor::new(Vec::new());
    if transparent {
        image
            .write_to(&mut cursor, ImageFormat::Png)
            .map_err(|e| format!("PNG 编码失败: {e}"))?;
    } else {
        let opaque: image::RgbImage = image::DynamicImage::ImageRgba8(image).to_rgb8();
        opaque
            .write_to(&mut cursor, ImageFormat::Png)
            .map_err(|e| format!("PNG 编码失败: {e}"))?;
    }
    Ok(cursor.into_inner())
}

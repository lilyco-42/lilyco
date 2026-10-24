# Lilyco 商业工具路线图（2026-10-24 调研）

> 状态：技术和商业筛选，不是已发布产品或已取得客户的证明。严禁只把第三方软件改名就销售。所有生产工具必须满足：AI 原生 Registry、CLI/MCP（能扩展 TUI/Web）、用户设备执行、许可证复核、上游版本与来源记录、在正式商业照片/资源上验收。

## 0. 商业原则

Lilyco 的销售对象不是“API 调用权”，而是**能在本地完成高价值交付的工作流**：批量任务、标准化模板、多模型对比、版本管理、受控 AI 自动化、企业团队配置、专业导出、技术支持。AI 可以调用所有工具，人类也可以通过 Tool Hub GUI 使用同一注册表。

开发模式：
- Rust 为核心工具接口：`#[derive(App)]` 实现可用于 CLI/TUI/Web/MCP 的原生能力；无法轻易移植的第三方程序用**本机适配器**，不尝试无根据地移植几十万行代码。
- 每个插件只有局部权限，强制输入输出白名单/工作空间边界、超时/取消/可追踪历史；危险文件修改须人工确认。
- 用户 CPU/GPU/WebGPU/WASM 优先，模型直接在客户设备下载及运行；轻量服务端只做会员授权、软件更新、协作元数据和可选的加密同步。
- 市场需求是**假设**，收费档位必须通过实际商家访谈与留存率验证。不要根据 GitHub Stars 直接推算营业额。

## 1. 候选产品与优先级

### P0 Lilyco Commerce — 面向跨境电商的本地商品内容工厂
- 源码：现有 `lilyco-vision` / `apps/lilyco-manager/frontend`（本地 Rembg + ONNX + WebAssembly），以及已有 `ffmpeg` / HTML-to-video。
- 提供 AI 可调 `remove-background -> crop -> resize -> composite -> batch-export -> html_to_video`；SKU 模板、A/B 版本、导出标准、背景透明边缘 QA、重试清单与磁盘限额。
- 收费点：批量任务队列、商家规格模板、协作版/批次审核/品牌规范，不为基础去背本身高收费。
- 假设价格测试：个人 29–49 元/月；工作室 99–199 元/月；团队定制另询。**非已验证售价**。
- 对标需求：https://www.photoroom.com/blog/best-batch-photo-editors （竞品自己的营销研究，不是独立市场规模统计）。
- 许可证：已用 Rembg 本地实现；若引入 imgly/background-removal-js，需处理 AGPL 义务或商业许可，不自动合并到闭源客户端。

### P1 Lilyco Spine — 游戏素材/骨骼动画检验与自动化
- 源参考：https://github.com/ww-rm/SpineViewer 、https://github.com/anosu/Spine-Viewer 、https://github.com/humensmoc/spine-viewer （各自需逐项核验代码许可证）。
- 第一阶段原生 Rust `lspine` **纯 JSON/atlas 元数据检查**：版本、骨骼、slot、动画清单、约束、皮肤、Atlas 页和资源完整性。可在工具注册表免费分发；不加载或派生 Spine Runtime，也不构成完整预览器。
- 第二阶段：在取得适当授权后接入 Spine Runtime / 第三方 Viewer CLI，添加批量预览、时间轴、帧/视频导出、打包 QA、多个骨架差异报告。
- 收费点：资产批量验收、CI 集成、Unity/Godot 发布前检查、专业导出、团队审阅流水线；比通用查看器更容易卖给小型游戏工作室。
- **重要授权**：https://esotericsoftware.com/spine-editor-license 的第 2 节及 Spine Runtimes License Agreement 对集成、修改、分发等有特殊限制，需要先确保符合许可；“查看器仓库 MIT”并不会使 Spine 官方 runtime 变成 MIT。不同工具的商业再发行也要看各自依赖许可证。
- 当前本人 GitHub 可见仓库中尚未定位到命名精确为 `spine-viewer` 的自有仓库；不能声称已经移植用户私有项目代码。

### P1 Lilyco Media / lilyco-srt — 视频+字幕批量交付
- 已有 `lilyco-srt`、`lilyco-ffmpeg`、`apps/lilyco-toolhub/media_mcp.py`（Manim 和 HTML→Video）。
- 参考：https://github.com/ggml-org/whisper.cpp （MIT）；https://github.com/chidiwilliams/buzz （MIT）；https://github.com/facebookresearch/sam2 （Apache-2.0，权重再单独验证）。
- 差异化：本地转写/说话人/字幕术语表、自动社媒比例包装、逐帧商品视频和批量交付。模型、媒体和转码全部客户设备本地。
- 收费：多语字幕品牌模板、批次工作流、智能质检、多人项目交接。
- 声明：第三方语音/视频模型可能有单独的使用条件，用户上传的语音、视频素材本身也要有许可。

### P1 Lilyco Agent Tools — AI 软件原生化工具生态
- https://github.com/HKUDS/CLI-Anything (Apache-2.0)，支持 GUI 软件 CLI harness 生成、`cli-hub` 检索/安装/更新。
- 改造方向：**保留上游独立 CLI harness**；在 Lilyco 中写 `lilyco-cli-anything` 可信适配器，读取具体工具能力后生成自己的 `#[derive(App)]` 声明/JSON Schema，并接入 Tool Hub。工具执行仍应走可审核的参数列表，不允许任何从网页搜索结果下载脚本并直接执行。
- 收费：经过兼容性实测的专业工作流包、企业支持与私有适配；不要靠零修改转售开源包。
- 优先候选：Blender、Inkscape、Kdenlive、GIMP、LibreOffice。**依各自 GPL/许可证义务选择 subprocess 适配器与源码分发策略**。

### P2 Lilyco Music Tools（lyco_music / LX Music）
- 自有：https://github.com/lilyco-42/lyco_music，已有 Vue/RN 客户端和平台共享内核；LICENSE 文件引用 Apache-2.0 与 LX Music 上游。
- 参考：https://github.com/lyswhut/lx-music-desktop（源码宣称 Apache-2.0，同时 README 特别声明非商业性质）；https://github.com/beetbox/beets（MIT，曲库标签/去重/格式管理）；https://github.com/ggml-org/whisper.cpp（MIT，本机转录）。
- 可安全收费：用户合法持有的本地音频库整理、标签/封面/播放列表迁移、重复扫描、格式批量转换、离线歌词/字幕整理、可选私有端到端加密同步、用户支持。
- **重大商业风险**：不能将非授权第三方音乐播放源/VIP 地址重新售卖或声称“年费获得全部付费曲库”。商业代码若重用上游非商业限制条款组件，需获取明确法律许可并审查版权。软件 Apache 许可与音乐内容授权是两回事。
- 收费档位可测试“本地工具买断 + 云同步订阅”，但收入预估远低于明确的商用交付工具；不建议作为现阶段首要现金流。

### P2 Lilyco Document — 私密文档 OCR 和导出
- 参考：https://github.com/naptha/tesseract.js (Apache-2.0, WASM)，https://github.com/PaddlePaddle/PaddleOCR (Apache-2.0；单独核验模型权重/数据许可)，https://github.com/JaidedAI/EasyOCR (Apache-2.0)。
- 浏览器本地 OCR -> 可编辑结构化表格 -> 按商家发票/订单/条码模板导出 CSV/XLSX；定价面向小批量档案、采购工作流、隐私敏感行业。
- 与通用 OCR 竞争激烈，定位应是**垂直工作流**和本地隐私，而不是“模型比别人强”。

## 2. 不作为默认商业依赖

- `imgly/background-removal-js`: AGPL，闭源重分发/云服务涉及义务，须获商业许可或单独开源合规。
- `cambacodes/spine-thing`: 其自有代码 MIT，但转换器 PolyForm Noncommercial；完整工具**不能当 MIT 商业成品**。
- `remotion-dev/remotion`: 按公司规模/商业再许可有特殊约束，免费许可不允许复制代码形成再销售衍生产品；当前使用我们已经实现的本地 HTML→Video。
- `immich-app/immich`: AGPL-3.0，暂不直接复制进商业封闭客户端。
- 未找到明确许可证的 Spine 二次开发/Viewer，不采集其代码进入 Lilyco。

## 3. 固定模型/上游来源治理

开发每个候选时登记：GitHub URL、commit SHA、源码许可证、模型权重许可证、字体/素材许可证、第三方分发义务、可替代实现、首次下载体积、设备 CPU/GPU 内存用量、CI 真实输入素材、每次成功交付的人工节省时间。

每周可自动对上游 release/tag 提醒，但**不自动无审查升级生产**：先 LICENSE 差异、API 差异、兼容矩阵测试，支持回滚。

## 4. 90 天验证（不是保证能盈利）

- 0–14 天：Lilyco Commerce 导出模板/批量队列 MVP；Lilyco Spine inspect 的安全元数据 CLI/MCP；10 个商家/游戏团队访谈。
- 15–45 天：打通 2–3 个端到端真实付费工作流；有可验证输出、客户设备基准和失败报告；不依赖云端 GPU。
- 46–90 天：选留存率最高的一条，做专业模板订阅/团队版，实际测试付费意愿/退款/客户获取成本；其余工具作为 Lilyco 开源获客入口。

**发布之前必须区分**：能够运行 ≠ 输出质量达标 ≠ 商业版权合规 ≠ 客户愿意付费。

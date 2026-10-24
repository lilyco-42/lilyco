# Lilyco AI Studio — 三视图到 3D 的本地自动工作流

产品目标：**用户购买软件后只表达目标和上传素材，不用学习 Blender、模型管理或命令行。** 客户设备运行 3D 软件、推理模型和质量检查，Lilyco 服务器只做可选账号/技能市场/更新。

现有最小可工作链路：

1. studio__lyco_find_skills：读取已审核的本地 lyco-skill catalog，另只读查询 CLI-Anything 的 cli-hub search blender --json。这里只是能力发现，严禁执行来自公开市场的任意脚本。
2. studio__lyco_environment_preflight：检测客户机是否装了 Blender 与 CLI-Hub、是否已批准本地创作根目录。
3. studio__lyco_create_3d_reference：接受正、侧、背 PNG/JPEG 路径；在用户设备 Blender --background --python 运行固定的 Blender reference-stage 脚本，输出可编辑 reference_stage.blend、草模预览 draft_preview.png、job.json、stage_result.json。
4. 用户 GUI：Lilyco Tool Hub 自动从 Schema 生成三视图参数字段和本机浏览按钮；同一工具可以被 AI 原生 MCP 调用。
5. 质量边界：当前交付是三视图参考工程和草模占位，不是经过图片重建的专业模型。job.json 明确标记 geometry-reconstruction = not_configured；此时 UI 不应显示“3D 成品完成”。

上游软件：Blender GPL；通过独立可执行程序调用、不混编源码。CLI-Anything Apache-2.0，通过 CLI-Hub 只读发现和经过审核的本地 harness；专家 pipeline 来自 lilyco-42/blender-game-pipeline-experts，版权 MIT，未审计的脚本不自动运行。具体仓库在 catalog/skills.json 中留出处和信任等级。

### 面向最终消费者的产品状态

- 用户语句：帮我制作一个 3D 模型，这是三视图。
- 系统弹出原生文件选择器或拖入正侧背三张图片。
- 页面阶段：正在寻找技能、正在检查环境、正在解析三视图、正在准备工程、正在生成预览；尽量让普通用户只看任务状态，底层 CLI/MCP/模型日志放高级面板。
- 只有获得签名/审计的技能可自动安装。新设备首次安装大型软件/显卡模型必须说明大小和来源并请求一次同意；之后在批准的名单内无感执行，不能静默授予管理员权限给任意 GitHub 内容。
- 最终“模型完成”必须由真实的多视图 3D 重建 Provider + 纹理/拓扑/UV/导出 QA 共同满足；目前这一步没有实现。
- 当前还没有收费或云模型调用；不因用户没有 GPU 就调用 Lilyco 业务服务器推理。

### 本地最小安装

从 GitHub Actions 的 Lilyco Tool Hub 发布包解压，按照其 README 生成 tools.json。额外为 studio 提供：

- Blender 4.2+ 本机安装；推荐从 Blender 官网或系统软件包管理器获取。
- 先在用户设备创建 LILYCO_AUTOMATION_ROOT（输出目录）。
- 可选安装 CLI-Anything 官方 cli-hub：pip install cli-anything-hub，然后 cli-hub search blender；harness 由审核的 CLI-Hub 环境进行安装。
- 直接启动 python -u hub.py --config tools.json 或人类 GUI python gui_server.py --config tools.json。
- 用 studio__lyco_environment_preflight 检查，再调用 studio__lyco_create_3d_reference。

### 下一步研发重点

1. 建立版本化、签名的 Lyco Skill 市场 API，包括适配器版本、设备资源、贡献者、依赖、许可证、回滚能力。
2. 引入合法商用的、客户端可部署的 3D 重建引擎（如 TRELLIS 类），评估三视图输入真实支持情况、模型权重许可、显存需求、Windows GPU 支持及低资源回退。
3. 对已生成网格调用专家技能拓扑修复、自动 UV/烘焙、FBX/GLB 导出；自动生成 360° 预览和差异质检。
4. 客户端工作流引擎需要有暂停、取消、恢复、检查点、下载体积和 CPU/GPU 限额；不要只实现一个长时间同步 HTTP 请求。
5. 面向商用按模板包/导出质量/批量额度/团队技能市场授权收费；基础工具免费 BYOK/本地化，不贩卖非授权 API 额度。

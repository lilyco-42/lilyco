# 生态融合蓝图：桌宠（丛雨）× lilyco

> Track 2 — 桌宠桥接。pet 的动作与语音成为 lilyco 工具（`lilyco-pet` crate，
> `lpet` 二进制），AI/Agent 可以让丛雨「说话」和「做动作」。
>
> 关联：Track 1（mpkg 人设包，本文 P1 阶段引用）；先例：`lly`（lilyco 工具层的
> 第一个外部 crate，其 EdgeTTS 实现方式被 `pet-say` 直接对齐复用）。

## 0. 参与者盘点

| 侧 | 资产 | 说明 |
|---|---|---|
| pet（lilyco-42/cute-pet） | macroquad 跨平台桌宠「丛雨」 | 立绘分层合成、眨眼/口型动画、聊天（LLM + 4404 条官方中译语料兜底）、GPT-SoVITS 克隆音色、微信/QQ 聊天记录语气学习 |
| pet 事件面 | `AppEvent::{Face,ToggleDress,ToggleDiff,QuickSpeak,…}` + `face_id()` 表 | 没有单一动作枚举——`pet-act` 的词表是其忠实投影（见 `lilyco-pet/src/action.rs` 待对齐说明） |
| lilyco | `#[derive(App)]` 四端派生 + P0 安全门（SafetyTier T0-T3）+ P1 遥测（ctx.telemetry）+ Registry | `lilyco-plc` 是硬件桥样板，`lilyco-pet` 按同一形态落地 |
| lly | EdgeTTS（msedge-tts crate）/ Whisper 转写 / 文件工具 | 「lilyco 工具层」的已验证先例；`pet-say` 复用其 `synthesize(text, voice, rate, pitch) -> mp3 bytes` 方案 |

## 1. 本 PR 落地（P0：桥接 + 语音）

### 1.1 工具签名

**`pet-say`（T0）** — 丛雨说话：EdgeTTS 文本转语音

| 参数 | 类型 | 必填 | 默认 | 说明 |
|---|---|---|---|---|
| `text` | String | ✅ | — | 要说的话（支持中日文） |
| `voice` | String | — | `zh-CN-XiaoyiNeural` | EdgeTTS 音色 id（少女声线近似丛雨） |
| `rate` | String | — | `+0%` | 语速，如 `+10%` |
| `output` | String | — | 系统临时目录 `lilyco-pet/pet_say_<时间戳>.mp3` | mp3 输出路径 |

行为：合成 → mp3 落盘 → 遥测上报 `pet.say.voice / pet.say.bytes / pet.say.seconds`
（48kbps ≈ 6KB/s 估算时长）→ Done 返回 `{ status, path, bytes, estimate_seconds, voice, rate }`。

**`pet-act`（T0）** — 丛雨做动作：输出结构化指令 JSON

| 参数 | 类型 | 必填 | 默认 | 说明 |
|---|---|---|---|---|
| `action` | ValueEnum | ✅ | — | `face_default/smile/confused/surprised/troubled/angry/childish/gloomy/furious`、`toggle_dress`、`toggle_diff`、`quick_speak` |
| `intensity` | f64 | — | `1.0` | 强度 0.0-2.0（夹取），预留动画幅度 / 情感强度门控 |

行为：产出指令 JSON（`{ event, face?, keypad?, intensity }`，`event` 对应 pet
`AppEvent` 变体名，`face` 为表情 id，`keypad` 为 1..=9 数字键序号）→ 遥测上报
`pet.action` → Done 返回 `{ status, action, intensity, instruction, note }`。

### 1.2 安全分级理由（为何都是 T0）

- `pet-say` 只写用户指定路径（缺省系统临时目录），不读敏感文件、不执行外部命令；
  网络出口只有微软 Edge 语音合成端点。
- `pet-act` 只产出指令 JSON——**不直接驱动任何渲染或真实世界行为**，消费方
  （pet 前端进程）是人类可见的桌宠窗口，天然"人类在环"。
- 对照表：P2 阶段若引入"持久改人设 / 写记忆库"类工具，将标 **T2**（见 §4）。

### 1.3 pet 前端如何接线（IPC / 本地 MCP）

```
┌─────────────────────────────┐         stdio JSON-RPC          ┌──────────────┐
│ pet 前端（macroquad 桌宠）  │◀──────(lpet --mcp 子进程)──────▶│ lpet 二进制  │
│  · 聊天输入 → 文本           │                                 │  pet-say     │
│  · 收到 mp3 路径 → 播放      │◀─────(telemetry/进度事件)───────│  pet-act     │
│  · 收到 instruction → AppEvent│                               └──────┬───────┘
└─────────────────────────────┘                                        │
        ▲                                                              │
        └────────────── Agent / MCP 宿主（Claude 等）直接调 lpet ──────┘
```

两种接法（按 pet 侧改造成本从低到高）：

1. **行协议起步（零依赖）**：pet 前端 `spawn` 一个长驻 `lpet --mcp`（或最简的
   `lpet pet-act --action …` 短命进程），解析其 stdout JSON。`pet-act` 的输出
   就是 `AppEvent` 投影，前端 switch 分发即可。
2. **本地 MCP 正坐**：pet 前端内嵌一个微型 MCP client（或复用 lilyco-mcp 的
   传输层），把 `lpet --mcp` 当本地工具服务器；同一时刻 Agent 宿主（Claude
   Desktop 等）也能看到 `pet-say / pet-act`，实现"AI 主动逗桌宠"。

### 1.4 语音链（聊天文本 → 丛雨开口）

```
聊天文本
  ├─ 官方克隆音色（还原度优先）：pet 现有 GPT-SoVITS 链路（PET_TTS_URL，GPU 常驻）
  └─ EdgeTTS 兜底（随手可用）：pet-say → mp3 路径 → pet 前端播放 + 口型动画
       · voice=zh-CN-XiaoyiNeural（少女声线近似）
       · rate 可绑定 persona 的语速特征（如 P1 语气风格里提取的短句比例）
```

取舍：有 GPU 服务 → 克隆音色优先；离线 / 低配 / 冷启动 → EdgeTTS 兜底。
两条链路产出同一形态（可播放音频），pet 前端按 `PET_TTS_URL` 是否配置自动切换
（现有 `TtsResult` / `TtsTimeout` 事件已具备兜底语义，无需改动状态机）。

## 2. P1：人设记忆走 mpkg（依赖 Track 1）

丛雨的人设不是一个 prompt，而是四类资产，应打进同一个 mpkg：

| mpkg 内容 | 来源 | 消费方 |
|---|---|---|
| 官方中译语料 4404 条（jsonl，语音码对齐） | pet `murasame_corpus_zh.jsonl` | pet `respond_corpus`、`pet-say` 的台词模板 |
| 语气风格（平均句长 / 短句比例 / 口头禅 / 代表句） | pet `chatlog::extract_style`（微信/QQ 记录学习产物） | LLM persona 系统提示、`pet-say` rate/voice 选择 |
| 语音资产索引（OGG / GPT-SoVITS 模型指针） | pet `assets/` | pet 前端播放、`pet-say` 克隆链路 |
| 好感 / 关系记忆（聊天记录蒸馏） | pet 聊天历史（最近 200 轮）→ mpkg 增量段 | 跨设备迁移：桌宠与 lilyco 工具共享同一份"丛雨认识你" |

落地动作：`lilyco-pet` 增加只读工具 `pet-persona`（T0，读 mpkg 返回人设摘要与
可用语音索引）；pet 前端启动时优先从 mpkg 装载，`PET_STYLE_LOG` 等环境变量
退为"导入器"。

## 3. P2：情感状态门控（改人设 = T2 的具体映射）

引入情感状态流（已由 P1 遥测铺路：`pet.action` / `pet.say.*` 就是现成的状态点）：

```
情感状态机（pet 侧，如 好感度/心情）
  → 随每次 pet-act / 聊天轮次经 ctx.telemetry 上报
  → lilyco 安全门按「动作后果」分层放行
```

| 操作 | 后果 | SafetyTier | 放行途径 |
|---|---|---|---|
| `pet-act` 表情/服装/说话 | 窗口内可见，即关即逝 | **T0** | 自动放行（现状） |
| `pet-say` 合成语音 | 写临时文件 | **T0** | 自动放行（现状） |
| 写入**会话内**心情微调（本次启动有效） | 影响响应风格，可逆 | **T1** | 本地交互面放行；MCP 面确认 |
| `pet-persona-edit` **持久改人设**（改语料/口头禅/好感记忆，写 mpkg） | 覆盖训练成果，影响所有后续交互，跨设备同步 | **T2** | 能力令牌策略；拒绝信息指明"改人设需持令牌" |
| 批量重训 GPT-SoVITS / 清空记忆 | 不可逆 | **T3** | 禁止自动化执行 |

映射要点：**「改人设」的不可逆程度决定档位**——会话内可逆 = T1；持久化 = T2；
重训/清空 = T3。`lilyco-pet` 现在只发 T0 工具，门控扩展点（`Registry::with_policy`
+ 自定义 `SafetyPolicy`）已在框架侧就绪，P2 只需新增工具并声明 `safety = "t2"`。

## 4. 分阶段落地计划

- [x] **P0 桥接 + 语音**（本 PR）：`lilyco-pet` crate（`pet-say` / `pet-act` 均 T0），
      `lpet` 二进制 CLI+MCP 双形态，测试离线全绿 + 真网络合成 `#[ignore]` 测试
- [ ] **P0.5 pet 侧消费**：pet 前端 spawn `lpet --mcp`，`pet-act` 指令 → `AppEvent`
      分发；`pet-say` mp3 → 口型动画播放（定稿 pet IPC 行协议）
- [ ] **P1 人设记忆**：丛雨 mpkg（语料 + 语气 + 语音索引 + 关系记忆）；
      `pet-persona` 只读工具；pet 从 mpkg 装载
- [ ] **P1.5 主动陪伴**：Agent（经 MCP）定时/事件驱动调 `pet-act`+`pet-say`
      组合，实现"AI 主动找你说话"，情感状态随交互经遥测回流
- [ ] **P2 情感状态门控**：情感状态机 → 遥测流；`pet-persona-edit`（T2 能力令牌）
      / 重训类操作（T3）落地，完成 §3 映射表

## 5. 待对齐项（跨仓契约）

1. **动作词表**：`PetAction` 是 pet 当前源码事件面的投影；pet IPC 协议定稿后以
   彼处为准（`tests/pet_bridge.rs` 的变体清单锁定测试会拦住单侧静默漂移）。
2. **默认音色**：`zh-CN-XiaoyiNeural` 是 EdgeTTS 免费音色里对丛雨的近似，非官方
   克隆；克隆音色始终走 pet 的 GPT-SoVITS 链路。
3. **instruction JSON 形状**：`{ event, face?, keypad?, intensity }` 是本 PR 提案，
   pet 侧确认字段后写入其 IPC 文档并在此回链。

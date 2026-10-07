## License

MIT OR Apache-2.0, at your option.


## 理论依据与相关研究

lilyco 的 "Token 2 Anything" 愿景建立在以下学术研究和工业实践基础之上：

### 1. Agent-Native Computer Use（Agent 原生计算机使用）

**核心论文：CLI-Anything: Towards Agent-Native Computer Use**
- 作者：香港大学 HKUDS 实验室
- arXiv: 2606.03854 (2026)
- 核心观点：提出 agent-native 计算机使用设计范式——不强迫 AI agent 导航视觉界面（GUI），而是创建与 agent 能力对齐的结构化命令行接口。一行命令即可将任意 GUI 软件转化为 AI agent 可操控的工具。
- 与 lilyco 的关系：lilyco 从代码层面实现这一理念，通过 Rust derive 宏自动生成 CLI/TUI/Web/MCP 四端，让软件天生 AI-callable。

### 2. Model Context Protocol (MCP) 与工具标准化

**核心论文：Model Context Protocol (MCP): Landscape, Security Threats and Future Directions**
- arXiv: 2503.23278 (2025)
- 核心观点：MCP 是一个新兴的开放标准，定义了 AI 模型与外部工具/资源之间的统一、双向通信和动态发现协议。

**实践论文：MCPToolBench++: A Large Scale AI Agent Model Context Protocol MCP Tool Use Benchmark**
- arXiv: 2508.07575 (2025)
- 核心观点：大规模评估 LLM 和 AI Agent 的 MCP 工具使用能力。

### 3. LLM Agent 工具使用与自主性

**综述论文：From LLM Reasoning to Autonomous AI Agents**
- arXiv: 2504.19678 (2025)
- 核心观点：全面综述从 LLM 推理到自主 AI agent 的发展路径。

**实践论文：LLM Agents Making Agent Tools (ToolMaker)**
- arXiv: 2502.11705 (2025)
- 核心观点：让 LLM agent 能够动态创建专用工具。

### 4. 终端使用 Agent 评估

**基准论文：TUA-Bench: A Benchmark for General-Purpose Terminal-Use Agents**
- arXiv: 2606.28480 (2026)
- 核心观点：评估通用终端使用 agent（TUA）的基准。

## 愿景实现路径

```
让软件天生可被 AI 调用（AI-callable by default）
        ↓
通过 MCP 标准协议暴露工具能力
        ↓
AI agent 发现 → 规划 → 调用 → 完成任何任务
        ↓
人类不再需要亲自操作计算机
```

lilyco 通过 Rust derive 宏实现"一个 struct 派生四端"：CLI、TUI、Web、MCP，结合采样桥实现工具与 LLM 的双向智能交互。

## 参考文献

1. CLI-Anything: Towards Agent-Native Computer Use. arXiv:2606.03854, 2026.
2. Model Context Protocol (MCP): Landscape, Security Threats and Future Directions. arXiv:2503.23278, 2025.
3. MCPToolBench++: A Large Scale AI Agent Model Context Protocol MCP Tool Use Benchmark. arXiv:2508.07575, 2025.
4. From LLM Reasoning to Autonomous AI Agents. arXiv:2504.19678, 2025.
5. LLM Agents Making Agent Tools (ToolMaker). arXiv:2502.11705, 2025.
6. TUA-Bench: A Benchmark for General-Purpose Terminal-Use Agents. arXiv:2606.28480, 2026.

---

## 相关项目与生态

### 核心相关项目

| 项目 | 链接 | 与 lilyco 的关系 |
|---|---|---|
| CLI-Anything | https://github.com/HKUDS/CLI-Anything | 理念互补：lilyco 从代码层面实现 agent-native，CLI-Anything 提供将现有软件转化为 agent 工具的流程 |
| MCP Rust SDK | https://github.com/modelcontextprotocol/rust-sdk | lilyco MCP 端可直接使用官方 Rust SDK 作为协议基础 |
| MCP 官方服务器 | https://github.com/modelcontextprotocol/servers | MCP 参考实现与社区服务器集合，提供最佳实践参考 |
| awesome-ratatui | https://github.com/ratatui/awesome-ratatui | lilyco TUI 端可基于 ratatui 框架 |
| Awesome-Agent-Papers | https://github.com/luo-junyu/awesome-agent-papers | LLM agent 论文集合，作为生态资源链接 |
| Awesome-AI-Agents | https://github.com/Jenqyang/Awesome-AI-Agents | 自主 AI agent 集合，展示多接口 agent 生态 |

### Agent 生态消费者（验证 lilyco 生成工具的兼容性）

| 项目 | Stars（约） | 用途 |
|---|---|---|
| OpenCode | ~200k | 开源终端 agent，消费 MCP 工具 |
| Claude Code | ~142k | Anthropic 官方 agent，MCP 兼容 |
| OpenAI Codex | ~109k | OpenAI 官方 agent |
| Gemini CLI | ~107k | Google 官方 agent |
| OpenHands | ~89k | 开源 agent 环境 |
| CrewAI | ~52k | 多 agent 编排框架 |
| OpenAI Agents SDK | ~27k | 官方 agent SDK |

---

## 前沿研究方向

### 1. 复杂工具链评估（ComplexMCP）

**ComplexMCP: Evaluation of LLM Agents in Dynamic, Interdependent, and Large-Scale Tool Sandbox** (arXiv:2605.10787, 2026)

- 当前 LLM agent 擅长调用孤立 API，但在商业软件自动化"最后一公里"表现不佳。
- 真实场景中工具是原子化、相互依赖、易受环境噪声影响的。
- ComplexMCP 提供 300+ 工具、7 个有状态沙箱，系统评估 agent 在动态工具链中的能力。
- **对 lilyco 的启示**：MCP 实现需考虑工具依赖与状态管理，可作为未来评测基准。

### 2. LLM 作为认知控制器

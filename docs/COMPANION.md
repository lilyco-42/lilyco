# P3 伴飞计算机档案 — aarch64 Linux 板（Radxa Cubie A7A 等）

伴飞计算机 = 挂在生产现场、与主控机并肩跑的 **aarch64 Linux 小板**。它的职责：

1. **CI 持续证明**：每次 PR / push，GitHub Actions 都在 `aarch64-unknown-linux-musl` 目标上
   编译 lilyco 全套 headless crate（CLI + MCP + PLC），aarch64 可编性不是口头承诺，是流水线事实。
2. **现场算子**：板上跑一个静态单文件二进制 `lplc`，作为 MCP 服务器（Agent 可调）
   或 CLI 注册表（脚本/定时轮询），直连 Modbus TCP 工业设备。

参考硬件：**Radxa Cubie A7A**（Allwinner A733，aarch64 Linux）。
生态先例：Radxa 板上已跑过 lilyco 系工具（lly）与 cache-node（musl 静态单文件 1-1.3MB），
本文的 musl 路线与 cache-node 同一路数——**scp 一个文件上去就能跑，不挑发行版与 glibc 版本**。

---

## CI 证明（`companion (aarch64)` job）

`.github/workflows/ci.yml` 中的 `companion` job：

- runner：`ubuntu-latest`（x86_64 宿主机交叉编译 aarch64 目标）
- 目标：`aarch64-unknown-linux-musl`（静态链接，纯 Rust 依赖树，零 C 依赖）
- 构建范围：`lilyco`（facade，headless）+ `lilyco-core` + `lilyco-cli` + `lilyco-mcp` + `lilyco-plc`
- 产物：`lplc-aarch64-linux-musl` artifact（静态单文件，下载即部署）

job 与现有 `test` / `release-build` / `android` job **独立并行**，不挂 `needs`，
不拖慢 PR 主路径。

---

## 交叉编译命令（与 CI 完全一致，可复制）

```bash
# 1) 工具链（x86_64 Linux 宿主机）
rustup toolchain install stable --profile minimal
rustup default stable
rustup target add aarch64-unknown-linux-musl
# aarch64 交叉链接器：rustc 给 musl 目标的链接命令传 AArch64 专用旗标
# （--fix-cortex-a53-843419），宿主 x86_64 的 ld 不认识，必须换 aarch64 ld
sudo apt-get install -y gcc-aarch64-linux-gnu

# 2) 构建 headless（CLI + MCP + PLC），静态单文件
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=aarch64-linux-gnu-gcc
cargo build -p lilyco -p lilyco-core -p lilyco-cli -p lilyco-mcp -p lilyco-plc \
  --target aarch64-unknown-linux-musl --no-default-features --release

# 产物
ls -lh target/aarch64-unknown-linux-musl/release/lplc
file target/aarch64-unknown-linux-musl/release/lplc
# → ELF 64-bit LSB executable, ARM aarch64, statically linked
```

> Windows/macOS 宿主机同样可行：musl 交叉工具链可从 musl.cc 或用
> `cross`（`cargo install cross`）获得，构建命令不变，仅替换链接器环境变量。

`--no-default-features` 是关键一行：裁掉 `tui` / `web` 后端后，
依赖树只剩 `serde` / `serde_json` / `thiserror` / `clap`，全部纯 Rust，
musl 静态链接一次通过。这与 README 中 aarch64-linux-android 的 headless 路线是同一套 feature 纪律。

---

## 部署到 Radxa Cubie A7A

### 1. 上传产物

```bash
# CI 侧：从 Run 页面下载 lplc-aarch64-linux-musl artifact（或本地按上节构建）
scp target/aarch64-unknown-linux-musl/release/lplc radxa@<板子IP>:/tmp/lplc

# 板上：校验（建议对照 CI 产物 sha256）+ 安装
ssh radxa@<板子IP>
sha256sum /tmp/lplc
sudo install -m 0755 /tmp/lplc /usr/local/bin/lplc
lplc --help   # 冒烟：看到 plc-read / plc-write 子命令即成功
```

静态链接的好处：不依赖板上 libc 版本，Debian / Ubuntu / Armbian / 龙芯系镜像通吃。

### 2. 跑成 MCP 服务器（systemd socket 激活，Agent 经网络直连）

`lplc --mcp` 是 stdio MCP 服务器。systemd 的 socket 激活可把 stdio 协议转成
TCP：每个连入连接 = 一个全新 MCP 会话，无需常驻进程，板上零额外依赖。

`/etc/systemd/system/lplc-mcp.socket`：

```ini
[Unit]
Description=lplc MCP (Modbus gateway) socket

[Socket]
ListenStream=5021
# 只放行局域网：加 BindIPv6Only=both 与防火墙规则，或改 ListenStream=192.168.x.x:5021

[Install]
WantedBy=sockets.target
```

`/etc/systemd/system/lplc-mcp.service`：

```ini
[Unit]
Description=lplc MCP server (stdio over socket)
Requires=lplc-mcp.socket

[Service]
# socket 激活：stdin/stdout 直接接到 TCP 连接上
StandardInput=socket
StandardOutput=socket
ExecStart=/usr/local/bin/lplc --mcp
# 最小权限
DynamicUser=yes
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload && sudo systemctl enable --now lplc-mcp.socket
```

Agent 侧（MCP 客户端）把远端 stdio 服务器配成 ssh 传输亦可：

```json
{ "lplc": { "command": "ssh", "args": ["radxa@<板子IP>", "/usr/local/bin/lplc --mcp"] } }
```

ssh 方式自带认证加密，适合跨网段；socket 激活适合局域网低延迟直连。

### 3. CLI 注册表模式（定时轮询 / 脚本）

```bash
# 手动：读 4 个保持寄存器（T0 只读，自动化面直接放行），逐寄存器遥测
lplc plc-read --host 127.0.0.1:5020 --addr 0 --count 4

# 写寄存器（T2 能力令牌——自动化面默认拒绝，需部署侧注入 token 的 SafetyPolicy）
lplc plc-write --host 127.0.0.1:5020 --addr 0 --value 42
```

`/etc/systemd/system/lplc-poll.service` + `.timer`（每 30 秒读一次，结果进 journal）：

```ini
# lplc-poll.service
[Unit]
Description=lplc register poll

[Service]
Type=oneshot
ExecStart=/usr/local/bin/lplc plc-read --host 127.0.0.1:5020 --addr 0 --count 4

[Install]
WantedBy=multi-user.target
```

```ini
# lplc-poll.timer
[Unit]
Description=lplc register poll timer

[Timer]
OnBootSec=1min
OnUnitActiveSec=30s

[Install]
WantedBy=timers.target
```

---

## 与 cache-node 节点组网（下一步路线）

cache-node 已在 Radxa 生态验证过 **musl 静态单文件** 的分发形态（1-1.3MB 单文件，
拷贝即部署）。`lplc` 沿用同一形态后，组网路线：

1. **节点发现**：板子上报自身（`hostname` / IP / `lplc --mcp` 端口）到 cache-node
   注册表，形成"边缘算子清单"。
2. **统一入口**：Agent 不直连每块板，而是经 cache-node 路由到目标板的 MCP 端点，
   板间用 Token 门控（lilyco P0 安全门：T0 直通 / T2 写令牌）。
3. **遥测汇聚**：`plc-read` 的 P1 遥测流（JSONL 逐寄存器数据点）汇入 cache-node，
   形成现场状态时间线。

以上为路线图，落地顺序以 P3 之后的迭代为准。

## lilyco-plc 接 GPIO / Modbus 设备（下一步路线）

- **Modbus TCP（已通）**：`lilyco-plc` 是零依赖 Modbus TCP 客户端，
  任何 PLC / 网关 / 变频器的 502 端口都可直接读写。
- **串口 Modbus RTU（规划）**：A733 板载 UART，接 RS-485 收发器即可覆盖 RTU 设备；
  预计以最小依赖方式（termios 系统调用）加到 `lilyco-plc`。
- **裸 GPIO（规划）**：经内核 gpiod 字符设备接口把引脚抽象成寄存器视图，
  与 Modbus 命令同构（`gpio-read` / `gpio-write`），复用同一套 T0/T2 安全门与遥测。

---

## 已知限制

- **TUI / Web 后端在伴飞板上被裁剪**（`--no-default-features`）：
  - 板子是无头设备，没有显示器与浏览器：`web` 后端会拉起 axum + tokio(full) +
    `webbrowser`（开浏览器），在无人值守的板上没有意义；
    `tui` 后端需要交互终端，定时/无人值守场景用不上。
  - 更硬的约束是**依赖树纪律**：裁剪后全链路纯 Rust（serde / clap / thiserror），
    musl 静态链接零波折，单文件 scp 即部署——与 android headless 构建是同一套
    feature 门控（见 `lilyco/Cargo.toml` 注释）。
  - 需要人工交互时，从运维终端 ssh 进板子用 CLI 子命令即可（TUI 体验留给桌面端）。
- **MCP 经 socket 激活暴露 TCP 时无内建认证**：务必限定局域网/防火墙，
  或优先用 ssh 传输（自带认证加密）；写操作受 P0 安全门 T2 令牌保护。
- **musl 静态二进制不做 DNS 解析的高级功能**（glibc NSS 插件不可用）：
  `--host` 请直接写 IP 或可静态解析的主机名，板上场景不受影响。

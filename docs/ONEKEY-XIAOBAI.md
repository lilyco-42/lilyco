# 手机一键跑小模型（小白版，全程 3 步）

> 目标：旧手机变离线 AI 节点。不用懂代码，会复制粘贴就行。

## 步骤 1：装 Termux（1 分钟）

去 **F-Droid**（注意不是 Play 商店）搜 `Termux` 安装。打开，看到 `$` 光标即成功。

## 步骤 2：跑一键脚本（自动下载约 400MB）

把 `scripts/termux-onekey.sh` 传到手机（如微信文件/QQ/OTG 任选其一），在 Termux 里执行：

```bash
bash termux-onekey.sh
```

看到 `✅ 完成` + 一句中文回复即成功。想加“看屏点按”的眼睛，改跑：

```bash
bash termux-onekey.sh --vision   # 多下约 2GB，喝杯水等
```

## 步骤 3：开始用（以后每次）

```bash
llama-cli -m ~/.lilyco/models/Qwen3-0.6B-Q4_K_M.gguf -ngl 99
```

- 手机只负责 **Approve / Reject**（点头摇头），重活仍在你的 Linux 小主机（A7A/路由器）上跑。
- 断网可用（首次下载后全离线）；4GB 内存以上手机都行，6GB+ 更顺。
- 出错先重跑一遍脚本（可重复执行，不会搞坏东西），再看报错最后一行发群里。

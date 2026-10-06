# 长尾待办（Backlog）

> 2026-10-06 建立。**这里放「想做但没排期」的事；issue tracker 只放「已确认的问题」。**
>
> 依据：一个条目如果既没有可复现的问题、也没有排期和负责人，它就不是 issue ——
> 它是愿望。愿望放文档，问题放 tracker，两者的生命周期和读者都不一样。
> 关闭不代表放弃，只代表**换了个该待的地方**，勾选框原样搬过来了。

---

## 1. 开发者提效 `lyco-cli` / `lyco-skill`

来源：`lilyco-42/lyco-cli#1`（已关闭）

- [ ] 消除 `lyco-cli` 与 `lyco-engine` 的文档重复（见 `lyco-engine` 定位 item）
      —— **阻塞**：等 `lyco-engine#1` 的 A′/B/C 决策落定
- [ ] `lyco-cli` 接入生态模板约定，新项目默认走声明式 `template.yaml`
- [ ] `lyco-skill` 预研结论沉淀为可检索的 build-vs-buy 决策表

## 2. 业务闭环 `lytrade` / `lysource` / `lly`

来源：`lilyco-42/lytrade#1`（已关闭）

- [ ] `lytrade` 跑通回测：信号 → 过滤 → 撮合 → 绩效归因
- [ ] `lysource` 输出稳定信号 API，带健康检查与降级
- [ ] `lly` 语音/转写接入至少一个真实调用方
- [ ] 三者共用一套配置与部署（归 `lyco-ops`）

**边界（不可越线）**：`lytrade` 仅供策略学习与验证，**绝不调用真实下单接口**。

## 3. 运维沉淀 + 路由评估门禁

来源：`lilyco-42/lyco-ops#1`（已关闭）

背景：`lyco-ops` 承接生态云/板端运维（2026-09-19 已从 `lyco_agent` 拆出，生态 P2 该项已完成）；
`lyco-router-eval` 结论「0.6B 够用，但必须配三件套（拒答 + 白名单 + 人工拍板）」已写入需求基线。

- [ ] 生态部署统一走 `lyco-ops`，业务仓不再自带部署脚本
- [ ] 三件套固化为发布门禁：拒答 / 白名单 / 人工拍板缺一不可
- [ ] 评估用例集版本化，模型或路由变更必回归

## 4. `lilyco` 主仓门面与生态导航

来源：`lilyco-42/lilyco#21`（已关闭）

- [ ] README 首屏明确生态入口定位，链到本 Project roadmap
- [ ] 生态导航（16 仓，按品牌 / 基座 / Agent / 工具 / 业务 / 运维 分类）
- [ ] 明确与 `lyco-cli` / `lyco-engine` / `lly` 的能力边界
- [ ] crates.io / docs 描述与 README 口径一致

## 5. 品牌与官网统一对外

来源：`lilyco-42/lilyco-42.github.io#1`（已关闭）

- [ ] 首页 Featured 卡片按生态分级排序（P0 优先）
- [ ] 视觉统一到 `lilyco-brand` 规范（风车结 mark、字标、色彩）
- [ ] 预研记录逐条可核验，链到对应仓库
- [ ] 移动端可用，无横向滚动

## 6. `ly*` 家族仓库分级与 README 导航

来源：`lilyco-42/lilyco-42#1`（已关闭）

背景：`docs/ECOSYSTEM_ROADMAP.md` P0 已归档 7 个零活动 fork + 2 个空仓；
但 `ly*` 家族 16 仓仍无统一分级与入口导航。

- [ ] 16 仓给出 P0/P1/P2 分级（依据 pushedAt + star + 依赖关系）
- [ ] 主仓 README 增生态成员导航表，每行标注分级与一句话定位
- [ ] 分级结果与本 Project README 保持一致

**验收标准**
- [ ] 16 仓全覆盖，每仓有分级 + 定位
- [ ] 归档决策逐仓有理由

---

## 从这里捞活干的规则

1. **有人真的要做时，开一个 issue**，写上可复现的现状和验收标准，再动手 ——
   不要把这些勾选框原地复活成「任务 issue」。
2. **做完一项就删一行**，不留已完成的陈述；文档描述现状，不描述历史。
3. 排期口径：需求以 `lyco_agent/docs/requirements-baseline-2026-09-23.md` 为准，
   生态以 `docs/ECOSYSTEM_ROADMAP.md` 为准。

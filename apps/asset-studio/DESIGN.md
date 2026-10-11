# Lain42 素材工坊

桌面创作工作台，不做营销页。左侧工具导航、导入与动作控制、右侧透明素材预览。
默认深色中性背景、绿色主操作，可切换浅色；颜色全部集中在 studio-theme.css 语义 token。
导入、读取、预览、导出是唯一主流程，取消和删除就近可见，失败有可操作文字。
大导出只显示 24 张预览和 100 个文件行，完整 ZIP 不截断。手机尺寸采用单列，不横向溢出。

React 19 + Vite + Tailwind v4 静态构建，由已有本机 FastAPI 服务，不新增 Next.js 服务。
button/card/badge/separator 是官方 base-vega registry 原始组件，不手改；来源与 SHA 在
ui/src/shadcn-sources.json，MIT 原文在组件目录。应用只通过 wrapper 和 className 定制。
所有生成、类型检查、真实浏览器和安装器验证在 GitHub Actions，不在本机编译。

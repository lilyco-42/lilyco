# 图片工坊界面约束

复用原素材工坊 / Mirage 桌面壳与 shadcn Base Vega 原始组件，组件保持只读并由 SHA 门禁核对。业务交互在 `vision-workspace.tsx`，文件、会话、异步任务在 `use-vision-workspace.ts`；工具标签和参数默认值在 `vision-tools.ts`。主题只使用 `studio-theme.css` 语义变量，不复制截图。

桌面为工具选择、原图与成果、参数三列；平板/手机自适应成纵向。裁剪用 `vision-crop-preview.tsx` 的 SVG 指针框选，按 getScreenCTM 映射原图像素，保留坐标输入供键盘使用；主色通过 SVG 填充显示，避免依赖 CSP 禁用的动态内联 CSS。HTML 不在工作台中嵌入执行，原图预览只使用用户选取的图片 Object URL，清空 / 替换会释放。

空、处理中、取消、失败和完成分别呈现，失败任务不提供下载。处理中不能切换输入与工具；清空会取消任务并删除真实临时文件。下载使用实际引擎结果，ZIP 必须包含报告及真实输出，不能以动画或示例图充当处理结果。

按钮、卡片、徽章、分隔线使用原 shadcn，输入保留标签、键盘访问，主题支持深浅色，实时状态用 aria-live。验收覆盖 320 / 390 / 768 / 1440 px 和真实 Windows WebView2；不以普通浏览器冒充原生桌面证明。

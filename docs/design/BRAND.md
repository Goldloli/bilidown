# bilidown 品牌资产

> 图标沿用项目自绘蓝粉图形，MIT 许可；不含第三方图标、插画或字体。

## 1. 标识与唯一母版

- 构成：蓝色渐变圆角底板、白色视频卡与蓝色播放三角、右上方粉色下载芯片与白色下箭头。
- 唯一可编辑母版：`docs/design/app-icon.svg`，96×96 viewBox，默认导出 1024×1024。
- `web/src/components/BrandMark.tsx` 中的 `BrandMark`（空态/关于页）与 `BrandGlyph`（标题栏）均显示母版导出的 `web/public/app-icon.svg`，保持颜色、主体形状及芯片位置一致。
- `web/public/favicon.svg` 与 PNG、Windows 图标均为生成物，不单独修改。`docs/design/icons/` 中的历史探索图不作为生成输入。

## 2. 生成入口

项目依赖安装完成并存在系统 Chrome 后，在仓库根目录运行：

```bash
node scripts/generate-icons.mjs
```

Windows 原入口仍可使用：

```powershell
pwsh -File scripts/make-ico.ps1
```

脚本复用现有 Playwright，以透明背景直接栅格化 SVG；默认使用系统 Chrome，不下载浏览器。可通过 `ICON_BROWSER_CHANNEL=msedge` 切换已安装的 Edge。同一浏览器版本下可以重复生成；脚本不使用旧 PNG 作为输入。

| 生成物 | 用途 |
| --- | --- |
| `web/public/app-icon.svg`、`favicon.svg` | 界面矢量标志、浏览器图标 |
| `web/public/app-icon-{64,256,1024}.png`、`favicon-32.png` | 前端所需位图 |
| `src-tauri/icons/{32x32,64x64,128x128,128x128@2x,icon}.png` | Windows/Tauri 图标；icon.png 为 512×512 |
| `src-tauri/icons/icon.ico` | 16/24/32/48/64/128/256 共七帧的 32 位透明图标 |

在 macOS 上，脚本还会生成标准 `.iconset` 并调用系统 `iconutil` 输出 `src-tauri/icons/icon.icns`；Dock、Finder 与 `.app` 图标均来自该文件。StoreLogo 与移动端资源仍不在此脚本输出范围，且不会被覆盖。切换系统图标后若 Finder/Dock 仍显示缓存旧图，应重新启动对应系统进程或使用新的构建产物路径复核。

Windows 的 EXE、默认窗口与托盘通过 Tauri 默认图标链路共用 ICO；安装器/卸载器也指向该 ICO。修改资源后必须重新构建应用与安装器，单改源文件不会更新已运行的旧 EXE。

## 3. 色彩

| 用途 | 值 | 说明 |
| --- | --- | --- |
| 下载芯片/界面强调色 | `#FB7299` | 沿用 B 站粉 |
| 界面强调色按下态 | `#E85F87` | 不另改图标配色 |
| 图标底板渐变 | `#4D98FF` → `#2B50E0` | 蓝色圆角底板与播放三角 |
| 媒体卡/下载箭头 | `#FFFFFF` | 白色主体 |

## 4. 使用与检查

- 保持等比，不旋转整体、不附加发光或外部底色；母版自带的渐变与芯片角度保持不变。
- 标题栏推荐 17–18px；系统 ICO 最小 16px。两个界面组件直接显示 SVG，避免多套手绘小图逐渐分叉。
- 检查各 ICO 帧尺寸、圆角透明区域与蓝粉主体；透明边缘不得混入黑色块。
- 构建后核对 EXE 的内嵌帧与 ICO，再查看任务栏、Alt+Tab、托盘、安装器及新快捷方式；旧固定项仍异常时先核对目标路径。
- 窗口外形需在真实桌面应用验收，浏览器内图标正常不代表原生窗口或任务栏已经通过。

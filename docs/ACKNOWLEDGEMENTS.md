# 致谢与第三方说明

bilidown 的实现可独立复现，界面、图标与文案均为项目原创；第三方内容仅限下列公开依赖与公开协议资料。

## 公开协议与文档

- [Bilibili-API-collect](https://github.com/SocialSisterYi/bilibili-API-collect)：B 站公开接口协议、WBI 与登录流程的社区文档；
- B 站 web 端公开接口的请求/响应行为（仅作为接口行为依据，不使用其代码或资源）；
- DASH、HTTP Range、SRT、FFmpeg 命令等公开规范。

## 依赖与工具

- [Tauri](https://tauri.app/) · [React](https://react.dev/) · [Tailwind CSS](https://tailwindcss.com/)
- [FFmpeg](https://ffmpeg.org/)：DASH 合流、音频转码。Windows 使用 BtbN LGPLv3 构建；macOS 使用 FFmpeg 8.1.3 + [LAME 3.100](https://lame.sourceforge.io/) 的 LGPL 静态 arm64 构建，版本、SHA256、构建参数和源码地址随安装包附带
- Rust crates：reqwest、rusqlite、tokio、tracing、rsa、hmac、sha2 等（见 `Cargo.lock`）
- 前端依赖：Vite、Zustand、motion、@tanstack/react-virtual 等（见 `web/package.json`）

## 图标

应用图标由项目作者使用 AI 图像工具生成并筛选定稿，源文件见 `docs/design/icons/v3-transparent/b-blue-tray-chip.png`。

## 许可

本项目以 MIT License 开源，详见根目录 `LICENSE`。安装包内 FFmpeg/LAME 的第三方许可证与源码说明位于 `src-tauri/resources/`，并会随 macOS/Windows 安装包分发。

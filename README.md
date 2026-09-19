<div align="center">
  <img src="./app/public/logo.svg" width="128" height="128" alt="Shanten Lens Logo" />
  <h1>Shanten Lens</h1>
</div>

Shanten Lens 是一款面向《雀魂》青云之志模式的桌面辅助工具。应用会在本地解析游戏状态，提供护身符与商店信息、手牌与牌山分析、换牌指导、分数推演、自动化、熔断保护、插件扩展和调试功能。

<div align="center">
  <a href="https://github.com/SpCoGov/shanten-lens/stargazers"><img src="https://img.shields.io/github/stars/SpCoGov/shanten-lens?logo=github" alt="GitHub stars" /></a>
  <a href="https://github.com/SpCoGov/shanten-lens/releases"><img src="https://img.shields.io/github/v/release/SpCoGov/shanten-lens?label=release&logo=github&include_prereleases" alt="Latest release" /></a>
  <a href="https://github.com/SpCoGov/shanten-lens/issues"><img src="https://img.shields.io/github/issues/SpCoGov/shanten-lens?logo=github" alt="Open issues" /></a>
  <a href="./LICENSE"><img src="https://img.shields.io/badge/license-Apache%202.0-blue?logo=apache" alt="License: Apache-2.0" /></a>
  <a href="https://github.com/SpCoGov/shanten-lens/actions/workflows/release-build.yml"><img src="https://img.shields.io/github/actions/workflow/status/SpCoGov/shanten-lens/release-build.yml?branch=v3&logo=githubactions&label=build" alt="Build status" /></a>
  <a href="https://tauri.app/"><img src="https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white" alt="Tauri 2" /></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Rust-2021-000000?logo=rust&logoColor=white" alt="Rust 2021" /></a>
  <a href="https://react.dev/"><img src="https://img.shields.io/badge/React-18-61DAFB?logo=react&logoColor=black" alt="React 18" /></a>
</div>

[日本語版 README](./README_JA.md)

## 目录

- [使用须知](#使用须知)
- [功能](#功能)
- [使用说明](#使用说明)
- [技术栈](#技术栈)
- [目录结构](#目录结构)
- [构建与打包流程](#构建与打包流程)
- [项目校验](#项目校验)
- [插件](#插件)
- [常见问题](#常见问题)
- [许可](#许可)

## 使用须知

本项目仅用于教育和研究。第三方辅助、自动化和修改通信可能违反游戏服务条款，并可能导致账号限制。使用者应自行了解并承担相关风险。

目前最稳定的使用环境仍然是 Windows。日常使用前需要准备：

- 可正常运行的 Shanten Lens 桌面程序
- `Proxifier` 或具备进程级代理转发能力的同类工具
- 安装本地根证书的权限
- 能被代理工具接管的雀魂客户端或浏览器进程

首次使用时，建议先启动一次 Shanten Lens 再关闭，让程序生成配置、证书和数据目录。

## 功能

- 主界面实时显示：
  - 当前护身符与印章
  - 商店商品与候补护身符
  - 手牌、牌山和换牌堆
  - 打牌建议与牌山统计
  - 当前分数、目标分数和关卡信息
- 分数计算器：
  - 按当前护身符顺序实时计算得分
  - 编辑单个护身符的触发次数、额外触发和成长公式
  - 推测未来关卡分数
  - 覆盖传导印章和后续成长等计算规则
- 花火与换牌指导：
  - 搜索花火换牌方案
  - 查看候选方案、四杠目录和参与搜索的牌
  - 计算万象换牌方案
  - 按步骤执行或一键执行换牌方案
- 自动化：
  - 按目标护身符和印章反复执行青云之志
  - 查看重开记录、候选详情和操作日志
  - 支持目标筛选、次数限制和邮件通知
- 熔断与守护：
  - 拦截或确认部分高风险操作
  - 配置购物、跳过、和牌、退出等保护规则
- 调试与辅助：
  - 配置封包流水线和模块顺序
  - 查看运行日志、游戏状态和封包详情
  - 使用独立 HUD 窗口显示关键信息
- 插件：
  - 从插件市场或本地 ZIP 安装插件
  - 管理插件状态、封包权限和自动更新
  - 通过插件扩展后端处理模块和前端页面

## 使用说明

以下步骤以Windows和Proxifier为例。

### 1.配置Proxifier

1. 从[Proxifier官网](https://www.proxifier.com/)下载并安装Proxifier。
2. 打开`Profile -> Proxy Servers... -> Add...`。
3. 添加本地代理：

   - `Address`：`127.0.0.1`
   - `Port`：`10999`
   - `Protocol`：`HTTPS`

4. 如果Proxifier询问是否设为默认代理，选择`No`。
5. 打开`Profile -> Proxification Rules... -> Add...`，为雀魂进程添加规则：

   - 客户端版通常填写`Jantama_MahjongSoul.exe`
   - 浏览器版填写实际使用的浏览器进程，例如`chrome.exe`或`msedge.exe`
   - `Action`选择刚才创建的本地代理

不要把Shanten Lens自身加入代理规则，否则可能形成代理循环。

### 2.安装根证书

首次启动MITM代理后，程序会在应用数据目录的`configs/ca/`中生成：

- `shanten-lens-ca.cer`
- `shanten-lens-ca.key`

可以在程序设置中选择“打开配置目录”，然后进入`ca`目录。双击`shanten-lens-ca.cer`，将证书安装到“本地计算机”的“受信任的根证书颁发机构”。

私钥文件`shanten-lens-ca.key`只应保存在本机，不要分享或上传。

### 3.启动顺序

推荐顺序：

1. 启动Shanten Lens
2. 确认MITM代理已监听`127.0.0.1:10999`
3. 启动Proxifier
4. 启动雀魂客户端或浏览器

注意：

- 如果Proxifier仍在运行，但Shanten Lens没有运行，游戏流量会被转发到不存在的本地代理，导致无法连接。
- 如果只在少数场景使用本工具，可以只在需要时启用Proxifier规则。
- 如果修改了设置中的MITM端口，Proxifier中的端口也必须保持一致。

### 4.与Cheat Engine配合使用

部分用户会将Shanten Lens与Cheat Engine配合使用，常见用途包括：

- 加快客户端动画，减少等待时间
- 在自动化期间将游戏速度设为`0`
- 避免使用过高倍率导致连接中断，建议最高不超过`5`

使用任何第三方工具前，请自行确认风险和适用规则。

## 技术栈

- 前端：`React 18`、`TypeScript`、`Vite 5`
- 桌面容器：`Tauri 2`
- 后端：`Rust 2021`
- MITM 代理：`Hudsucker`
- 协议：`protobuf`、HTTPS 和 WebSocket
- 前后端通信：Tauri 进程内 IPC commands 和 events
- 包管理器：`pnpm 9`

Shanten Lens 3.0 将 Rust 后端直接嵌入 Tauri 进程，不需要 Python 后端、sidecar 或独立的 UI WebSocket 服务。

## 目录结构

```text
.
├─ app/                 React 前端与 Tauri 桌面应用
│  ├─ src/              页面、组件和前端业务逻辑
│  ├─ public/           护身符、印章、牌面与 Logo 资源
│  └─ src-tauri/        Tauri Rust 入口与 IPC 命令
├─ backend/             内嵌 Rust 后端、MITM、插件和自动化逻辑
├─ proto/               游戏协议定义
├─ scripts/             Windows 与 macOS 打包脚本
├─ .github/workflows/   持续集成与发布流程
├─ Cargo.toml           Rust workspace
└─ README.md
```

## 构建与打包流程

### 环境要求

- Rust stable 与 Cargo
- Node.js 22
- pnpm 9
- Tauri 2 对应平台的系统依赖

安装前端依赖：

```powershell
cd app
pnpm install
```

Rust 依赖会由 Cargo 在构建时自动下载。

### 本地开发

Rust 后端内嵌在 Tauri 进程中，没有单独的启动命令。

```powershell
cd app
pnpm run tauri:dev
```

默认情况下，MITM 代理监听 `127.0.0.1:10999`。前端与后端通过 Tauri IPC 直接通信。

只调试浏览器中的前端界面时，可以运行：

```powershell
cd app
pnpm run dev
```

纯浏览器模式无法使用依赖 Tauri IPC 的完整后端功能。

### Windows 打包

```powershell
scripts\build_all.bat
```

脚本会安装锁定版本的前端依赖，并构建包含内嵌 Rust 后端的 MSI 安装包。输出目录：

```text
app/src-tauri/target/release/bundle/msi/
```

### macOS 打包

```bash
./scripts/build_macos.sh
```

脚本会生成 `.app` 和 `.dmg`。macOS 构建还需要 Xcode Command Line Tools 和 Tauri 对应的系统依赖。输出目录：

```text
app/src-tauri/target/release/bundle/macos/
app/src-tauri/target/release/bundle/dmg/
```

## 项目校验

Rust 后端测试：

```powershell
cargo test --manifest-path backend\Cargo.toml
```

TypeScript 与前端生产构建：

```powershell
cd app
pnpm run build
```

Tauri 后端检查：

```powershell
cargo check --manifest-path app\src-tauri\Cargo.toml
```

## 插件

相关项目：[插件市场](https://github.com/SpCoGov/shanten-lens-marketplace) · [插件 SDK](https://github.com/SpCoGov/shanten-lens-sdk)

应用内的“插件”页面提供插件市场、已安装插件和插件源管理。

- 可以从插件市场安装插件，也可以导入根目录含 `plugin.json` 的 ZIP 包。
- 插件默认需要经过启用和封包权限确认；只授予你理解且确实需要的权限。
- 支持检查更新、自动更新、禁用、重新扫描和卸载插件。
- 插件可以提供独立后端进程、前端页面、本地化资源和封包处理模块。
- 第三方插件不属于 Shanten Lens 核心代码。安装前应核对来源、权限和发布者，并自行承担运行第三方代码的风险。

手动开发或调试插件时，可以在插件页面打开插件目录，放入包含 `plugin.json` 的插件目录后执行“重新扫描”。当前插件 API 版本为 `1`。

## 常见问题

### 为什么游戏突然无法连接？

最常见的情况是Proxifier规则已经启用，但Shanten Lens未运行，或两边配置的MITM端口不一致。此时流量会被转发到没有可用代理服务的端口。

### 浏览器版一定要安装证书吗？

只要客户端、浏览器或登录链路会验证HTTPS证书，就必须信任Shanten Lens生成的本地根证书。未安装或安装到错误的证书存储区会导致TLS连接失败。

### 为什么牌山显示与实际牌山不一致？

部分护身符存在尚未完全处理的特殊情况。请查看程序“关于”页面中的“已知问题”，并谨慎使用其中列出的护身符。

### 为什么自动化后暂时无法登录？

自动化操作间隔过短可能触发临时限制。建议将操作间隔设置为至少`400ms`；如果已经受到限制，请停止自动化并等待恢复或更换网络。

### 配置和证书保存在哪里？

它们保存在操作系统提供的应用数据目录中。可以在程序设置里选择“打开配置目录”，无需手动猜测完整路径。证书位于该目录下的`ca/`。

### 这个项目会修改服务器数据吗？

程序会代理和处理游戏通信，部分功能会发送或修改请求。README只说明当前代码结构和本地功能，不对第三方服务、账号安全或平台规则作任何保证。请在充分了解风险后使用。

## 许可

项目源代码采用 [Apache License 2.0](./LICENSE) 发布。部分牌面及游戏相关素材来源于《雀魂》，仅用于非商业用途与学习研究；相关素材不因项目代码许可证而改变其原有权利归属。

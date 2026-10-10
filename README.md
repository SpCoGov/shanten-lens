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

## 连接本地青云之志项目

本版本支持直接连接 `D:/qyzz` 游戏：打开两边的新版程序，在游戏设置的“辅助设置”中开启“连接向听镜”，然后开始或继续游戏。底部出现“已连接本机青云之志”后，原有花火指导、万象指导、分数计算器与换牌序列会读取当前局。执行序列会实际换牌，并由游戏校验版本、保存和播放动画。

该接入不需要代理或证书。连接仅限同一台电脑，自动读取连接文件中的临时端口和随机连接令牌：Windows 为 `%LOCALAPPDATA%/Qyzz/shanten-lens.json`，macOS 为 `~/Library/Application Support/Qyzz/shanten-lens.json`（需要游戏与向听镜两端均更新到支持 Mac 本机发现的版本，并使用同一个系统用户运行）；再次点击游戏连接按钮或退出游戏即可断开。若同时收到本机游戏和原版封包流水线的数据，会显示全屏“数据源冲突”动画，选择其中一个来源后继续。未选来源的状态单独缓存，封包继续转发，但暂停其流水线模块和操作；来源变化会停止自动操作并清除旧换牌方案。断开一个来源后自动使用另一个，再次同时连接时重新选择。

支持读取完整牌山、换牌堆、手牌、杠、宝牌、魂牌、护身符成长和印章；操作支持换牌、结束换牌、出牌、暗杠。其他功能中的原版协议操作暂不转发到本地游戏。计算器仍使用向听镜自身规则，未声称与本地游戏所有自定义计分完全相同。

花火指导的“一键执行方案”支持换牌后继续两杠或三杠及打牌。最后一次换牌使游戏自动进入出牌阶段时直接继续；仍在换牌阶段时才发送结束换牌请求。

构建本版本：在 `app` 目录运行 `pnpm exec tauri build --no-bundle`，启动 `app/src-tauri/target/release/shanten-lens.exe`。普通旧版程序不包含此接口。

接入回归测试：后端 `cargo test -p shanten-backend --lib`；计算器在 `app` 目录运行 `node scripts/qyzz-score.test.mjs D:/qyzz/test-output/lens-state.json`（先运行游戏的 `tests/lens_bridge_test.gd` 生成数据）。真实双进程换牌测试的启动方法见游戏 README 中的“连接向听镜”。

完整方案的双进程测试：先编译后端测试，在游戏目录启动 `tests/lens_bridge_test.gd -- --serve-lens-test --hanabi-fixture`；追加 `--last-exchange` 可覆盖换牌次数耗尽，追加 `--three-quads` 切换三杠构筑。测试进程设置 `QYZZ_TEST_DISCOVERY=D:/qyzz/test-output/lens-discovery.json`、`QYZZ_TEST_ROOT` 为独立临时目录、`QYZZ_TEST_FULL_PLAN=2` 或 `3`，运行 `cargo test -p shanten-backend real_qyzz_connection_search_and_exchange -- --ignored --nocapture`。这会验证真实游戏中的换牌、结束换牌、杠牌及打牌；不设置 `QYZZ_TEST_FULL_PLAN` 则只验证换牌。

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

脚本会安装锁定版本的前端依赖，并构建当前用户安装的 NSIS 安装包及更新签名。打包前需设置下述签名环境变量。输出目录：

```text
app/src-tauri/target/release/bundle/nsis/
```

### Windows 自动更新与签名

正式安装版启动后自动检查并下载更新，签名验证通过后，在用户关闭向听镜时静默安装，下次打开即为新版。下载期间关闭程序会直接退出，下次启动重新下载，不会安装未完成的包。设置中可以关闭下次启动时的自动更新；更新窗口可以忽略已下载的版本。配置、证书和插件仍保存在原有应用数据目录中。

旧版需要手动安装一次新的 `*-setup.exe`。原 MSI 用户建议先通过 Windows 卸载旧程序，再安装新版本，避免两套安装记录；保留应用数据。之后无需手动下载或解压。开发构建不自动安装更新。

首次发布前，在 `app` 目录使用 `pnpm exec tauri signer generate -w <仓库外的私钥路径>` 生成并安全备份密钥，密码按提示设置。配置以下 GitHub Actions 变量与密钥，本地打包也使用同名环境变量：

- Repository variable `TAURI_UPDATER_PUBLIC_KEY`：公钥文件内容，编译时嵌入程序。
- Repository secret `TAURI_SIGNING_PRIVATE_KEY`：私钥文件内容（本地也可使用私钥文件路径）。
- Repository secret `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`：私钥密码。

私钥不能提交到仓库，后续版本必须继续使用同一密钥。发布工作流上传安装包、`.sig` 和 `latest.json` 到 GitHub Releases；清单缺少对应版本的安装包或签名时会拒绝发布。签名密钥未配置时 Windows 打包会失败，避免发布无法更新的安装版。仅检查代码可使用 `pnpm run build`、`cargo check` 或 `pnpm exec tauri build --no-bundle`，无需私钥。

### macOS 打包

```bash
./scripts/build_macos.sh
```

脚本会生成同时支持 Apple 芯片和 Intel 的通用版 `.app` 和 `.dmg`，并校验两种架构、应用签名及 DMG 完整性。当前使用临时签名，尚未做 Apple 公证；首次打开若被拦截，可在“系统设置 → 隐私与安全性”中选择“仍要打开”。macOS 构建还需要 Xcode Command Line Tools 和 Tauri 对应的系统依赖。输出目录：

```text
app/src-tauri/target/universal-apple-darwin/release/bundle/macos/
app/src-tauri/target/universal-apple-darwin/release/bundle/dmg/
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

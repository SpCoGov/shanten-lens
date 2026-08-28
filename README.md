# Shanten Lens 3.0

[日本語版README](./README_JA.md)

Shanten Lens是一款面向《雀魂》青云之志模式的桌面辅助工具。3.0版本采用：

- `Tauri 2+React 18+TypeScript`作为桌面前端
- 内嵌`Rust`后端负责游戏状态、自动化和业务逻辑
- `Hudsucker+Proxifier`负责HTTPS/WebSocket流量代理
- 前后端通过Tauri进程内commands和events通信

程序会在本地解析游戏状态，并提供护身符、商店候选、手牌与牌山、换牌指导、分数推演、自动化、熔断保护和调试功能。3.0不再需要Python后端、sidecar或UI WebSocket服务。

## 当前功能

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
  - 使用独立HUD窗口显示关键信息

## 使用须知

本项目仅用于教育和研究。第三方辅助、自动化和修改通信可能违反游戏服务条款，并可能导致账号限制。使用者应自行了解并承担相关风险。

目前最稳定的使用环境仍然是Windows。日常使用前需要准备：

- 可正常运行的Shanten Lens桌面程序
- `Proxifier`或具备进程级代理转发能力的同类工具
- 安装本地根证书的权限
- 能被代理工具接管的雀魂客户端或浏览器进程

首次使用时，建议先启动一次Shanten Lens再关闭，让程序生成配置、证书和数据目录。

## 用户使用说明

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

## 开发环境

### 技术栈

- 前端：`React 18`、`TypeScript`、`Vite`
- 桌面容器：`Tauri 2`
- 后端：`Rust 2021`
- MITM代理：`Hudsucker`
- 协议：`protobuf`、HTTPS和WebSocket
- 前后端通信：Tauri进程内IPC
- 包管理器：`pnpm 9`

### 目录结构

```text
.
├─ app/                 React前端与Tauri桌面应用
│  ├─ src/              前端页面、组件和业务逻辑
│  ├─ public/           护身符、印章和牌面资源
│  └─ src-tauri/        Tauri Rust入口与IPC命令
├─ backend/             内嵌Rust后端、MITM和自动化逻辑
├─ proto/               游戏协议定义
├─ scripts/             协议工具与Windows/macOS打包脚本
├─ Cargo.toml           Rust workspace
└─ README.md
```

### 安装依赖

需要安装：

- Rust stable与Cargo
- Node.js 22
- pnpm 9
- Tauri 2所需的系统依赖

安装前端依赖：

```powershell
cd app
pnpm install
```

Rust依赖会由Cargo在构建时自动下载。正常开发、构建和运行不需要Python；只有重新生成协议文件时才会使用`scripts/desc_to_proto.py`。

## 本地开发运行

Rust后端内嵌在Tauri进程中，没有单独的启动命令。

```powershell
cd app
pnpm run tauri:dev
```

默认情况下，MITM代理监听`127.0.0.1:10999`。前端与后端通过Tauri IPC直接通信，不监听旧版UI WebSocket端口。

如果只需要调试浏览器中的前端界面，可以运行：

```powershell
cd app
pnpm run dev
```

纯浏览器模式无法使用依赖Tauri IPC的完整后端功能。

## 构建与打包

### Windows一键构建

```powershell
scripts\build_all.bat
```

脚本会依次执行：

1. 使用`pnpm install --frozen-lockfile`安装前端依赖
2. 编译TypeScript和前端资源
3. 编译Tauri与内嵌Rust后端
4. 生成Windows MSI安装包

输出目录：

```text
app/src-tauri/target/release/bundle/msi/
```

### macOS构建

```bash
./scripts/build_macos.sh
```

脚本会生成`.app`和`.dmg`：

```text
app/src-tauri/target/release/bundle/macos/
app/src-tauri/target/release/bundle/dmg/
```

macOS构建还需要Xcode Command Line Tools和Tauri对应的系统依赖。

## 项目校验

Rust后端测试：

```powershell
cargo test --manifest-path backend\Cargo.toml
```

TypeScript与前端生产构建：

```powershell
cd app
pnpm run build
```

Tauri后端检查：

```powershell
cargo check --manifest-path app\src-tauri\Cargo.toml
```

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

项目源代码采用[Apache License 2.0](./LICENSE)发布。部分牌面及游戏相关素材来源于《雀魂》，仅用于非商业用途与学习研究；相关素材不因项目代码许可证而改变其原有权利归属。

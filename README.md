# Shanten Lens

[日本語版 README](./README_JA.md)

Shanten Lens 是一个面向《雀魂》青云之志模式的桌面辅助工具。项目目前采用：

- `Tauri + React + TypeScript` 作为桌面前端
- `Python` 作为后端与 MITM 桥接层
- `mitmproxy` + `Proxifier` 作为抓包与转发方案

它会在本地解析游戏状态，并提供护身符、候选、牌山、换牌、分数推演、自动化与调试相关能力。

## 当前功能

- 主界面实时显示：
  - 当前护身符
  - 商店商品 / 候补护身符
  - 手牌、牌山、换牌堆
  - 打牌建议与牌山统计
  - 当前分数、目标分数、关卡信息
- 分数计算器：
  - 按当前护身符顺序实时计算得分
  - 可编辑单个护身符的触发次数、额外触发、成长公式
  - 支持未来关卡分数推测
  - 支持“强制按传导印章计算”和“停用后续成长预测”等规则覆盖
- 黑洞 / 换牌相关页面：
  - 搜索换牌方案
  - 查看候选方案、四杠目录与被纳入搜索的牌
- 自动化页面：
  - 按目标护身符 / 印章反复执行青云之志
  - 支持重开记录与目标筛选
- 熔断与守护：
  - 对部分高风险操作进行拦截或二次确认

## 使用前提

目前最稳定的使用环境仍然是 Windows。日常使用前，你需要准备：

- 可正常运行的 Shanten Lens 桌面程序
- `Proxifier`
- 本机可安装根证书的权限
- 能被 Proxifier 接管的雀魂客户端或浏览器进程

首次使用时，建议先启动一次 Shanten Lens 再关闭，让程序生成初始配置、证书与数据目录。

## 用户使用说明

### 1. 配置 Proxifier

1. 从 [Proxifier 官网](https://www.proxifier.com/) 下载并安装 Proxifier。
2. 打开 `Profile -> Proxy Servers... -> Add...`。
3. 添加本地代理：

   - `Address`: `127.0.0.1`
   - `Port`: `10999`
   - `Protocol`: `HTTPS`

4. 如果 Proxifier 询问“是否设为默认代理”，请选择 `No`。
5. 打开 `Profile -> Proxification Rules... -> Add...`，为你的雀魂进程添加规则：

   - 客户端版通常填写 `Jantama_MahjongSoul.exe`
   - 浏览器版填写对应浏览器进程，例如 `chrome.exe`、`msedge.exe`
   - `Action` 选择刚刚创建的本地代理

### 2. 安装 mitmproxy 根证书

如果你使用浏览器版，或登录流程会校验证书，需要安装 mitmproxy 根证书：

1. 打开 `%USERPROFILE%\.mitmproxy`
2. 找到 `mitmproxy-ca-cert.cer`
3. 双击并安装到：
   - `本地计算机`
   - `受信任的根证书颁发机构`

这张证书只用于让你的本机信任本地 HTTPS 代理。

### 3. 启动顺序

推荐顺序：

1. 启动 Shanten Lens
2. 启动 Proxifier
3. 启动雀魂客户端或浏览器

注意：

- 如果 Proxifier 仍在运行，但 Shanten Lens 没有运行，游戏流量会被转发到一个不存在的本地代理，导致无法正常连接。
- 如果你只在少数场景下使用本工具，可以只在需要时启动 Proxifier。

### 4. 与 Cheat Engine 配合使用

推荐将 Shanten Lens 与 Cheat Engine 配合使用，常见用途包括：

- 对雀魂客户端进行动画加速，减少等待时间
- 在自动化运行期间将雀魂速度设为 `0`
- 加速时不宜把速度调得特别快，以免导致连接断开等问题，推荐最高为 `5`

在自动化场景下，将速度设为 `0` 时可以冻结 AFK 判定计时器。

## 开发环境

### 技术栈

- 前端：`React 18`、`TypeScript`、`Vite`
- 桌面容器：`Tauri 2`
- 后端：`Python 3.11`
- 通信：
  - 本地前后端使用 `WebSocket`
  - 游戏侧通过 `mitmproxy` 桥接

### 目录结构

```text
.
├─ app/                 Tauri + React 前端
├─ backend/             Python 后端、MITM、自动化逻辑
├─ proto/               协议与相关数据
├─ scripts/             打包脚本
├─ dist/                后端打包产物
└─ README.md
```

### 安装依赖

建议先创建并激活虚拟环境，再安装后端依赖。

后端依赖：

```powershell
python -m venv .venv
.venv\Scripts\activate
python -m pip install -r requirements.txt
```

前端依赖：

```powershell
cd app
npm install
```

## 本地开发运行

### 仅运行后端

```powershell
.venv\Scripts\python.exe backend\run_server.py
```

默认情况下：

- MITM 监听端口：`10999`
- UI WebSocket 端口：`8787`

### 仅运行前端

```powershell
cd app
npm run dev
```

### 同时运行 Tauri 前端和 Python 后端

```powershell
cd app
npm run tauri:dev-all
```

这个脚本会同时启动：

- `tauri dev`
- `backend/run_server.py`

## 构建与打包

### Windows 一键构建

仓库提供了 Windows 打包脚本：

```powershell
scripts\build_all.bat
```

它会依次完成：

1. 安装 / 校验 Python 依赖
2. 用 PyInstaller 打包后端
3. 将后端复制为 Tauri sidecar
4. 构建前端
5. 执行 `tauri build`
6. 生成便携版压缩包 `Shanten-Lens-portable.zip`

### macOS 构建

```bash
./scripts/build_macos.sh
```

该脚本会生成：

- `.app`
- `.dmg`

前提是本机已安装：

- Xcode Command Line Tools
- Rust / cargo
- Node.js / npm
- Python 虚拟环境与依赖

## 常见问题

### 为什么游戏突然连不上？

最常见原因是：

- Proxifier 已启用
- 但 Shanten Lens 后端没有运行

这时流量会被重定向到本地 `10999`，而该端口没有可用代理服务。

### 浏览器版一定要装证书吗？

不一定，但很多情况下需要。只要浏览器或登录链路会验证本地 HTTPS 代理证书，就必须安装。

### 为什么 Shanten Lens 显示的牌山和实际的牌山不一致？

Shanten Lens 没有处理每一个护身符的特殊情况。你可以查看关于页面里的“已知问题”章节确认相关问题，不推荐携带其中提到的护身符。

### 为什么使用自动化后无法登录雀魂了？

通常是自动化界面的操作间隔设置得太短。建议最少设为 `400ms`；如果设得过低，可能会导致 IP 被暂时封禁。等待一段时间，或者切换 IP 后即可重新登录。

### 这个项目会修改服务器数据吗？

README 只描述当前代码结构与本地功能，不对第三方服务、账号风控或平台规则作任何保证。请自行判断使用风险。

## 许可

本仓库当前包含 [LICENSE](./LICENSE)。如你要分发、二次开发或打包发布，建议先阅读许可证文本，并自行确认第三方依赖与游戏相关资源的使用边界。

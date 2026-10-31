测试数据说明：

- `playbook.json`：合成的全流程与刷开局配置，供前后端共享校验、目标匹配和配置存储测试使用，不含用户配置。
- `provider-events.json`：从仓库的 `provider.md` 提取的开局、角色选择、初始护身符、进入对局和交易响应，用于重放状态更新。
- `mini-dragon-opening.json`：2026-10-31 迷你龙护符触发后的开局报文裁剪，保留牌池、实际手牌和公开待摸牌的位置，用于校验事件更新与刷新后的分区顺序；不含账号数据。
- `windstopper-opening.json`：2026-10-31 风停了锁定并后移风牌的真实报文裁剪，包含开局、5 次换牌和结束换牌后的首张摸牌，用于校验效果返回的实际牌山、增量更新与执行器的摸牌预期；不含账号数据。
- `souzu_v241.json`：原有换牌算法回归样本，由 `../generate_souzu_v241.py` 生成。
- `direct-cn-web-manifest.bundle`、`direct-cn-web-version.bundle`：2026-10-23 的官方公开 CN Web 客户端资源样本，沿用 `majsoul-ws-login` 的测试样本。清单保留原始 Unity 序列化结构、前 5 个 bundle 和 `MyAssets/docs_version/` 的两个文件映射并以 LZ4 保存；版本包包含 `0.16.289 / WebGL`。两者仅用于离线解析和截断校验，不含账号数据。

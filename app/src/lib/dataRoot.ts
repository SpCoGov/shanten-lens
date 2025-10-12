import { appDataDir, join } from "@tauri-apps/api/path";
import { exists, mkdir } from "@tauri-apps/plugin-fs";

/** 返回数据根；若 FS 不可用或权限不足，返回空串表示“走本地存储兜底”。 */
export async function dataRoot(): Promise<string> {
    try {
        const base = await appDataDir();
        const dir = await join(base, "shanten");
        const ok = await exists(dir).catch(() => false);
        if (!ok) {
            await mkdir(dir, { recursive: true });
        }
        return dir;
    } catch {
        // plugin 未注册 / 权限未开 / dev url 场景下都走兜底
        return "";
    }
}
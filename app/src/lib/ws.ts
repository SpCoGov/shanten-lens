import {useLogStore} from "./logStore";
import {setRegistry, type RegistryPayload} from "./registryStore";
import {setFuseConfig, type FuseConfig} from "./fuseStore";
import {AutoRunnerConfig, setAutoConfig} from "./autoRunnerStore";

export type UpdateConfigPacket = { type: "update_config"; data: Record<string, Record<string, any>> };
export type Packet =
    | UpdateConfigPacket
    | { type: "keep_alive"; data: {} }
    | { type: "edit_config"; data: any }
    | { type: "request_update"; data: {} }
    | { type: "open_config_dir"; data: {} }
    | { type: "open_result"; data: { ok: boolean; error?: string } }
    | { type: string; data: any };

type PacketHandler = (pkt: Packet) => void;
type RawHandler = (raw: string) => void;
type VoidFn = () => void;

function ts() {
    const d = new Date();
    const t = [d.getHours(), d.getMinutes(), d.getSeconds()].map((n) => n.toString().padStart(2, "0")).join(":");
    return `${t}.${d.getMilliseconds().toString().padStart(3, "0")}`;
}

const addFrame = useLogStore.getState().addFrame;

class WS {
    private url: string;
    private ws: WebSocket | null = null;

    connected = false;

    // 事件订阅
    private packetHandlers = new Set<PacketHandler>();
    private rawHandlers = new Set<RawHandler>();
    private openHandlers = new Set<VoidFn>();
    private closeHandlers = new Set<VoidFn>();

    // 心跳
    private keepTimer: any = null;

    constructor(url: string) {
        this.url = url.replace(/^http/, "ws");
    }

    on(h: PacketHandler): VoidFn {
        this.packetHandlers.add(h);
        return () => this.packetHandlers.delete(h);
    }

    onPacket(h: PacketHandler): VoidFn {
        this.packetHandlers.add(h);
        return () => this.packetHandlers.delete(h);
    }

    onRaw(h: RawHandler): VoidFn {
        this.rawHandlers.add(h);
        return () => this.rawHandlers.delete(h);
    }

    onOpen(h: VoidFn): VoidFn {
        this.openHandlers.add(h);
        return () => this.openHandlers.delete(h);
    }

    onClose(h: VoidFn): VoidFn {
        this.closeHandlers.add(h);
        return () => this.closeHandlers.delete(h);
    }

    connect() {
        if (this.ws) return;

        const url = this.url.endsWith("/ws") ? this.url : this.url + "/ws";
        const ws = new WebSocket(url);
        this.ws = ws;

        ws.onopen = () => {
            this.connected = true;
            console.log(`[WS ${ts()}] connected`);
            this._startKeepAlive();
            // 通知所有 onOpen 订阅者
            this.openHandlers.forEach((fn) => {
                try {
                    fn();
                } catch (e) {
                    console.warn("onOpen handler error:", e);
                }
            });
        };

        ws.onmessage = (ev) => {
            const raw = typeof ev.data === "string" ? ev.data : String(ev.data);
            addFrame("in", raw);

            // 先广播原始消息
            this.rawHandlers.forEach((fn) => {
                try {
                    fn(raw);
                } catch (e) {
                    console.warn("onRaw handler error:", e);
                }
            });

            // 再尝试解析为 JSON Packet
            try {
                const pkt = JSON.parse(raw) as Packet;
                this.packetHandlers.forEach((h) => {
                    try {
                        h(pkt);
                    } catch (e) {
                        console.warn("packet handler error:", e);
                    }
                });
                console.log(`[WS ${ts()}] recv:`, pkt);
            } catch {
                console.warn(`[WS ${ts()}] recv (non-JSON):`, raw);
            }
        };

        ws.onclose = () => {
            this.connected = false;
            console.log(`[WS ${ts()}] closed`);
            this.ws = null;
            this._stopKeepAlive();

            // 通知 onClose
            this.closeHandlers.forEach((fn) => {
                try {
                    fn();
                } catch (e) {
                    console.warn("onClose handler error:", e);
                }
            });

            // 简单重连
            setTimeout(() => this.connect(), 1000);
        };

        ws.onerror = (e) => {
            console.warn(`[WS ${ts()}] error`, e);
            try {
                ws.close();
            } catch {
            }
        };
    }

    send(pkt: Packet) {
        const data = JSON.stringify(pkt);
        addFrame("out", data);
        console.log(`[WS ${ts()}] send:`, data);
        if (this.ws && this.connected) {
            try {
                this.ws.send(data);
            } catch (e) {
                console.warn("[WS] send failed:", e);
            }
        }
    }

    private _startKeepAlive() {
        this._stopKeepAlive();
        this.keepTimer = setInterval(() => {
            this.send({type: "keep_alive", data: {}});
        }, 5000);
    }

    private _stopKeepAlive() {
        if (this.keepTimer) clearInterval(this.keepTimer);
        this.keepTimer = null;
    }
}

export const ws = new WS("http://127.0.0.1:8787");
ws.onPacket((pkt) => {
    if (pkt.type === "update_registry") {
        const data = pkt.data as RegistryPayload;
        if (data && Array.isArray(data.amulets) && Array.isArray(data.badges)) {
            setRegistry(data);
        }
    }
});

ws.onPacket((pkt) => {
    if (pkt.type === "update_fuse_config") {
        const cfg = pkt.data as FuseConfig;
        if (cfg && cfg.guard_skip_contains) {
            setFuseConfig(cfg);
        }
    } else if (pkt.type === "update_autorun_config") {
        const cfg = pkt.data as AutoRunnerConfig;
        if (cfg) {
            setAutoConfig(cfg);
        }
    } else if (pkt.type === "autorun_control_result") {
        return;
    }
});
import { useLogStore } from "./logStore";

export type UpdateConfigPacket = { type: "update_config"; data: Record<string, Record<string, any>> };
export type Packet =
    | UpdateConfigPacket
    | { type: "keep_alive"; data: {} }
    | { type: "edit_config"; data: any }
    | { type: "request_update"; data: {} }
    | { type: "open_config_dir"; data: {} }
    | { type: "open_result"; data: { ok: boolean; error?: string } }
    | { type: string; data: any }; // 兼容未来扩展

type Handler = (pkt: Packet) => void;

function ts() {
    const d = new Date();
    const t = [d.getHours(), d.getMinutes(), d.getSeconds()]
        .map((n) => n.toString().padStart(2, "0"))
        .join(":");
    return `${t}.${d.getMilliseconds().toString().padStart(3, "0")}`;
}

const addFrame = useLogStore.getState().addFrame;

class WS {
    private url: string;
    private ws: WebSocket | null = null;
    private handlers = new Set<Handler>();
    connected = false;
    private keepTimer: any = null;

    constructor(url: string) {
        this.url = url.replace(/^http/, "ws");
    }

    on(h: Handler) {
        this.handlers.add(h);
        return () => this.handlers.delete(h);
    }

    connect() {
        if (this.ws) return;
        const ws = new WebSocket(this.url.endsWith("/ws") ? this.url : this.url + "/ws");
        this.ws = ws;

        ws.onopen = () => {
            this.connected = true;
            console.log(`[WS ${ts()}] ✅ connected`);
            this._startKeepAlive();
        };
        ws.onmessage = (ev) => {
            const raw = typeof ev.data === "string" ? ev.data : String(ev.data);
            addFrame("in", raw);
            try {
                console.log(`[WS ${ts()}] ⬅ recv:`, raw);
                const pkt = JSON.parse(raw);
                this.handlers.forEach((h) => h(pkt));
            } catch (e) {
                console.warn(`[WS ${ts()}] ⬅ recv (non-JSON):`, raw);
            }
        };
        ws.onclose = () => {
            this.connected = false;
            console.log(`[WS ${ts()}] ❌ closed`);
            this.ws = null;
            this._stopKeepAlive();
            setTimeout(() => this.connect(), 1000);
        };
        ws.onerror = (e) => {
            console.warn(`[WS ${ts()}] ⚠ error`, e);
            try { ws.close(); } catch {}
        };
    }

    send(pkt: Packet) {
        const data = JSON.stringify(pkt);
        addFrame("out", data);
        console.log(`[WS ${ts()}] ➡ send:`, data);
        if (this.ws && this.connected) this.ws.send(data);
    }

    private _startKeepAlive() {
        this._stopKeepAlive();
        this.keepTimer = setInterval(() => {
            this.send({ type: "keep_alive", data: {} });
        }, 5000);
    }
    private _stopKeepAlive() {
        if (this.keepTimer) clearInterval(this.keepTimer);
        this.keepTimer = null;
    }
}

// 如需可配置，替换成读取配置的地址
export const ws = new WS("http://127.0.0.1:8787");
import {useSyncExternalStore} from "react";

export type TargetBadge = { kind: "badge"; id: number };
export type TargetAmulet = { kind: "amulet"; id: number; plus?: boolean; badge?: number | null };
export type TargetItem = TargetBadge | TargetAmulet;

export type AutoRunnerConfig = {
    end_count: number;
    targets: TargetItem[];
    cutoff_level?: number;
    __tick__?: number; // 轻量刷新
    op_interval_ms?: number;
    email_notify?: EmailNotifyConfig;
};

export type EmailNotifyConfig = {
    enabled: boolean;
    host: string;   // SMTP 服务器
    port: number;   // 端口
    ssl: boolean;   // SSL/TLS
    from: string;   // 发件邮箱（同时作为认证用户名）
    pass: string;   // 密码/授权码
    to: string;     // 收件邮箱
};

export type AutoRunnerStatus = {
    mode?: "continuous" | "step";
    running: boolean;
    runs: number;
    elapsed_ms: number;
    best_achieved_count: number;
    current_step?: string;
    last_error?: string;
    started_at?: number;

    game_ready?: boolean;
    has_live_game?: boolean;
    game_ready_reason?: string;
    game_ready_code?: "BUSINESS_REFUSED" | "GAME_NOT_READY" | "PROBE_TIMEOUT" | "" | "NOT_PROBED";
    probe_fail_count?: number;
    preferred_flow_ready?: boolean;
    preferred_flow_peer?: string;
};

const defaultConfig: AutoRunnerConfig = {
    end_count: 1,
    targets: [],
    cutoff_level: 102,
    op_interval_ms: 1000,
    email_notify: {
        enabled: false,
        host: "",
        port: 587,
        ssl: false,
        from: "",
        pass: "",
        to: "",
    },
};

const defaultStatus: AutoRunnerStatus = {
    mode: "continuous",
    running: false,
    runs: 0,
    elapsed_ms: 0,
    best_achieved_count: 0,
    current_step: "-",
    last_error: "",
    started_at: 0,
    game_ready: false,
    has_live_game: false,
    game_ready_reason: "未探测，请点击“刷新状态”",
    game_ready_code: "NOT_PROBED",
    probe_fail_count: 0,
};

type State = {
    config: AutoRunnerConfig;
    status: AutoRunnerStatus;
};

let state: State = {
    config: defaultConfig,
    status: defaultStatus,
};

const subs = new Set<() => void>();
const emit = () => subs.forEach((fn) => fn());

export function setAutoConfig(cfg: AutoRunnerConfig) {
    state = {...state, config: cfg ?? defaultConfig};
    emit();
}

export function patchAutoConfig(patch: Partial<AutoRunnerConfig>) {
    state = {...state, config: {...state.config, ...patch}};
    emit();
}

export function setAutoStatus(newStatus: AutoRunnerStatus) {
    state = {...state, status: {...defaultStatus, ...(newStatus ?? {})}};
    emit();
}

export function addTargetAmulet(item: { id: number; plus?: boolean; badge?: number | null }) {
    const next: TargetItem = {kind: "amulet", id: item.id, plus: !!item.plus, badge: item.badge ?? null};
    state = {...state, config: {...state.config, targets: [...state.config.targets, next]}};
    emit();
}

export function addTargetBadge(badgeId: number) {
    const next: TargetItem = {kind: "badge", id: badgeId};
    state = {...state, config: {...state.config, targets: [...state.config.targets, next]}};
    emit();
}

export function removeTargetAt(index: number) {
    const arr = state.config.targets.slice();
    if (index >= 0 && index < arr.length) arr.splice(index, 1);
    state = {...state, config: {...state.config, targets: arr}};
    emit();
}

export function replaceTargetAt(index: number, item: TargetItem) {
    const arr = state.config.targets.slice();
    if (index >= 0 && index < arr.length) arr[index] = item;
    state = {...state, config: {...state.config, targets: arr}};
    emit();
}

export function parseLevelText(s: string): number | null {
    const m = /^\s*([1-5])\s*-\s*([1-3])\s*$/.exec(s);
    if (!m) return null;
    const a = Number(m[1]), b = Number(m[2]);
    return a * 100 + b;
}

export function formatLevelNum(n?: number): string {
    if (!n || n <= 0) return "";
    const a = Math.floor(n / 100);
    const b = n % 100;
    return `${a}-${b}`;
}

export function useAutoRunner() {
    return useSyncExternalStore(
        (fn) => {
            subs.add(fn);
            return () => subs.delete(fn);
        },
        () => state
    );
}
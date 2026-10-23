import {useSyncExternalStore} from "react";
import {emptyAutoPlayConfig, type AutoPlayConfig, type OpeningPlayProfile} from "./autoPlayConfig";

export type TargetBadge = { kind: "badge"; id: number; value?: number };
export type TargetAmulet = { kind: "amulet"; id: number; plus?: boolean; badge?: number | null; value?: number };
export type TargetItem = TargetBadge | TargetAmulet;

export type AutoRunnerConfig = {
    playbook: AutoPlayConfig;
    op_interval_ms?: number;
    record_detailed_operations?: boolean;
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

export type AutoRunnerRemakeRecord = {
    seq: number;
    run_index: number;
    ts: number;
    reason: string;
    stage?: number | null;
    level?: number | null;
    target_value: number;
    amulet_count: number;
    effect_list: any[];
    operation_count?: number;
    operation_records?: AutoRunnerOperationRecord[];
};

export type AutoRunnerOperationRecord = {
    seq: number;
    run_index: number;
    op_index: number;
    ts: number;
    stage?: number | null;
    level?: number | null;
    step: string;
    action: string;
    reason: string;
    result: string;
    details?: Record<string, unknown>;
};

export type AutoRunnerStatus = {
    active_profile?: OpeningPlayProfile | null;
    running: boolean;
    runs: number;
    elapsed_ms: number;
    best_achieved_count: number;
    current_achieved_count?: number | null;
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
    remake_records?: AutoRunnerRemakeRecord[];
    best_remake_record?: AutoRunnerRemakeRecord | null;
    record_detailed_operations?: boolean;
    operation_records?: AutoRunnerOperationRecord[];
    operation_count_by_run?: Record<string, number>;
};

const defaultConfig: AutoRunnerConfig = {
    playbook: emptyAutoPlayConfig,
    op_interval_ms: 1000,
    record_detailed_operations: false,
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
    active_profile: null,
    running: false,
    runs: 0,
    elapsed_ms: 0,
    best_achieved_count: 0,
    current_achieved_count: 0,
    current_step: "-",
    last_error: "",
    started_at: 0,
    game_ready: false,
    has_live_game: false,
    game_ready_reason: "",
    game_ready_code: "NOT_PROBED",
    probe_fail_count: 0,
    remake_records: [],
    best_remake_record: null,
    record_detailed_operations: false,
    operation_records: [],
    operation_count_by_run: {},
};

type State = {
    config: AutoRunnerConfig;
    status: AutoRunnerStatus;
    playbookDirty: boolean;
};

let state: State = {
    config: defaultConfig,
    status: defaultStatus,
    playbookDirty: false,
};

const subs = new Set<() => void>();
const emit = () => subs.forEach((fn) => fn());

export function setAutoConfig(cfg: AutoRunnerConfig) {
    state = {
        ...state,
        config: {
            ...defaultConfig,
            ...(cfg ?? defaultConfig),
            ...(state.playbookDirty ? {playbook: state.config.playbook} : {}),
        },
    };
    emit();
}

export function patchAutoConfig(patch: Partial<AutoRunnerConfig>) {
    state = {
        ...state,
        playbookDirty: state.playbookDirty || patch.playbook !== undefined,
        config: {
            ...state.config,
            ...patch,
        },
    };
    emit();
}

export function markPlaybookSaved(saved: AutoPlayConfig) {
    if (state.config.playbook !== saved) return;
    state = {...state, playbookDirty: false};
    emit();
}

export const getAutoRunnerSnapshot = () => state;

export function setAutoStatus(newStatus: AutoRunnerStatus) {
    state = {...state, status: {...defaultStatus, ...(newStatus ?? {})}};
    emit();
}

export function useAutoRunner() {
    return useSyncExternalStore(
        (fn) => {
            subs.add(fn);
            return () => subs.delete(fn);
        },
        getAutoRunnerSnapshot
    );
}

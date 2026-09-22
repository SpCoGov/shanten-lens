import React from "react";
import {
    Responsive,
    useContainerWidth,
    verticalCompactor,
    type Layout,
    type ResponsiveLayouts,
} from "react-grid-layout";
import {useTranslation} from "react-i18next";
import * as backendIpc from "../lib/ipc";
import type {PacketModuleInfo, PacketOperation, PipelineConfig, PipelineModule} from "../lib/ipc";
import protoMethods from "virtual:liqi-methods";
import "react-grid-layout/css/styles.css";
import styles from "./PacketPipelinePage.module.css";

const PROTO_METHOD_SET = new Set(protoMethods);
const OPERATION_ORDER: PacketOperation[] = ["read", "edit", "drop", "bypass", "inject"];
const GRID_ROW_HEIGHT = 8;
const GRID_GAP = 8;

const FALLBACK: PipelineConfig = {
    schema: 1,
    modules: [
        {id: "replay_injector", enabled: true, options: {}},
        {id: "method_filter", enabled: true, options: {bypass_methods: [".lq.Route.heartbeat"]}},
        {id: "game_record", enabled: true, options: {}},
        {id: "packet_logger", enabled: true, options: {}},
        {id: "game_state", enabled: true, options: {}},
        {id: "unlock_illustrated_book", enabled: true, options: {}},
        {id: "fuse_rules", enabled: true, options: {}},
        {id: "autorun", enabled: true, options: {}},
    ],
};
const BUILTIN_MODULE_IDS = new Set(FALLBACK.modules.map((module) => module.id));

const moduleHeight = (id: string, subscriptions = 0) => id === "method_filter"
    ? 20
    : 10 + Math.ceil(subscriptions / 2);

function bypassMethods(module: PipelineModule): string[] {
    if (Array.isArray(module.options?.bypass_methods)) {
        return module.options.bypass_methods.filter((method): method is string => typeof method === "string");
    }
    return [];
}

function createLayout(modules: PipelineModule[], moduleInfo: Map<string, PacketModuleInfo>, measuredRows: Record<string, number>): Layout {
    let y = 0;
    return modules.map((module) => {
        const h = measuredRows[module.id] ?? moduleHeight(module.id, moduleInfo.get(module.id)?.subscriptions.length);
        const item = {i: module.id, x: 0, y, w: 1, h, minW: 1, maxW: 1, minH: h, maxH: h};
        y += h;
        return item;
    });
}

function MeasuredBody({id, onHeight, children}: {
    id: string;
    onHeight(id: string, height: number): void;
    children: React.ReactNode;
}) {
    const ref = React.useRef<HTMLDivElement>(null);
    React.useLayoutEffect(() => {
        const body = ref.current;
        if (!body) return;
        const measure = () => onHeight(id, body.scrollHeight);
        const observer = new ResizeObserver(measure);
        observer.observe(body);
        measure();
        return () => observer.disconnect();
    }, [id, onHeight]);
    return <div ref={ref} className={styles.body}>{children}</div>;
}

export default function PacketPipelinePage() {
    const {t} = useTranslation();
    const [config, setConfig] = React.useState<PipelineConfig>(FALLBACK);
    const [loaded, setLoaded] = React.useState(false);
    const [saved, setSaved] = React.useState(false);
    const [methodDraft, setMethodDraft] = React.useState("");
    const [modules, setModules] = React.useState<PacketModuleInfo[]>([]);
    const [measuredRows, setMeasuredRows] = React.useState<Record<string, number>>({});
    const methodQuery = methodDraft.trim().toLowerCase();
    const configuredMethods = React.useMemo(() => {
        const methodFilter = config.modules.find((module) => module.id === "method_filter");
        return new Set(methodFilter ? bypassMethods(methodFilter) : []);
    }, [config.modules]);
    const methodSuggestions = React.useMemo(
        () => methodQuery.length < 5
            ? []
            : protoMethods.filter((method) => !configuredMethods.has(method) && method.toLowerCase().includes(methodQuery)).slice(0, 20),
        [configuredMethods, methodQuery],
    );
    const {width, containerRef, mounted} = useContainerWidth();
    const moduleInfo = React.useMemo(() => new Map(modules.map((module) => [module.id, module])), [modules]);
    const layout = React.useMemo(() => createLayout(config.modules, moduleInfo, measuredRows), [config.modules, measuredRows, moduleInfo]);
    const acceptHeight = React.useCallback((id: string, height: number) => {
        const rows = Math.max(1, Math.ceil((height + GRID_GAP) / (GRID_ROW_HEIGHT + GRID_GAP)));
        setMeasuredRows((current) => current[id] === rows ? current : {...current, [id]: rows});
    }, []);

    React.useEffect(() => {
        const accept = (next: PipelineConfig) => {
            setConfig({...next, modules: next.modules.map((module) => ({...module, options: module.options ?? {}}))});
            setLoaded(true);
        };
        void backendIpc.getPacketPipeline().then(accept, () => setLoaded(true));
        void backendIpc.getPacketModules().then(setModules);
        const refreshStats = window.setInterval(() => void backendIpc.getPacketModules().then(setModules), 1000);
        const stopPipeline = backendIpc.subscribeBackendEvent("packet_pipeline", accept);
        const stopModules = backendIpc.subscribeBackendEvent("packet_modules", setModules);
        return () => {
            window.clearInterval(refreshStats);
            stopPipeline();
            stopModules();
        };
    }, []);

    const updateModule = (index: number, update: Partial<PipelineModule>) => {
        setSaved(false);
        setConfig((current) => ({...current, modules: current.modules.map((module, i) => i === index ? {...module, ...update} : module)}));
    };

    const updateBypassMethods = (index: number, module: PipelineModule, methods: string[]) => {
        updateModule(index, {options: {...(module.options ?? {}), bypass_methods: methods}});
    };

    const addBypassMethod = (index: number, module: PipelineModule, value = methodDraft) => {
        const method = value.trim();
        const methods = bypassMethods(module);
        if (!method || methods.includes(method)) return;
        updateBypassMethods(index, module, [...methods, method]);
        setMethodDraft("");
    };

    const reorder = (next: Layout) => {
        const order = new Map([...next].sort((a, b) => a.y - b.y || a.x - b.x).map((item, index) => [item.i, index]));
        setConfig((current) => ({
            ...current,
            modules: [...current.modules].sort((a, b) => (order.get(a.id) ?? 0) - (order.get(b.id) ?? 0)),
        }));
        setSaved(false);
    };

    const reset = () => {
        const defaults = structuredClone(FALLBACK);
        defaults.modules.push(...structuredClone(config.modules.filter((module) => !BUILTIN_MODULE_IDS.has(module.id))));
        setConfig(defaults);
        setSaved(false);
        void backendIpc.setPacketPipeline(defaults).then((result) => setSaved(result.ok));
    };

    return (
        <div className={styles.page}>
            <header className={styles.header}>
                <div>
                    <h1>{t("packet_pipeline.title")}</h1>
                </div>
                <div className={styles.actions}>
                    {saved ? <span className={styles.saved}>{t("packet_pipeline.saved")}</span> : null}
                    <button className="btn ghost" onClick={reset}>
                        {t("packet_pipeline.reset")}
                    </button>
                    <button className="nav-btn" onClick={() => void backendIpc.setPacketPipeline(config).then((result) => setSaved(result.ok))}>
                        {t("packet_pipeline.save")}
                    </button>
                </div>
            </header>
            <div className={styles.flow} ref={containerRef as React.Ref<HTMLDivElement>}>
                {mounted && loaded ? (
                    <Responsive
                        width={width}
                        breakpoints={{lg: 0}}
                        cols={{lg: 1}}
                        rowHeight={GRID_ROW_HEIGHT}
                        margin={[0, GRID_GAP]}
                        containerPadding={[0, 0]}
                        layouts={{lg: layout} as ResponsiveLayouts}
                        dragConfig={{cancel: "button, a, input, textarea, select, option, label, [contenteditable='true']"}}
                        resizeConfig={{enabled: false}}
                        compactor={verticalCompactor}
                        onDragStop={reorder}
                    >
                        {config.modules.map((module, index) => {
                            const methods = bypassMethods(module);
                            const info = moduleInfo.get(module.id);
                            const operations = OPERATION_ORDER.filter((operation) =>
                                info?.subscriptions.some((subscription) => subscription.operations.includes(operation)),
                            );
                            const title = info?.builtin
                                ? t(`packet_pipeline.modules.${module.id}.title`, {defaultValue: info.name})
                                : info?.name ?? module.id;
                            const description = info?.builtin
                                ? t(`packet_pipeline.modules.${module.id}.description`, {defaultValue: info.description ?? ""})
                                : info?.description ?? "";
                            return (
                                <article
                                    key={module.id}
                                    data-module={module.id}
                                    className={`${styles.module} ${module.enabled ? "" : styles.disabled}`}
                                >
                                    <MeasuredBody id={module.id} onHeight={acceptHeight}>
                                        <div className={styles.moduleHeader}>
                                            <div className={styles.moduleTitle}>
                                                <span className={`ms ${styles.dragHandle}`} aria-hidden="true">drag_indicator</span>
                                                <strong>{title}</strong>
                                            </div>
                                            <label className={styles.toggle}>
                                                <input
                                                    type="checkbox"
                                                    checked={module.enabled}
                                                    onChange={(event) => updateModule(index, {enabled: event.target.checked})}
                                                    aria-label={t("packet_pipeline.module_toggle", {module: title})}
                                                />
                                                <span className={styles.toggleTrack} aria-hidden="true"><span/></span>
                                                <span className={styles.toggleLabel}>{module.enabled ? t("packet_pipeline.enabled") : t("packet_pipeline.disabled")}</span>
                                            </label>
                                        </div>
                                        <div className={styles.moduleMeta}>
                                            <span>{t("packet_pipeline.provider", {provider: info?.builtin ? t("packet_pipeline.builtin_provider") : info?.providerId ?? "-"})}</span>
                                            <span className={info?.online ? styles.online : styles.offline}>
                                                {info?.online ? t("packet_pipeline.online") : t("packet_pipeline.offline")}
                                            </span>
                                        </div>
                                        {info ? (
                                            <div className={styles.stats}>
                                                {t("packet_pipeline.stats", {
                                                    count: info.invocationCount,
                                                    average: info.invocationCount
                                                        ? (info.totalDurationUs / info.invocationCount / 1000).toFixed(2)
                                                        : "0.00",
                                                    timeouts: info.timeoutCount,
                                                    dropped: info.droppedReadCount,
                                                })}
                                            </div>
                                        ) : null}
                                        {operations.length ? (
                                            <div className={styles.operations}>
                                                {operations.map((operation) => (
                                                    <span key={operation} className={styles.operation} data-operation={operation}>
                                                        {t(`packet_pipeline.operations.${operation}`)}
                                                    </span>
                                                ))}
                                            </div>
                                        ) : null}
                                        {description ? <p>{description}</p> : null}
                                        {module.id !== "method_filter" && info?.subscriptions.length ? (
                                            <div className={styles.subscriptions}>
                                                {info.subscriptions.map((subscription, subscriptionIndex) => (
                                                    <div className={styles.subscription} key={`${subscription.direction ?? "*"}:${subscription.type ?? "*"}:${subscription.method ?? "*"}:${subscriptionIndex}`}>
                                                        <span>{subscription.direction ? t(`packet_pipeline.directions.${subscription.direction}`) : t("packet_pipeline.any_direction")}</span>
                                                        <span>{subscription.type ?? t("packet_pipeline.any_type")}</span>
                                                        <code>{subscription.method ?? t("packet_pipeline.any_method")}</code>
                                                        <span className={styles.subscriptionOperations}>
                                                            {OPERATION_ORDER.filter((operation) => subscription.operations.includes(operation)).map((operation) => (
                                                                <em key={operation} data-operation={operation}>{t(`packet_pipeline.operations.${operation}`)}</em>
                                                            ))}
                                                        </span>
                                                    </div>
                                                ))}
                                            </div>
                                        ) : null}
                                        {module.id === "method_filter" ? (
                                            <div className={styles.methodEditor}>
                                                <div className={styles.methodEditorHeader}>
                                                    <span>{t("packet_pipeline.bypass_methods")}</span>
                                                    <span className={styles.methodCount}>{t("packet_pipeline.method_count", {count: methods.length})}</span>
                                                </div>
                                                <div className={styles.methodInputRow}>
                                                    <input
                                                        value={methodDraft}
                                                        onChange={(event) => {
                                                            const value = event.target.value;
                                                            setMethodDraft(value);
                                                            if (PROTO_METHOD_SET.has(value)) addBypassMethod(index, module, value);
                                                        }}
                                                        list={methodQuery.length >= 5 ? "proto-method-suggestions" : undefined}
                                                        autoComplete="off"
                                                        onKeyDown={(event) => {
                                                            if (event.key !== "Enter" || methodSuggestions.length > 0) return;
                                                            event.preventDefault();
                                                            addBypassMethod(index, module);
                                                        }}
                                                        placeholder=".lq.Service.method"
                                                        aria-label={t("packet_pipeline.method_input")}
                                                    />
                                                    <datalist id="proto-method-suggestions">
                                                        {methodSuggestions.map((method) => <option value={method} key={method}/>)}
                                                    </datalist>
                                                    <button type="button" onClick={() => addBypassMethod(index, module)} disabled={!methodDraft.trim() || methods.includes(methodDraft.trim())}>
                                                        <span className="ms" aria-hidden="true">add</span>
                                                        {t("packet_pipeline.add_method")}
                                                    </button>
                                                </div>
                                                {methodQuery.length >= 5 && methodSuggestions.length === 0 ? (
                                                    <div className={styles.autocompleteHint}>{t("packet_pipeline.no_method_matches")}</div>
                                                ) : null}
                                                <div className={styles.methodList}>
                                                    {methods.length ? methods.map((method) => (
                                                        <div className={styles.methodChip} key={method}>
                                                            <code>{method}</code>
                                                            <button
                                                                type="button"
                                                                onClick={() => updateBypassMethods(index, module, methods.filter((item) => item !== method))}
                                                                aria-label={t("packet_pipeline.remove_method", {method})}
                                                            >
                                                                <span className="ms" aria-hidden="true">close</span>
                                                            </button>
                                                        </div>
                                                    )) : <div className={styles.methodEmpty}>{t("packet_pipeline.no_methods")}</div>}
                                                </div>
                                            </div>
                                        ) : null}
                                    </MeasuredBody>
                                </article>
                            );
                        })}
                    </Responsive>
                ) : null}
            </div>
        </div>
    );
}

import React, {act} from "react";
import {createRoot} from "react-dom/client";
import {mockIPC, clearMocks} from "@tauri-apps/api/mocks";
import i18n from "i18next";
import {initReactI18next} from "react-i18next";
import ActionProviderPanel from "../src/components/ActionProviderPanel";
import AutoRunnerInventory from "../src/components/AutoRunnerInventory";
import {useActionExecution} from "../src/lib/useActionExecution";
import type {ActionCandidate, ActionExecutionRequest, CommandResult} from "../src/bindings";
import type {GameStateData} from "../src/lib/gamestate";
import zh from "../src/locales/zh-CN.json";

// Run in a browser; every action is intercepted by the Tauri IPC mock.
export async function runActionExecutionTests() {
    await i18n.use(initReactI18next).init({lng: "zh-CN", resources: {"zh-CN": {translation: zh}}});
    const assert = (value: unknown, message: string) => {if (!value) throw new Error(message);};
    const requests: ActionExecutionRequest[] = [];
    let resolve!: (result: CommandResult) => void;
    let reject!: (error: Error) => void;
    mockIPC((command, payload) => {
        if (command !== "backend_execute_action") throw new Error(`Unexpected IPC: ${command}`);
        requests.push((payload as {request: ActionExecutionRequest}).request);
        return new Promise<CommandResult>((ok, fail) => {resolve = ok; reject = fail;});
    });
    const remake: ActionCandidate = {id: "remake:{}", kind: "remake", params: {}, cost: null, cost_resource: "coin", availability: "available", reasons: []};
    const initial: GameStateData = {
        session_id: 8, revision: 12, stage: 5, coin: "0", level: 13001, node: 1,
        deck_map: {}, hand_tiles: [], dora_tiles: [], replacement_tiles: [], wall_tiles: [],
        desktop_remain: 0, locked_tiles: [], switch_used_tiles: [],
        action_report: {stage: 5, interrupt: 0, status: "battle_delegated", actions: [], global_actions: [remake], issues: []},
    };
    let update!: React.Dispatch<React.SetStateAction<GameStateData | null>>;
    function Harness() {
        const [state, setState] = React.useState<GameStateData | null>(initial);
        update = setState;
        const execution = useActionExecution(state);
        return <><AutoRunnerInventory state={state} execution={execution}/><ActionProviderPanel state={state} execution={execution}/></>;
    }
    const container = document.createElement("div");
    document.body.append(container);
    const root = createRoot(container);
    const quick = () => container.querySelector<HTMLButtonElement>("[aria-labelledby='autorun-amulets'] button")!;
    const lower = () => container.querySelector<HTMLButtonElement>("li button")!;
    const nextRevision = () => update(state => ({...state!, revision: state!.revision! + 1}));
    try {
        await act(async () => {root.render(<Harness/>);});
        assert(!quick().disabled && !lower().disabled, "both remake entries should be available");
        await act(async () => {quick().click(); lower().click();});
        assert(requests.length === 1, "clicking both entries must send only one request");
        assert(JSON.stringify(requests[0]) === JSON.stringify({action_id: remake.id, session_id: 8, revision: 12}), "shortcut must send the current action and state identifiers");
        assert(quick().disabled && lower().disabled, "both entries must be disabled while sending");
        await act(async () => {resolve({ok: true});});
        assert(quick().disabled && lower().disabled, "success must wait for the next state before allowing another action");
        await act(async () => {nextRevision();});
        assert(!quick().disabled && !lower().disabled, "the next revision must unlock both entries");
        await act(async () => {lower().click(); quick().click();});
        assert(requests.length === 2 && requests[1].revision === 13, "the lower entry must share the same execution lock and latest revision");
        await act(async () => {reject(new Error("connection lost"));});
        assert(quick().disabled && lower().disabled, "an uncertain send must block both entries until state updates");
        assert(container.textContent?.includes(i18n.t("action_provider.execution.uncertain")), "the existing panel must show the shared error");
        await act(async () => {nextRevision();});
        await act(async () => {quick().click();});
        await act(async () => {resolve({ok: false, reason_key: "action_provider.execution.send_failed"});});
        assert(!quick().disabled && !lower().disabled, "a definitive rejection must allow retry from either entry");
        await act(async () => {quick().click();});
        await act(async () => {resolve({ok: false, msg_id: 7});});
        assert(quick().disabled && lower().disabled, "a submitted request must wait for state even when its result reports failure");
        await act(async () => {update({...initial, action_report: {...initial.action_report!, global_actions: [{...remake, availability: "unavailable"}]}});});
        assert(quick().disabled && !lower(), "unavailable remakes must not be executable from the shortcut");
        await act(async () => {update({...initial, action_report: {...initial.action_report!, global_actions: []}});});
        assert(quick().disabled, "a missing provider remake must disable the shortcut");
        await act(async () => {update({...initial, session_id: undefined});});
        assert(quick().disabled && lower().disabled, "missing session identifiers must disable both entries");
        await act(async () => {update(null);});
        assert(quick().disabled, "no game state must disable the shortcut");
        return "Action execution checks passed (mock IPC only).";
    } finally {
        await act(async () => {root.unmount();});
        container.remove();
        clearMocks();
    }
}

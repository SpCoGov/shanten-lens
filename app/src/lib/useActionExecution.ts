import {useRef, useState} from "react";
import {useTranslation} from "react-i18next";
import type {ActionCandidate} from "../bindings";
import type {GameStateData} from "./gamestate";
import {executeAction} from "./ipc";

export function useActionExecution(state: GameStateData | null) {
    const {t} = useTranslation();
    const executing = useRef(false);
    const [pending, setPending] = useState<string | null>(null);
    const [submitted, setSubmitted] = useState<string | null>(null);
    const [feedback, setFeedback] = useState<{error: boolean; text: string} | null>(null);
    const stamp = `${state?.session_id}:${state?.revision}`;
    const waiting = submitted === stamp;
    const disabled = pending !== null || waiting || state?.session_id == null || state.revision == null;
    const execute = async (action: ActionCandidate) => {
        if (executing.current || waiting || action.availability !== "available" || state?.session_id == null || state.revision == null) return;
        executing.current = true;
        setPending(action.id);
        setFeedback(null);
        try {
            const result = await executeAction({action_id: action.id, session_id: state.session_id, revision: state.revision});
            if (result.ok || result.msg_id != null || result.reason_key === "action_provider.execution.uncertain" || result.reason_key === "action_provider.execution.awaiting_state") setSubmitted(stamp);
            setFeedback({error: !result.ok, text: result.ok
                ? t("action_provider.execution.success", {action: t(`action_provider.actions.${action.kind}`)})
                : t(result.reason_key || "action_provider.execution.send_failed", {...result.reason_values})});
        } catch {
            setSubmitted(stamp);
            setFeedback({error: true, text: t("action_provider.execution.uncertain")});
        } finally {
            executing.current = false;
            setPending(null);
        }
    };
    return {pending, waiting, disabled, feedback, execute};
}

export type ActionExecution = ReturnType<typeof useActionExecution>;

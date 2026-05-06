import React from "react";
import {createPortal} from "react-dom";
import "../styles/theme.css";
import {type EffectItem} from "../lib/gamestate";
import AmuletCard from "./AmuletCard";
import "../lib/i18n";
import {useTranslation} from "react-i18next";

type DragState = {
    uid: number;
    item: EffectItem;
    pointerId: number;
    startX: number;
    startY: number;
    x: number;
    y: number;
    offsetX: number;
    offsetY: number;
    width: number;
    height: number;
    active: boolean;
};

export default function AmuletBar({
                                      items,
                                      scale = 0.55,
                                      max = 8,
                                      onItemClick,
                                      showPrice,
                                      onReorder,
                                  }: {
    items: EffectItem[];
    scale?: number;
    max?: number;
    onItemClick?: (item: EffectItem) => void;
    showPrice?: boolean;
    onReorder?: (sortedUid: number[]) => void;
}) {
    const list = React.useMemo(() => Array.isArray(items) ? items.slice(0, max) : [], [items, max]);
    const {t} = useTranslation();
    const [displayList, setDisplayList] = React.useState<EffectItem[]>(list);
    const [drag, setDrag] = React.useState<DragState | null>(null);
    const dragRef = React.useRef<DragState | null>(null);
    const itemRefs = React.useRef(new Map<number, HTMLDivElement>());
    const capturedElementRef = React.useRef<HTMLDivElement | null>(null);
    const suppressClickRef = React.useRef(false);
    const latestDisplayListRef = React.useRef<EffectItem[]>(list);

    const canDrag = Boolean(onReorder) && list.length > 1;

    React.useEffect(() => {
        latestDisplayListRef.current = displayList;
    }, [displayList]);

    React.useEffect(() => {
        dragRef.current = drag;
    }, [drag]);

    React.useEffect(() => {
        if (drag) return;
        setDisplayList(list);
        latestDisplayListRef.current = list;
    }, [drag, list]);

    const moveDraggedItem = React.useCallback((dragUid: number, clientX: number) => {
        setDisplayList((current) => {
            const from = current.findIndex((item) => item.uid === dragUid);
            if (from < 0) return current;

            let to = from;
            for (let index = 0; index < current.length; index += 1) {
                const item = current[index];
                if (item.uid === dragUid) continue;
                const el = itemRefs.current.get(item.uid);
                if (!el) continue;
                const rect = el.getBoundingClientRect();
                const midpoint = rect.left + rect.width / 2;
                if (from < index && clientX > midpoint) to = index;
                if (from > index && clientX < midpoint) to = index;
            }

            if (to === from) return current;
            const next = [...current];
            const [moved] = next.splice(from, 1);
            next.splice(to, 0, moved);
            latestDisplayListRef.current = next;
            return next;
        });
    }, []);

    const commitDrag = React.useCallback((state: DragState) => {
        if (state.active) {
            const sortedUid = latestDisplayListRef.current
                .map((item) => Number(item.uid))
                .filter((uid) => Number.isFinite(uid));
            if (sortedUid.length === list.length) {
                const original = list.map((item) => Number(item.uid));
                const changed = sortedUid.some((uid, index) => uid !== original[index]);
                if (changed) onReorder?.(sortedUid);
            }
        } else if (!suppressClickRef.current) {
            onItemClick?.(state.item);
        }

        setDrag(null);
        dragRef.current = null;
        capturedElementRef.current = null;
        window.setTimeout(() => {
            suppressClickRef.current = false;
        }, 80);
    }, [list, onItemClick, onReorder]);

    const cancelDrag = React.useCallback(() => {
        setDisplayList(list);
        latestDisplayListRef.current = list;
        setDrag(null);
        dragRef.current = null;
        capturedElementRef.current = null;
        window.setTimeout(() => {
            suppressClickRef.current = false;
        }, 80);
    }, [list]);

    React.useEffect(() => {
        if (!drag) return;

        const handleWindowPointerMove = (event: PointerEvent) => {
            const state = dragRef.current;
            if (!state || event.pointerId !== state.pointerId) return;
            const dx = event.clientX - state.startX;
            const dy = event.clientY - state.startY;
            const shouldActivate = state.active || Math.hypot(dx, dy) >= 5;
            if (shouldActivate) {
                suppressClickRef.current = true;
                event.preventDefault();
                moveDraggedItem(state.uid, event.clientX);
            }
            const next = {
                ...state,
                x: event.clientX,
                y: event.clientY,
                active: shouldActivate,
            };
            dragRef.current = next;
            setDrag(next);
        };

        const handleWindowPointerUp = (event: PointerEvent) => {
            const state = dragRef.current;
            if (!state || event.pointerId !== state.pointerId) return;
            try {
                capturedElementRef.current?.releasePointerCapture(event.pointerId);
            } catch {
            }
            commitDrag(state);
        };

        const handleWindowPointerCancel = (event: PointerEvent) => {
            const state = dragRef.current;
            if (!state || event.pointerId !== state.pointerId) return;
            cancelDrag();
        };

        window.addEventListener("pointermove", handleWindowPointerMove, {passive: false});
        window.addEventListener("pointerup", handleWindowPointerUp);
        window.addEventListener("pointercancel", handleWindowPointerCancel);
        window.addEventListener("blur", cancelDrag);
        return () => {
            window.removeEventListener("pointermove", handleWindowPointerMove);
            window.removeEventListener("pointerup", handleWindowPointerUp);
            window.removeEventListener("pointercancel", handleWindowPointerCancel);
            window.removeEventListener("blur", cancelDrag);
        };
    }, [cancelDrag, commitDrag, drag, moveDraggedItem]);

    const startPointerDrag = React.useCallback((event: React.PointerEvent<HTMLDivElement>, item: EffectItem) => {
        if (!canDrag && !onItemClick) return;
        if (event.button !== 0) return;
        const rect = event.currentTarget.getBoundingClientRect();
        try {
            event.currentTarget.setPointerCapture(event.pointerId);
            capturedElementRef.current = event.currentTarget;
        } catch {
            capturedElementRef.current = null;
        }
        const next = {
            uid: item.uid,
            item,
            pointerId: event.pointerId,
            startX: event.clientX,
            startY: event.clientY,
            x: event.clientX,
            y: event.clientY,
            offsetX: event.clientX - rect.left,
            offsetY: event.clientY - rect.top,
            width: rect.width,
            height: rect.height,
            active: false,
        };
        dragRef.current = next;
        setDrag(next);
    }, [canDrag, onItemClick]);

    if (list.length === 0) {
        return (
            <div
                style={{
                    border: "1px dashed var(--border)",
                    borderRadius: 10,
                    padding: 6,
                    color: "var(--muted-fg)",
                    fontSize: 12,
                }}
            >
                {t("noAmulet")}
            </div>
        );
    }

    return (
        <div
            style={{
                display: "flex",
                flexWrap: "nowrap",
                gap: 8,
                overflowX: "auto",
                overflowY: "hidden",
                padding: "6px 4px",
            }}
        >
            {displayList.map((it) => (
                <div
                    key={`${it.uid}-${it.id}`}
                    ref={(el) => {
                        if (el) itemRefs.current.set(it.uid, el);
                        else itemRefs.current.delete(it.uid);
                    }}
                    className={`amulet-drag-item ${drag?.uid === it.uid && drag.active ? "is-dragging" : ""}`}
                    data-draggable={canDrag ? "true" : "false"}
                    onPointerDown={(event) => startPointerDrag(event, it)}
                >
                    <AmuletCard item={it} scale={scale} showPrice={showPrice}/>
                </div>
            ))}
            {drag?.active ? createPortal(
                <div
                    className="amulet-drag-ghost"
                    style={{
                        left: drag.x - drag.offsetX,
                        top: drag.y - drag.offsetY,
                        width: drag.width,
                        height: drag.height,
                    }}
                >
                    <AmuletCard item={drag.item} scale={scale} showPrice={showPrice}/>
                </div>,
                document.body,
            ) : null}
        </div>
    );
}

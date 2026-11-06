import React, {act} from "react";
import {createRoot} from "react-dom/client";
import Modal from "../src/components/Modal";

export async function runModalTests() {
    const assert = (value: unknown, message: string) => { if (!value) throw new Error(message); };
    const container = document.createElement("div");
    document.body.append(container);
    const root = createRoot(container);
    const html = document.documentElement;
    const previousWidth = Object.getOwnPropertyDescriptor(html, "clientWidth");
    const previousHtmlStyle = html.getAttribute("style");
    const previousBodyStyle = document.body.getAttribute("style");
    let backgroundStarts = 0;
    let closes = 0;
    const close = () => { closes += 1; };
    const render = (first: boolean, second = false) => root.render(
        <div onMouseDown={() => backgroundStarts++} onTouchStart={() => backgroundStarts++}
             onPointerDown={() => backgroundStarts++}>
            <span data-testid="background"/>
            <Modal open={first} onClose={close} title="First"><span data-testid="dialog-content"/></Modal>
            <Modal open={second} onClose={close} title="Second"><span/></Modal>
        </div>,
    );
    try {
        html.style.overflow = "auto";
        document.body.style.overflow = "visible";
        html.style.scrollbarGutter = "auto";
        Object.defineProperty(html, "clientWidth", {configurable: true, value: window.innerWidth - 16});
        await act(async () => { render(true); });
        assert(html.style.scrollbarGutter === "stable", "opening must preserve the existing scrollbar space");
        const dialog = document.querySelector('[role="dialog"]')!;
        const content = dialog.querySelector('[data-testid="dialog-content"]')!;
        const overlay = dialog.parentElement!;
        for (const target of [content, overlay]) {
            for (const type of ["mousedown", "pointerdown", "touchstart"]) {
                await act(async () => { target.dispatchEvent(new Event(type, {bubbles: true})); });
            }
        }
        assert(backgroundStarts === 0, "dialog and overlay events must not start dragging a React ancestor across the portal");
        container.querySelector('[data-testid="background"]')!.dispatchEvent(new Event("mousedown", {bubbles: true}));
        assert(backgroundStarts === 1, "ordinary background dragging must remain available");
        await act(async () => { dialog.querySelector<HTMLButtonElement>("button")!.click(); });
        assert(closes === 1, "the dialog close button must still work");
        await act(async () => { overlay.click(); });
        assert(closes === 2, "the overlay close action must still work");
        await act(async () => { render(true, true); });
        await act(async () => { render(false, true); });
        assert(html.style.overflow === "hidden" && html.style.scrollbarGutter === "stable", "closing the first dialog must keep the remaining dialog's lock");
        await act(async () => { render(false); });
        assert(html.style.overflow === "auto" && document.body.style.overflow === "visible" && html.style.scrollbarGutter === "auto", "closing the last dialog must restore the original styles regardless of close order");
        Object.defineProperty(html, "clientWidth", {configurable: true, value: window.innerWidth});
        await act(async () => { render(true); });
        assert(html.style.scrollbarGutter === "auto", "opening without a scrollbar must not add a new gutter");
        await act(async () => { render(false); });
        html.style.scrollbarGutter = "stable both-edges";
        Object.defineProperty(html, "clientWidth", {configurable: true, value: window.innerWidth - 32});
        await act(async () => { render(true); });
        assert(html.style.scrollbarGutter === "stable both-edges", "an existing gutter policy must be preserved");
        return "Modal portal event and scroll-lock checks passed (9 assertions; virtual DOM only).";
    } finally {
        await act(async () => { root.unmount(); });
        container.remove();
        if (previousWidth) Object.defineProperty(html, "clientWidth", previousWidth);
        else Reflect.deleteProperty(html, "clientWidth");
        if (previousHtmlStyle == null) html.removeAttribute("style");
        else html.setAttribute("style", previousHtmlStyle);
        if (previousBodyStyle == null) document.body.removeAttribute("style");
        else document.body.setAttribute("style", previousBodyStyle);
    }
}

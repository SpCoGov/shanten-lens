import React from "react";
import {createPortal} from "react-dom";
import {useTranslation} from "react-i18next";
import {normalizeSidebarLayout, separator, SIDEBAR_LAYOUT_KEY, type SidebarLayout} from "../lib/sidebarLayout";
import styles from "./NavigationSidebar.module.css";

export type SidebarItem = {id: string; title: string; icon: string; active?: boolean; onClick: () => void; tutorial?: string; buttonRef?: React.Ref<HTMLButtonElement>};
export default function NavigationSidebar({items, defaults}: {items: SidebarItem[]; defaults: SidebarLayout}) {
    const {t} = useTranslation();
    const [saved, setSaved] = React.useState<unknown>(() => {try {return JSON.parse(localStorage.getItem(SIDEBAR_LAYOUT_KEY) || "null");} catch {return null;}});
    const layout = normalizeSidebarLayout(saved, defaults);
    const [draft, setDraft] = React.useState<{id: string; outside: boolean}[] | null>(null);
    const [error, setError] = React.useState("");
    const [menuOpen, setMenuOpen] = React.useState(false);
    const [position, setPosition] = React.useState({top: 0, left: 0});
    const sidebar = React.useRef<HTMLElement>(null);
    const menu = React.useRef<HTMLDivElement>(null);
    const moreButton = React.useRef<HTMLButtonElement>(null);
    const editor = React.useRef<HTMLDivElement>(null);
    const dragId = React.useRef<string | null>(null);
    const itemMap = new Map(items.map(item => [item.id, item]));
    const moreActive = layout.more.some(id => itemMap.get(id)?.active);
    const activeId = items.find(item => item.active)?.id;
    const outsideOrder = layout.outside.join("|");
    React.useLayoutEffect(() => {
        const root = sidebar.current;
        if (!root) return;
        const update = () => {
            const active = root.querySelector<HTMLButtonElement>("button.nav-icon.active");
            root.style.setProperty("--nav-indicator-opacity", active ? "1" : "0");
            if (active) {
                root.style.setProperty("--nav-indicator-top", active.offsetTop + "px");
                root.style.setProperty("--nav-indicator-height", active.offsetHeight + "px");
            }
        };
        update();
        const observer = new ResizeObserver(update);
        observer.observe(root);
        Array.from(root.children).forEach(child => observer.observe(child));
        return () => observer.disconnect();
    }, [activeId, outsideOrder, moreActive]);
    React.useLayoutEffect(() => {
        if (!menuOpen && !draft) return;
        const close = (e: PointerEvent) => {if (!menu.current?.contains(e.target as Node) && !moreButton.current?.contains(e.target as Node) && !editor.current?.contains(e.target as Node)) {setMenuOpen(false);setDraft(null);}};
        const key = (e: KeyboardEvent) => {if (e.key === "Escape") {setMenuOpen(false); setDraft(null); moreButton.current?.focus();}};
        const reposition = () => {
            const rect = moreButton.current?.getBoundingClientRect();
            if (!rect) return;
            const top = editor.current
                ? Math.max(8, sidebar.current?.getBoundingClientRect().top ?? 8)
                : Math.max(8, Math.min(rect.top, window.innerHeight - (menu.current?.offsetHeight ?? 300) - 8));
            if (editor.current) editor.current.style.maxHeight = `min(560px, ${Math.max(0, window.innerHeight - top - 8)}px)`;
            setPosition({top, left: rect.right + 10});
        };
        reposition();
        document.addEventListener("pointerdown", close); document.addEventListener("keydown", key);
        window.addEventListener("resize", reposition); window.addEventListener("scroll", reposition, true);
        return () => {document.removeEventListener("pointerdown", close);document.removeEventListener("keydown", key);window.removeEventListener("resize", reposition);window.removeEventListener("scroll", reposition, true);};
    }, [menuOpen, !!draft]);
    const rows = (value: SidebarLayout) => [...value.outside.map(id => ({id, outside:true})), ...value.more.map(id => ({id, outside:false}))];
    const openEditor = () => {setMenuOpen(false);setDraft(rows(layout));setError("");};
    const finish = () => {
        if (!draft) return;
        const next = {outside:draft.filter(row => row.outside).map(row => row.id), more:draft.filter(row => !row.outside).map(row => row.id)};
        try {localStorage.setItem(SIDEBAR_LAYOUT_KEY, JSON.stringify(next));setSaved(next);setDraft(null);moreButton.current?.focus();} catch {setError(t("nav_editor.save_failed"));}
    };
    const move = (id: string, target: string) => setDraft(current => {
        if (!current || id === target) return current;
        const next = [...current], from = next.findIndex(row => row.id === id), to = next.findIndex(row => row.id === target);
        if (from < 0 || to < 0) return current;
        next.splice(to, 0, next.splice(from, 1)[0]);
        return next;
    });
    React.useEffect(() => {if (draft) editor.current?.querySelector<HTMLButtonElement>("button")?.focus();}, [!!draft]);
    const renderItem = (id: string, outside: boolean) => {
        if (separator(id)) return <div key={id} className={id === "spacer" && outside ? styles.spacer : styles.divider} role="separator"/>;
        if (id === "more") return <button key={id} ref={moreButton} className={`nav-icon ${moreActive ? "active" : ""}`} title={t("nav.more")} aria-label={t("nav.more")} aria-expanded={menuOpen} aria-haspopup="menu" data-tutorial="nav-more" onClick={() => setMenuOpen(!menuOpen)}><span className="ms" aria-hidden="true">more_horiz</span></button>;
        const item = itemMap.get(id);
        if (!item) return null;
        return <button key={id} ref={item.buttonRef} className={`${outside ? "nav-icon" : "more-menu-item"} ${item.active ? "active" : ""}`} role={outside ? undefined : "menuitem"} title={item.title} aria-label={item.title} aria-current={item.active ? "page" : undefined} data-tutorial={item.tutorial} onClick={() => {setMenuOpen(false);item.onClick();}}><span className="ms" aria-hidden="true">{item.icon}</span>{!outside && <span>{item.title}</span>}</button>;
    };
    return <>
        <aside ref={sidebar} className={`sidebar ${styles.sidebar}`} onContextMenu={event => {event.preventDefault();openEditor();}} aria-label={t("nav_editor.title")}>
            <div className="sidebar-active-indicator" aria-hidden="true" style={{opacity:"var(--nav-indicator-opacity, 0)"}}/>
            {layout.outside.map(id => renderItem(id, true))}
        </aside>
        {menuOpen && createPortal(<div ref={menu} className={`more-menu ${styles.menu}`} style={position} role="menu">
            {layout.more.map(id => renderItem(id, false))}
            <div className={styles.divider} role="separator"/>
            <button className="more-menu-item" role="menuitem" onClick={openEditor}><span className="ms" aria-hidden="true">edit</span><span>{t("nav_editor.title")}</span></button>
        </div>, document.body)}
        {draft && createPortal(<div ref={editor} className={styles.editor} style={position} role="dialog" aria-label={t("nav_editor.title")} onPointerMove={event => {
                    if (!dragId.current) return;
                    const target = document.elementFromPoint(event.clientX, event.clientY)?.closest<HTMLElement>("[data-row-id]");
                    if (target && editor.current?.contains(target) && target.dataset.rowId) move(dragId.current, target.dataset.rowId);
                    const list = editor.current?.querySelector<HTMLElement>("." + styles.editorList);
                    if (list) {const rect = list.getBoundingClientRect();if (event.clientY < rect.top + 24) list.scrollTop -= 12;else if (event.clientY > rect.bottom - 24) list.scrollTop += 12;}
                }} onPointerUp={() => {dragId.current = null;}} onPointerCancel={() => {dragId.current = null;}} onLostPointerCapture={() => {dragId.current = null;}}>
            <header><span>{t("nav_editor.customize")}</span><button onClick={finish}>{t("nav_editor.done")}</button></header>
            <div className={styles.editorList}>{draft.map(row => <div key={row.id} className={styles.row} data-row-id={row.id}>
                <input type="checkbox" checked={row.outside} disabled={row.id === "more" || row.id === "spacer"} title={t("nav_editor.checked_hint")} aria-label={t("nav_editor.checked_hint")} onChange={event => {
                    const outside = event.target.checked;
                    setDraft(current => current?.map(item => item.id === row.id ? {...item, outside} : item) ?? null);
                }}/>
                <span className="ms" aria-hidden="true">{separator(row.id) ? "horizontal_rule" : row.id === "more" ? "more_horiz" : itemMap.get(row.id)?.icon ?? "extension"}</span>
                <span className={styles.label}>{separator(row.id) ? t(row.id === "spacer" ? "nav_editor.spacer" : "nav_editor.separator") : row.id === "more" ? t("nav.more") : itemMap.get(row.id)?.title ?? t("nav_editor.unavailable", {id:row.id})}</span>
                {row.id.startsWith("separator:") && <button className={styles.remove} title={t("nav_editor.remove")} aria-label={t("nav_editor.remove")} onClick={() => setDraft(current => current?.filter(item => item.id !== row.id) ?? null)}>×</button>}
                <button className={styles.handle} aria-label={t("nav_editor.drag")} title={t("nav_editor.drag")} onPointerDown={event => {if (event.button !== 0) return;dragId.current = row.id;editor.current?.setPointerCapture(event.pointerId);}}><span className="ms" aria-hidden="true">drag_indicator</span></button>
            </div>)}</div>
            {error && <p role="alert">{error}</p>}
            <footer><button onClick={() => setDraft(current => [...(current ?? []), {id:"separator:" + crypto.randomUUID(), outside:true}])}>{t("nav_editor.add_separator")}</button><button onClick={() => setDraft(rows(defaults))}>{t("nav_editor.reset")}</button></footer>
        </div>, document.body)}
    </>;
}

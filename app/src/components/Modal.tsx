import React from "react";
import {createPortal} from "react-dom";
import "../styles/theme.css";
import styles from "./Modal.module.css";
import {t} from "i18next";

let openModalCount = 0;

export default function Modal({
                                  open,
                                  onClose,
                                  title,
                                  children,
                                  width = 720,
                                  actions,
                              }: {
    open: boolean;
    onClose: () => void;
    title?: string;
    children: React.ReactNode;
    width?: number;
    actions?: React.ReactNode;
}) {
    React.useEffect(() => {
        if (!open) return;

        openModalCount += 1;
        const html = document.documentElement;
        const body = document.body;
        const prevHtmlOverflow = html.style.overflow;
        const prevBodyOverflow = body.style.overflow;

        html.style.overflow = "hidden";
        body.style.overflow = "hidden";
        body.classList.add("modal-open");

        const onKeyDown = (e: KeyboardEvent) => {
            if (e.key === "Escape") onClose();
        };
        window.addEventListener("keydown", onKeyDown);

        return () => {
            window.removeEventListener("keydown", onKeyDown);
            openModalCount = Math.max(0, openModalCount - 1);
            if (openModalCount === 0) {
                html.style.overflow = prevHtmlOverflow;
                body.style.overflow = prevBodyOverflow;
                body.classList.remove("modal-open");
            }
        };
    }, [open, onClose]);

    if (!open) return null;
    return createPortal(
        <div
            className={styles.overlay}
            onClick={onClose}
            onWheelCapture={(e) => {
                if (e.target === e.currentTarget) {
                    e.preventDefault();
                }
            }}
        >
            <div
                className={styles.card}
                style={{width}}
                onClick={(e) => e.stopPropagation()}
            >
                <div className={styles.header}>
                    <div className={styles.headerTitle}>{title}</div>
                    <div className={styles.headerActions}>
                        {actions}
                        <button className="nav-btn" onClick={onClose}>{t("modal.close")}</button>
                    </div>
                </div>

                <div className={styles.body}>
                    {children}
                </div>
            </div>
        </div>,
        document.body
    );
}

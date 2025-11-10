import React from "react";
import "../styles/theme.css";
import styles from "./Modal.module.css";
import {t} from "i18next";

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
    if (!open) return null;
    return (
        <div className={styles.overlay} onClick={onClose}>
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
        </div>
    );
}
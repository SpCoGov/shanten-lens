import React from "react";
import "../styles/theme.css";
import styles from "./AboutPage.module.css";
import {useTranslation, Trans} from "react-i18next";
import AmuletCard from "../components/AmuletCard";
import {type EffectItem} from "../lib/gamestate";

export default function AboutPage() {
    const {t} = useTranslation();

    const amulet225: EffectItem = {id: 2250, uid: 0, volume: 1, store: [], tags: []};
    const amulet218: EffectItem = {id: 2180, uid: 0, volume: 1, store: [], tags: []};

    return (
        <div className={styles.wrap}>
            <h1>{t("app.title")} <span className={styles.sub}>{t("app.subtitle")}</span></h1>

            <div className={styles.meta}>
                <span className={styles.author}>{t("app.author")}</span>
                <span className={styles.sep} aria-hidden>·</span>
                <span className={styles.version}>v1.1.14</span>
                <span className={styles.build}>(build&nbsp;1)</span>
            </div>

            <section>
                <h2>{t("about.section_license_title")}</h2>
                <p>
                    <Trans
                        i18nKey="about.license_copyright_html"
                        values={{ year: new Date().getFullYear() }}
                    />
                </p>
                <details>
                    <summary>{t("about.license_toggle_summary")}</summary>
                    <pre className={styles.license}>
{`Licensed under the Apache License, Version 2.0 (the "License").
You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0


Distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND.`}
          </pre>
                </details>
            </section>

            <section>
                <h2>{t("about.section_assets_title")}</h2>
                <p>{t("about.section_assets_body")}</p>
            </section>

            <section>
                <h2>{t("about.section_usage_title")}</h2>
                <p>
                    <Trans
                        i18nKey="about.section_usage_body_html"
                        values={{ year: new Date().getFullYear() }}
                    />
                </p>
            </section>

            <section>
                <h2>{t("about.section_known_issues_title")}</h2>

                <div className={styles.knownIssueRow}>
                    <AmuletCard item={amulet225} scale={0.42}/>
                    <div className={styles.knownIssueText}>
                        <Trans i18nKey="about.known_issue_225_html"/>
                    </div>
                </div>

                <div className={styles.knownIssueRow}>
                    <AmuletCard item={amulet218} scale={0.42}/>
                    <div className={styles.knownIssueText}>
                        <Trans i18nKey="about.known_issue_218_html"/>
                    </div>
                </div>
            </section>

            <footer className={styles.footer}>
                <details>
                    <summary>{t("about.section_repo_title")}</summary>
                    <p className={styles.muted}>
                        <a
                            href="https://github.com/SpCoGov/shanten-lens"
                            target="_blank"
                            rel="noreferrer"
                        >
                            {t("about.repo_link_text")}
                        </a>
                    </p>
                </details>
            </footer>
        </div>
    );
}
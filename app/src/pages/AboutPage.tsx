import React from "react";
import "../styles/theme.css";
import styles from "./AboutPage.module.css";
import {useTranslation, Trans} from "react-i18next";
import AmuletCard from "../components/AmuletCard";
import {type EffectItem} from "../lib/gamestate";
import Modal from "../components/Modal";
import Tile from "../components/Tile";

const TILE_GROUPS: Array<{ titleKey: string; tiles: string[] }> = [
    {titleKey: "about.tile_groups.manzu", tiles: ["0m", "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m"]},
    {titleKey: "about.tile_groups.pinzu", tiles: ["0p", "1p", "2p", "3p", "4p", "5p", "6p", "7p", "8p", "9p"]},
    {titleKey: "about.tile_groups.souzu", tiles: ["0s", "1s", "2s", "3s", "4s", "5s", "6s", "7s", "8s", "9s"]},
    {titleKey: "about.tile_groups.honors", tiles: ["1z", "2z", "3z", "4z", "5z", "6z", "7z"]},
    {titleKey: "about.tile_groups.others", tiles: ["bd"]},
];

const ASSET_SOURCES = [
    {
        nameKey: "about.asset_sources.mahjong_soul.name",
        noteKey: "about.asset_sources.mahjong_soul.note",
        url: "https://mahjongsoul.com/",
    },
    {
        nameKey: "about.asset_sources.tempai_svg.name",
        noteKey: "about.asset_sources.tempai_svg.note",
        url: "https://github.com/tempai-dev/riichi-mahjong-tiles-svg",
    },
];

export default function AboutPage({onSecretClick}: { onSecretClick: () => void }) {
    const {t} = useTranslation();
    const [openTileGallery, setOpenTileGallery] = React.useState(false);

    const amulet225: EffectItem = {id: 2250, uid: 0, volume: 1, store: [], tags: []};
    const amulet218: EffectItem = {id: 2180, uid: 0, volume: 1, store: [], tags: []};

    return (
        <div className={styles.wrap}>
            <div className={styles.hero}>
                <img className={styles.logo} src="/logo.svg" alt="Shanten Lens logo"/>
                <h1 className={styles.secretTitle} onClick={onSecretClick}>
                    {t("app.title")} <span className={styles.sub}>{t("app.subtitle")}</span>
                </h1>
            </div>

            <div className={styles.meta}>
                <span className={styles.author}>{t("app.author")}</span>
                <span className={styles.sep} aria-hidden>|</span>
                <span className={styles.version}>v2.0.3</span>
                <span className={styles.build}>(build&nbsp;1)</span>
            </div>

            <section>
                <h2>{t("about.section_license_title")}</h2>
                <p>
                    <Trans i18nKey="about.license_copyright_html" values={{year: new Date().getFullYear()}}/>
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
                <p>{t("about.assets_intro")}</p>
                <div className={styles.assetList}>
                    {ASSET_SOURCES.map((item) => (
                        <div key={item.url} className={styles.assetItem}>
                            <div className={styles.assetName}>
                                {t(item.nameKey)}
                            </div>
                            <div className={styles.assetNote}>
                                {t(item.noteKey)}
                            </div>
                            <a href={item.url} target="_blank" rel="noreferrer">
                                {t("about.open_link")}
                            </a>
                        </div>
                    ))}
                </div>
                <div className={styles.actionRow}>
                    <button className="nav-btn" onClick={() => setOpenTileGallery(true)}>
                        {t("about.view_all_tiles")}
                    </button>
                </div>
            </section>

            <section>
                <h2>{t("about.section_usage_title")}</h2>
                <p>
                    <Trans i18nKey="about.section_usage_body_html" values={{year: new Date().getFullYear()}}/>
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

            <Modal
                open={openTileGallery}
                onClose={() => setOpenTileGallery(false)}
                title={t("about.tile_gallery_title")}
                width={980}
            >
                <div className={styles.tileGallery}>
                    {TILE_GROUPS.map((group) => (
                        <section key={group.titleKey} className={styles.tileSection}>
                            <div className={styles.tileSectionTitle}>{t(group.titleKey)}</div>
                            <div className={styles.tileGrid}>
                                {group.tiles.map((tile) => (
                                    <div key={tile} className={styles.tileItem}>
                                        <Tile tile={tile} width={54} height={72}/>
                                        <span className={styles.tileCode}>{tile}</span>
                                    </div>
                                ))}
                            </div>
                        </section>
                    ))}
                </div>
            </Modal>
        </div>
    );
}

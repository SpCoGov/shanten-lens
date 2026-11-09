import React from "react";
import {useTranslation} from "react-i18next";
import Modal from "./Modal";
import styles from "../pages/ScorePage.module.css";

const dragons = ["hatchling", "white", "red", "ice", "black", "blue", "green", "bronze"] as const;

export default function DragonBossEffects() {
    const {t} = useTranslation();
    const [open, setOpen] = React.useState(false);
    return <>
        <button className="nav-btn" onClick={() => setOpen(true)}>{t("score.dragon_boss_title")}</button>
        <Modal open={open} onClose={() => setOpen(false)} title={t("score.dragon_boss_title")} width={760}>
            <div className={styles.bossList}>
                {dragons.map(dragon => <section className={styles.bossGroup} key={dragon}>
                    <h3>{t(`score.dragon_bosses.${dragon}.name`)}</h3>
                    <ul>{(t(`score.dragon_bosses.${dragon}.effects`, {returnObjects: true}) as string[])
                        .map((effect, index) => <li key={index}>{effect}</li>)}</ul>
                </section>)}
            </div>
        </Modal>
    </>;
}

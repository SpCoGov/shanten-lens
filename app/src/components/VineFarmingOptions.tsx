import React from "react";
import {useTranslation} from "react-i18next";
import {vineVisiblePrices, vineRefreshLimits, type VineFarmingProfile} from "../lib/autoPlayConfig";
import styles from "./VineFarmingOptions.module.css";

const kinds = ["reward", "normal"] as const;
// Axis geometry in percent of the axis width: ticks at 10..90, track 7..93, ∞ at 98.
const TRACK_END = 93;
const positions = [...vineRefreshLimits.map((_, i) => 15 + i * 10), 98];

export default function VineFarmingOptions({profile, onChange}: {profile: VineFarmingProfile; onChange: (next: VineFarmingProfile) => void}) {
    const {t} = useTranslation();
    const titleId = React.useId();
    const axis = React.useRef<HTMLDivElement>(null);
    const drag = React.useRef<{key: "reward_refresh_limit" | "normal_refresh_limit"; original: number | null} | null>(null);
    const indexOf = (value: number | null | undefined) => value == null ? vineRefreshLimits.length : vineRefreshLimits.indexOf(value);
    const describe = (index: number) => index === vineRefreshLimits.length ? t("vine.unlimited")
        : t("vine.price_interval", {left: vineVisiblePrices[index], right: vineVisiblePrices[index+1]});
    function select(key: "reward_refresh_limit" | "normal_refresh_limit", index: number) {
        onChange({...profile, [key]: vineRefreshLimits[Math.max(0,Math.min(index,vineRefreshLimits.length))] ?? null});
    }
    function move(clientX: number) {
        if (!drag.current || !axis.current) return;
        const rect = axis.current.getBoundingClientRect(), x = (clientX - rect.left) / rect.width * 100;
        const index = positions.reduce((best,pos,i) => Math.abs(pos-x) < Math.abs(positions[best]-x) ? i : best,0);
        select(drag.current.key,index);
    }
    return <section className="panel">
        <h3 className="panel-title">{t("vine.title")}</h3>
        <div className={styles.refresh} role="group" aria-labelledby={titleId}>
            <div className={styles.head}>
                <span id={titleId} className={styles.headTitle}>{t("vine.refresh_title")}</span>
                <span className={styles.chips}>{kinds.map(kind => <span key={kind} className={`${styles.chip} ${styles[kind]}`}>
                    {t(`vine.${kind}_refresh`)}<output>{describe(indexOf(profile[`${kind}_refresh_limit`]))}</output>
                </span>)}</span>
            </div>
            <div className={styles.scroll}><div ref={axis} className={styles.axis}>
                <span className={styles.track}/>
                {kinds.map(kind => <span key={kind} className={`${styles.range} ${styles[kind]}`}
                    style={{width: `${Math.min(positions[indexOf(profile[`${kind}_refresh_limit`])], TRACK_END) - 7}%`}}/>)}
                {vineVisiblePrices.map((price,i) => <span key={price} className={styles.tick} style={{left: `${10 + i * 10}%`}}>{price}</span>)}
                <span className={styles.axisEnd} aria-hidden="true">
                    <svg viewBox="0 0 24 24" fill="none">
                        <path d="M2 12h5l3-8 5 16 3-8h4" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"/>
                    </svg>
                </span>
                <span className={`ms ${styles.axisEnd} ${styles.axisEndRight}`} aria-hidden="true">all_inclusive</span>
                {kinds.map(kind => {
                    const key = `${kind}_refresh_limit` as const, index = indexOf(profile[key]);
                    return <button type="button" key={kind} className={`${styles.handle} ${styles[kind]}`}
                        style={{left: `${positions[index]}%`}}
                        role="slider" aria-label={t(`vine.${kind}_refresh`)} aria-valuemin={0} aria-valuemax={positions.length-1}
                        aria-valuenow={index} aria-valuetext={describe(index)} title={`${t(`vine.${kind}_refresh`)} · ${describe(index)}`}
                        onPointerDown={event => {
                            if (event.button !== 0) return;
                            event.preventDefault(); event.currentTarget.focus();
                            drag.current={key,original:profile[key] ?? null};event.currentTarget.setPointerCapture(event.pointerId);
                        }} onPointerMove={event => {if (drag.current?.key === key) move(event.clientX);}}
                        onPointerUp={() => {drag.current=null;}} onLostPointerCapture={() => {drag.current=null;}}
                        onPointerCancel={() => {if (drag.current) onChange({...profile,[key]:drag.current.original});drag.current=null;}}
                        onKeyDown={event => {
                            const next = {ArrowLeft:index-1,ArrowDown:index-1,ArrowRight:index+1,ArrowUp:index+1,Home:0,End:positions.length-1}[event.key];
                            if (next !== undefined) {event.preventDefault();select(key,next);}
                        }}>
                        <img draggable={false} src={`/assets/vine/${kind === "reward" ? "arrow_up_FuJi" : "arrow_down_Lion"}.png`} alt=""/>
                    </button>;
                })}
            </div></div>
        </div>
        <div className={styles.stops}>
            <label className={styles.stopLevel}><span>{t("vine.stop_ex")}</span><select className="form-input" value={profile.stop_ex ?? ""} onChange={event => onChange({...profile,stop_ex:event.target.value ? Number(event.target.value) : null})}>
                <option value="">{t("vine.stop_default")}</option>
                {Array.from({length:70},(_,i) => i+1).map(n => <option key={n} value={n}>{t("vine.ex_level",{n})}</option>)}
            </select></label>
            <label className={styles.check}>{t("vine.half_hp_option")}<input className="config-switch" type="checkbox" checked={profile.pause_half_hp !== false} onChange={event => onChange({...profile,pause_half_hp:event.target.checked})}/></label>
        </div>
    </section>;
}

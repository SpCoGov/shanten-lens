import "../styles/theme.css";
import React from "react";
import {useFuse, addAmulet, addBadge, removeSelected, clearSelection, patchFuseConfig} from "../lib/fuseStore";
import * as backendIpc from "../lib/ipc";
import AmuletPickerModal from "../components/AmuletPickerModal";
import BadgePickerModal from "../components/BadgePickerModal";
import FuseBar from "../components/FuseBar";
import {t} from "i18next";
import {Trans} from "react-i18next";

export default function FusePage() {
    const {config, selected} = useFuse();
    const [openA, setOpenA] = React.useState(false);
    const [openB, setOpenB] = React.useState(false);
    const [saving, setSaving] = React.useState(false);

    const onSave = React.useCallback(async () => {
        setSaving(true);
        try {
            await backendIpc.updateConfig({fuse: config});
        } finally {
            setSaving(false);
        }
    }, [config]);

    const onRemove = React.useCallback(() => {
        if (selected.amulets.size === 0 && selected.badges.size === 0) return;
        removeSelected();
    }, [selected]);

    return (
        <div className="settings-wrap wide-page config-page fuse-page">
            <header className="config-page-header">
                <div>
                    <h1>{t("fuse.title")}</h1>
                    <p>{t("fuse.subtitle")}</p>
                </div>
                <button className="config-primary-button" onClick={onSave} disabled={saving}>
                    {saving ? t("fuse.btn_saving") : t("fuse.btn_save")}
                </button>
            </header>

            <div className="fuse-grid">
                <section className="config-card fuse-watch-card">
                    <div className="config-card-heading">
                        <div>
                            <h2>{t("fuse.section_shop_guard_title")}</h2>
                            <p><Trans i18nKey="fuse.section_shop_guard_desc"/></p>
                        </div>
                    </div>

                    <div className="fuse-watch-list"><FuseBar/></div>
                    <div className="fuse-watch-actions">
                        <button onClick={() => setOpenA(true)}>{t("fuse.btn_add_amulet")}</button>
                        <button onClick={() => setOpenB(true)}>{t("fuse.btn_add_badge")}</button>
                        <button onClick={onRemove} disabled={selected.amulets.size === 0 && selected.badges.size === 0}>
                            {t("fuse.btn_delete_selected")}
                        </button>
                        <button onClick={clearSelection} disabled={selected.amulets.size === 0 && selected.badges.size === 0}>
                            {t("fuse.btn_clear_selection")}
                        </button>
                    </div>
                    <div className="config-card-settings">
                        <label className="config-toggle-row">
                            <span>{t("fuse.toggle_skip_guard")}</span>
                            <input className="config-switch" type="checkbox" checked={Boolean(config.enable_skip_guard)} onChange={(e) => patchFuseConfig({enable_skip_guard: e.target.checked})}/>
                        </label>
                        <label className="config-toggle-row">
                            <span>{t("fuse.toggle_force_pick")}</span>
                            <input className="config-switch" type="checkbox" checked={Boolean(config.enable_shop_force_pick)} onChange={(e) => patchFuseConfig({enable_shop_force_pick: e.target.checked})}/>
                        </label>
                    </div>
                </section>

                <section className="config-card">
                    <div className="config-card-heading"><div><h2>{t("fuse.section_switch_guard_title")}</h2><p><Trans i18nKey="fuse.section_switch_guard_desc"/></p></div></div>
                    <label className="config-toggle-row">
                        <span>{t("fuse.toggle_ting_ready_skip_guard")}</span>
                        <input className="config-switch" type="checkbox" checked={config.enable_ting_ready_skip_guard !== false} onChange={(e) => patchFuseConfig({enable_ting_ready_skip_guard: e.target.checked})}/>
                    </label>
                </section>

                <section className="config-card">
                    <div className="config-card-heading"><div><h2>{t("fuse.section_conduction_title")}</h2><p><Trans i18nKey="fuse.section_conduction_desc"/></p></div></div>
                    <div className="config-card-settings">
                        <label className="config-toggle-row">
                            <span><Trans i18nKey="fuse.toggle_conduction"/></span>
                            <input className="config-switch" type="checkbox" checked={Boolean(config.enable_prestart_kavi_guard)} onChange={(e) => patchFuseConfig({enable_prestart_kavi_guard: e.target.checked})}/>
                        </label>
                        <label className="config-number-row">
                            <span><Trans i18nKey="fuse.label_conduction_threshold"/></span>
                            <input className="form-input" type="number" min={0} value={Number(config.conduction_min_count ?? 3)} onChange={(e) => patchFuseConfig({conduction_min_count: Number(e.target.value || 0)})}/>
                        </label>
                        <p className="config-field-hint"><Trans i18nKey="fuse.hint_conduction_threshold"/></p>
                    </div>
                </section>

                <section className="config-card">
                    <div className="config-card-heading"><div><h2><Trans i18nKey="fuse.section_badge_guard_title"/></h2><p><Trans i18nKey="fuse.section_badge_guard_desc"/></p></div></div>
                    <label className="config-toggle-row"><span><Trans i18nKey="fuse.toggle_badge_guard"/></span><input className="config-switch" type="checkbox" checked={Boolean(config.enable_anti_steal_eat)} onChange={(e) => patchFuseConfig({enable_anti_steal_eat: e.target.checked})}/></label>
                </section>

                <section className="config-card">
                    <div className="config-card-heading"><div><h2><Trans i18nKey="fuse.section_discard_guard_title"/></h2><p><Trans i18nKey="fuse.section_discard_guard_desc"/></p></div></div>
                    <label className="config-toggle-row"><span><Trans i18nKey="fuse.toggle_missing_hand_tile_guard"/></span><input className="config-switch" type="checkbox" checked={Boolean(config.enable_missing_hand_tile_guard)} onChange={(e) => patchFuseConfig({enable_missing_hand_tile_guard: e.target.checked})}/></label>
                </section>

                <section className="config-card">
                    <div className="config-card-heading"><div><h2><Trans i18nKey="fuse.section_kavi_plus_title"/></h2><p><Trans i18nKey="fuse.section_kavi_plus_desc"/></p></div></div>
                    <label className="config-toggle-row"><span><Trans i18nKey="fuse.toggle_kavi_plus"/></span><input className="config-switch" type="checkbox" checked={Boolean(config.enable_kavi_plus_buffer_guard)} onChange={(e) => patchFuseConfig({enable_kavi_plus_buffer_guard: e.target.checked})}/></label>
                </section>

                <section className="config-card">
                    <div className="config-card-heading"><div><h2><Trans i18nKey="fuse.section_hanabi_win_title"/></h2><p><Trans i18nKey="fuse.section_hanabi_win_desc"/></p></div></div>
                    <label className="config-toggle-row"><span><Trans i18nKey="fuse.toggle_hanabi_win_guard"/></span><input className="config-switch" type="checkbox" checked={Boolean(config.enable_hanabi_win_guard)} onChange={(e) => patchFuseConfig({enable_hanabi_win_guard: e.target.checked})}/></label>
                </section>

                <section className="config-card">
                    <div className="config-card-heading"><div><h2><Trans i18nKey="fuse.section_exit_life_title"/></h2><p><Trans i18nKey="fuse.section_exit_life_desc"/></p></div></div>
                    <div className="config-card-settings">
                        <label className="config-toggle-row"><span><Trans i18nKey="fuse.toggle_exit_coin_guard"/></span><input className="config-switch" type="checkbox" checked={Boolean(config.enable_exit_coin_guard)} onChange={(e) => patchFuseConfig({enable_exit_coin_guard: e.target.checked})}/></label>
                        <label className="config-toggle-row"><span><Trans i18nKey="fuse.toggle_exit_life"/></span><input className="config-switch" type="checkbox" checked={Boolean(config.enable_exit_life_guard)} onChange={(e) => patchFuseConfig({enable_exit_life_guard: e.target.checked})}/></label>
                    </div>
                </section>
            </div>

            <AmuletPickerModal
                open={openA}
                onClose={() => setOpenA(false)}
                onSelect={(id) => addAmulet(id)}
            />
            <BadgePickerModal
                open={openB}
                onClose={() => setOpenB(false)}
                onSelect={(id) => addBadge(id)}
            />
        </div>
    );
}

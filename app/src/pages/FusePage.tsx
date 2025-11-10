import "../styles/theme.css";
import React from "react";
import {useFuse, addAmulet, addBadge, removeSelected, clearSelection, patchFuseConfig} from "../lib/fuseStore";
import {ws} from "../lib/ws";
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

    const onSave = React.useCallback(() => {
        setSaving(true);
        try {
            ws.send({
                type: "edit_config",
                data: {
                    fuse: config,
                },
            });
        } finally {
            setSaving(false);
        }
    }, [config]);

    const onRemove = React.useCallback(() => {
        if (selected.amulets.size === 0 && selected.badges.size === 0) return;
        removeSelected();
    }, [selected]);

    return (
        <div className="settings-wrap" style={{padding: 16}}>
            <h2 className="title">{t("fuse.title")}</h2>

            <section className="panel">
                <div className="panel-title">{t("fuse.section_shop_guard_title")}</div>
                <p className="hint" style={{marginTop: 0, lineHeight: 1.5}}>
                    <Trans
                        i18nKey="fuse.section_shop_guard_desc"
                    />
                </p>

                <FuseBar/>

                <div className="toolbar" style={{marginTop: 10, flexWrap: "wrap" as const}}>
                    <button className="nav-btn" onClick={() => setOpenA(true)}>
                        {t("fuse.btn_add_amulet")}
                    </button>
                    <button className="nav-btn" onClick={() => setOpenB(true)}>
                        {t("fuse.btn_add_badge")}
                    </button>
                    <button
                        className="nav-btn"
                        onClick={onRemove}
                        disabled={selected.amulets.size === 0 && selected.badges.size === 0}
                    >
                        {t("fuse.btn_delete_selected")}
                    </button>
                    <button
                        className="nav-btn"
                        onClick={clearSelection}
                        disabled={selected.amulets.size === 0 && selected.badges.size === 0}
                    >
                        {t("fuse.btn_clear_selection")}
                    </button>
                </div>

                <div className="rows" style={{marginTop: 12}}>
                    <label className="row" style={{gridTemplateColumns: "auto 1fr"}}>
                        <input
                            className="form-checkbox"
                            type="checkbox"
                            checked={Boolean(config.enable_skip_guard)}
                            onChange={(e) => patchFuseConfig({enable_skip_guard: e.target.checked})}
                        />
                        <span>{t("fuse.toggle_skip_guard")}</span>
                    </label>

                    <label className="row" style={{gridTemplateColumns: "auto 1fr"}}>
                        <input
                            className="form-checkbox"
                            type="checkbox"
                            checked={Boolean(config.enable_shop_force_pick)}
                            onChange={(e) => patchFuseConfig({enable_shop_force_pick: e.target.checked})}
                        />
                        <span>{t("fuse.toggle_force_pick")}</span>
                    </label>
                </div>
            </section>

            <section className="panel">
                <div className="panel-title">{t("fuse.section_conduction_title")}</div>
                <p className="hint" style={{marginTop: 0, lineHeight: 1.5}}>
                    <Trans
                        i18nKey="fuse.section_conduction_desc"
                    />
                </p>

                <label className="row" style={{gridTemplateColumns: "auto 1fr", marginBottom: 10}}>
                    <input
                        className="form-checkbox"
                        type="checkbox"
                        checked={Boolean(config.enable_prestart_kavi_guard)}
                        onChange={(e) => patchFuseConfig({enable_prestart_kavi_guard: e.target.checked})}
                    />
                    <span>
                        <Trans
                        i18nKey="fuse.toggle_conduction"
                        />
                    </span>
                </label>

                <div className="row" style={{gridTemplateColumns: "auto auto 1fr", alignItems: "center"}}>
                      <span className="hint" style={{color: "var(--color-text)"}}>
                        <Trans
                            i18nKey="fuse.label_conduction_threshold"
                        />
                      </span>
                    <input
                        className="form-input"
                        type="number"
                        min={0}
                        value={Number(config.conduction_min_count ?? 3)}
                        onChange={(e) => patchFuseConfig({conduction_min_count: Number(e.target.value || 0)})}
                        style={{width: 100}}
                    />
                    <span className="hint">
                        <Trans
                            i18nKey="fuse.hint_conduction_threshold"
                        />
                    </span>
                </div>
            </section>

            <section className="panel">
                <div className="panel-title"><Trans
                    i18nKey="fuse.section_badge_guard_title"
                /></div>
                <p className="hint" style={{marginTop: 0, lineHeight: 1.5}}>
                    <Trans
                        i18nKey="fuse.section_badge_guard_desc"
                    />
                </p>

                <label className="row" style={{gridTemplateColumns: "auto 1fr"}}>
                    <input
                        className="form-checkbox"
                        type="checkbox"
                        checked={Boolean(config.enable_anti_steal_eat)}
                        onChange={(e) => patchFuseConfig({enable_anti_steal_eat: e.target.checked})}
                    />
                    <span><Trans
                        i18nKey="fuse.toggle_badge_guard"
                    /></span>
                </label>
            </section>

            <section className="panel">
                <div className="panel-title"><Trans
                    i18nKey="fuse.section_kavi_plus_title"
                /></div>
                <p className="hint" style={{marginTop: 0, lineHeight: 1.5}}>
                    <Trans
                        i18nKey="fuse.section_kavi_plus_desc"
                    />
                </p>

                <label className="row" style={{gridTemplateColumns: "auto 1fr"}}>
                    <input
                        className="form-checkbox"
                        type="checkbox"
                        checked={Boolean(config.enable_kavi_plus_buffer_guard)}
                        onChange={(e) => patchFuseConfig({enable_kavi_plus_buffer_guard: e.target.checked})}
                    />
                    <span><Trans
                        i18nKey="fuse.toggle_kavi_plus"
                    /></span>
                </label>
            </section>

            <section className="panel">
                <div className="panel-title"><Trans
                    i18nKey="fuse.section_exit_life_title"
                /></div>
                <p className="hint" style={{marginTop: 0, lineHeight: 1.5}}>
                    <Trans
                        i18nKey="fuse.section_exit_life_desc"
                    />
                </p>

                <label className="row" style={{gridTemplateColumns: "auto 1fr"}}>
                    <input
                        className="form-checkbox"
                        type="checkbox"
                        checked={Boolean(config.enable_exit_life_guard)}
                        onChange={(e) => patchFuseConfig({enable_exit_life_guard: e.target.checked})}
                    />
                    <span><Trans
                        i18nKey="fuse.toggle_exit_life"
                    /></span>
                </label>
            </section>

            <button className="nav-btn" onClick={onSave} disabled={saving}>
                {saving ? t("fuse.btn_saving") : t("fuse.btn_save")}
            </button>

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
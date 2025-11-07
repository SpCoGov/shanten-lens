import "../styles/theme.css";
import React from "react";
import {useFuse, addAmulet, addBadge, removeSelected, clearSelection, patchFuseConfig} from "../lib/fuseStore";
import {ws} from "../lib/ws";
import AmuletPickerModal from "../components/AmuletPickerModal";
import BadgePickerModal from "../components/BadgePickerModal";
import FuseBar from "../components/FuseBar";

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
            <h2 className="title">熔断</h2>

            <section className="panel">
                <div className="panel-title">购物守护</div>
                <p className="hint" style={{marginTop: 0, lineHeight: 1.5}}>
                    维护监控列表后，在购物时应用两条规则：
                    <br/>
                    1）出现监控项时<strong>禁止“跳过”</strong>（启用下方“禁止跳过”）；
                    <br/>
                    2）出现监控项但你<strong>没有选择监控项之一</strong>则阻止（启用下方“强制选择监控项”）。
                </p>

                <FuseBar/>

                <div className="toolbar" style={{marginTop: 10, flexWrap: "wrap" as const}}>
                    <button className="nav-btn" onClick={() => setOpenA(true)}>
                        添加护身符
                    </button>
                    <button className="nav-btn" onClick={() => setOpenB(true)}>
                        添加印章
                    </button>
                    <button
                        className="nav-btn"
                        onClick={onRemove}
                        disabled={selected.amulets.size === 0 && selected.badges.size === 0}
                    >
                        删除选中
                    </button>
                    <button
                        className="nav-btn"
                        onClick={clearSelection}
                        disabled={selected.amulets.size === 0 && selected.badges.size === 0}
                    >
                        取消选择
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
                        <span>禁止跳过（出现监控项时阻止“跳过”）</span>
                    </label>

                    <label className="row" style={{gridTemplateColumns: "auto 1fr"}}>
                        <input
                            className="form-checkbox"
                            type="checkbox"
                            checked={Boolean(config.enable_shop_force_pick)}
                            onChange={(e) => patchFuseConfig({enable_shop_force_pick: e.target.checked})}
                        />
                        <span>强制选择监控项（出现监控项但未选其一时阻止）</span>
                    </label>
                </div>
            </section>

            <section className="panel">
                <div className="panel-title">传导链守护</div>
                <p className="hint" style={{marginTop: 0, lineHeight: 1.5}}>
                    若传导卡数量达到阈值，且卡维未按规则摆放，将阻止开始对局，以避免忘记给护身符上「传导」而导致断链。
                </p>

                <label className="row" style={{gridTemplateColumns: "auto 1fr", marginBottom: 10}}>
                    <input
                        className="form-checkbox"
                        type="checkbox"
                        checked={Boolean(config.enable_prestart_kavi_guard)}
                        onChange={(e) => patchFuseConfig({enable_prestart_kavi_guard: e.target.checked})}
                    />
                    <span>启用传导链守护</span>
                </label>

                <div className="row" style={{gridTemplateColumns: "auto auto 1fr", alignItems: "center"}}>
                      <span className="hint" style={{color: "var(--color-text)"}}>
                        传导卡阈值
                      </span>
                    <input
                        className="form-input"
                        type="number"
                        min={0}
                        value={Number(config.conduction_min_count ?? 3)}
                        onChange={(e) => patchFuseConfig({conduction_min_count: Number(e.target.value || 0)})}
                        style={{width: 100}}
                    />
                    <span className="hint">（当传导卡数量 ≥ 阈值且卡维位置不安全时，阻止开局）</span>
                </div>
            </section>

            <section className="panel">
                <div className="panel-title">印章守护</div>
                <p className="hint" style={{marginTop: 0, lineHeight: 1.5}}>
                    若卡维携带「传导」或「膨胀」印章且置于盗印左侧，将阻止和牌。
                </p>

                <label className="row" style={{gridTemplateColumns: "auto 1fr"}}>
                    <input
                        className="form-checkbox"
                        type="checkbox"
                        checked={Boolean(config.enable_anti_steal_eat)}
                        onChange={(e) => patchFuseConfig({enable_anti_steal_eat: e.target.checked})}
                    />
                    <span>启用印章守护</span>
                </label>
            </section>

            <section className="panel">
                <div className="panel-title">卡维+缓冲守护</div>
                <p className="hint" style={{marginTop: 0, lineHeight: 1.5}}>
                    当存在「卡维+」与「膨胀」时，若二者之间没有至少隔一个<b>非膨胀</b>护身符作为缓冲，阻止开局。
                </p>

                <label className="row" style={{gridTemplateColumns: "auto 1fr"}}>
                    <input
                        className="form-checkbox"
                        type="checkbox"
                        checked={Boolean(config.enable_kavi_plus_buffer_guard)}
                        onChange={(e) => patchFuseConfig({enable_kavi_plus_buffer_guard: e.target.checked})}
                    />
                    <span>启用卡维+缓冲守护</span>
                </label>
            </section>

            <section className="panel">
                <div className="panel-title">退出商店守护</div>
                <p className="hint" style={{marginTop: 0, lineHeight: 1.5}}>
                    若当前护身符中<strong>没有</strong>携带「生命」印章（600100），阻止退出商店。
                </p>

                <label className="row" style={{gridTemplateColumns: "auto 1fr"}}>
                    <input
                        className="form-checkbox"
                        type="checkbox"
                        checked={Boolean(config.enable_exit_life_guard)}
                        onChange={(e) => patchFuseConfig({enable_exit_life_guard: e.target.checked})}
                    />
                    <span>启用退出商店守护</span>
                </label>
            </section>

            <button className="nav-btn" onClick={onSave} disabled={saving}>
                {saving ? "保存中…" : "保存熔断配置"}
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
import React, {act} from "react";
import {createRoot} from "react-dom/client";
import {renderToStaticMarkup} from "react-dom/server";
import {mockIPC, clearMocks} from "@tauri-apps/api/mocks";
import i18n from "i18next";
import {initReactI18next} from "react-i18next";
import LevelRecordPanel from "../src/components/LevelRecordPanel";
import type {MagicVineGame} from "../src/bindings";
import zh from "../src/locales/zh-CN.json";
import MagicVineStats from "../src/components/MagicVineStats";

export async function runMagicVineHighestRenderTests() {
    await i18n.use(initReactI18next).init({lng:"zh-CN",resources:{"zh-CN":{translation:zh}},interpolation:{escapeValue:false}});
    const markup=renderToStaticMarkup(<MagicVineStats amulets={[{id:3760}]} tileScoreMap={{"1m":"1e200","2p":"2e200","3s":"2e200"}}/>);
    if (!markup.includes("2.00e200")) throw new Error("highest score must retain scientific precision and must not sum matching scores");
    if (markup.indexOf(zh.magic_vine.highest_score)>markup.indexOf(zh.magic_vine.title)) throw new Error("highest score must be above the statistics");
    if (renderToStaticMarkup(<MagicVineStats tileScoreMap={{"1m":"1e200"}}/>) !== "") throw new Error("both vine rows must be hidden until the first acquisition");
    const smallMarkup=renderToStaticMarkup(<MagicVineStats amulets={[{id:3761}]} tileScoreMap={{"1m":"115"}}/>);
    if (!smallMarkup.includes('class="home-record-value">115</span>') || smallMarkup.includes("1.15e2")) throw new Error("small base scores must use the existing homepage metric value style and remain plain");
    if (!smallMarkup.includes('class="home-record-row is-up"') || !smallMarkup.includes('class="home-record-label"')) throw new Error("highest score must reuse the existing homepage metric row and label styles");
    return "Magic vine static rendering checks passed (5 assertions; no UI automation).";
}

export async function runMagicVineStatsTests() {
    await i18n.use(initReactI18next).init({lng:"zh-CN",resources:{"zh-CN":{translation:zh}},interpolation:{escapeValue:false}});
    const assert = (value: unknown, message: string) => { if (!value) throw new Error(message); };
    const game: MagicVineGame = {started_at:1793864876106,updated_at:1793877168962,level:14021,node:1,
        finished:true,incomplete:false,pack_obtained:110,shop_obtained:8,regeneration_obtained:1,other_obtained:0,
        score_triggers:120,expansion_extra_triggers:1,shops:38,packs_bought:1166,rounds:39,highest_ex:21,farming_rounds:10,farming_obtained:50,farming_score_triggers:51,farming_status:"ended",farming_periods:[{start_level:13003,start_node:2,end_level:14005,end_node:2,rounds:10,obtained:50,score_triggers:51}]};
    let current = game;
    mockIPC(command => {
        if (command === "backend_get_magic_vine_stats") return {current,history:[game],loading:false};
        if (command === "plugin:event|listen") return 1;
        if (command === "plugin:event|unlisten") return null;
        throw new Error(`Unexpected IPC: ${command}`);
    });
    const container = document.createElement("div");
    document.body.append(container);
    const root = createRoot(container);
    try {
        await act(async () => {root.render(<LevelRecordPanel level={14021} tileScoreMap={{"1m":"3976400000000000000000000000000000000000","2m":"25"}} items={[{
            key:"fan_5_haitei",label:zh.level_records.haitei_fan,current:"1",previousBest:null,best:"1",delta:null,state:"meetsBest",scale:0,
        }]}/>);});
        const button = container.querySelector<HTMLButtonElement>("button")!;
        assert(button && container.querySelector(".home-record-row")?.nextElementSibling?.getAttribute("data-testid") === "magic-vine-highest-score" && container.querySelector("[data-testid=magic-vine-highest-score]")?.nextElementSibling === button,"highest score and summary must appear below haitei in order");
        assert(button.textContent?.includes("119") && button.textContent.includes("5.00") && button.textContent.includes("38 / 8") && !button.textContent.includes("1166"),"summary must show total, average, shop visits and direct vine purchases, excluding pack purchases");
        assert(!button.textContent?.includes("120"),"trigger details should stay in the dialog");
        await act(async () => {button.click();});
        const dialog = document.querySelector('[role="dialog"]')!;
        const row = dialog.querySelector("tbody tr")!;
        assert(dialog && dialog.querySelector(".modal-header")?.textContent?.includes(zh.magic_vine.states.ended) && !dialog.querySelector(".modal-body > p") && row.textContent?.includes("110") && row.textContent.includes("120") && row.textContent.includes("38 / 8") && !row.textContent.includes("1166"),"dialog must put the phase in its header, omit explanatory paragraphs and retain the statistics");
        assert(row.textContent?.includes("+1"),"expansion extra triggers must be separately labelled");
        assert(!dialog.querySelector("tfoot"),"history must not include an aggregate total row");
        await act(async () => {dialog.querySelector<HTMLButtonElement>("button")!.click();});
        assert(!document.querySelector('[role="dialog"]'),"dialog must close");
        current={...game,pack_obtained:0,shop_obtained:0,regeneration_obtained:0,other_obtained:0};
        await act(async () => {root.render(<></>);});
        await act(async () => {root.render(<LevelRecordPanel level={13001} items={[]}/>);});
        assert(!container.querySelector("button") && !container.querySelector('[data-testid="magic-vine-highest-score"]'),"a new game without vines must hide the component even when old history exists");
        return "Magic vine UI checks passed (8 assertions, mock IPC only).";
    } finally {
        await act(async () => {root.unmount();});
        container.remove();
        clearMocks();
    }
}

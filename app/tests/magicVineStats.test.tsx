import React, {act} from "react";
import {createRoot} from "react-dom/client";
import {renderToStaticMarkup} from "react-dom/server";
import {mockIPC, clearMocks} from "@tauri-apps/api/mocks";
import i18n from "i18next";
import {initReactI18next} from "react-i18next";
import LevelRecordPanel, {useLevelRecordItems, MAX_TILE_SCORE_METRIC_KEY} from "../src/components/LevelRecordPanel";
import type {EffectItem} from "../src/lib/gamestate";
import type {MagicVineGame} from "../src/bindings";
import zh from "../src/locales/zh-CN.json";
import MagicVineStats from "../src/components/MagicVineStats";
import AboutPage from "../src/pages/AboutPage";

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
    let current: MagicVineGame | null = game;
    mockIPC(command => {
        if (command === "backend_get_magic_vine_stats") return {current,history:[game],loading:false};
        if (command === "plugin:event|listen") return 1;
        if (command === "plugin:event|unlisten") return null;
        throw new Error(`Unexpected IPC: ${command}`);
    });
    const container = document.createElement("div");
    document.body.append(container);
    const root = createRoot(container);
    const recordKey="sl-level-metric-records-v1", savedRecords=localStorage.getItem(recordKey);
    const probeAmulets: EffectItem[]=[{id:3760,uid:1,volume:1,store:[],tags:[]}];
    const probeFans: Record<string,string>={};
    function HighestProbe({level,scores}: {level:number;scores:Record<string,string>}) {
        const items=useLevelRecordItems({level,tileScoreMap:scores,fanValueMap:probeFans,amulets:probeAmulets});
        return <LevelRecordPanel level={level} items={items} tileScoreMap={scores} amulets={probeAmulets}/>;
    }
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
        assert(dialog.querySelector("thead")?.textContent?.includes("神秘商店购买"),"direct vine purchases must explicitly be labelled as mysterious shop purchases");
        assert(dialog && dialog.querySelector(".modal-header")?.textContent?.includes(zh.magic_vine.states.ended) && !dialog.querySelector(".modal-body > p") && row.textContent?.includes("110") && row.textContent.includes("120") && row.textContent.includes("38 / 8") && !row.textContent.includes("1166"),"dialog must put the phase in its header, omit explanatory paragraphs and retain the statistics");
        assert(row.textContent?.includes("+1"),"expansion extra triggers must be separately labelled");
        assert(!dialog.querySelector("tfoot"),"history must not include an aggregate total row");
        await act(async () => {dialog.querySelector<HTMLButtonElement>("button")!.click();});
        assert(!document.querySelector('[role="dialog"]'),"dialog must close");
        current={...game,started_at:game.started_at+1,pack_obtained:105,shop_obtained:9,shops:37,farming_obtained:43};
        await act(async () => {root.render(<></>);});
        await act(async () => {root.render(<LevelRecordPanel level={14021} items={[]}/>);});
        assert(Array.from(container.querySelectorAll('[data-trend]')).map(node => node.getAttribute('data-trend')).join(',')==='down,down,down,up',
            "total, farming average and shop visits below historical best turn red while higher purchases stay green");
        current={...game,pack_obtained:0,shop_obtained:0,regeneration_obtained:0,other_obtained:0};
        await act(async () => {root.render(<></>);});
        await act(async () => {root.render(<LevelRecordPanel level={13001} items={[]}/>);});
        assert(!container.querySelector("button") && !container.querySelector('[data-testid="magic-vine-highest-score"]'),"a new game without vines must hide the component even when old history exists");
        current = null;
        await act(async () => {root.render(<AboutPage onSecretClick={() => {}} onShowUsageNotice={() => {}}
            onCheckUpdate={() => {}} updateAvailable={null} checkingUpdate={false}/>);});
        const historyButton = Array.from(container.querySelectorAll("button")).find(button => button.textContent?.includes(zh.magic_vine.title));
        assert(historyButton,"the about page must keep the history entry visible after the game ends");
        assert(historyButton?.nextElementSibling?.textContent === zh.about.show_usage_notice,"the history entry must be immediately to the left of the usage notice");
        assert(historyButton?.querySelector('.ms') && !container.querySelector('[data-testid="magic-vine-highest-score"]'),"the about entry must have an icon and omit homepage metrics");
        await act(async () => {historyButton!.click();});
        const historyDialog = document.querySelector('[role="dialog"]')!;
        assert(historyDialog?.getAttribute("aria-label") === zh.magic_vine.history_title,"the about entry must open the shared history dialog without a current game");
        assert(historyDialog?.querySelector("tbody tr")?.textContent?.includes("119") && historyDialog.querySelector("thead")?.textContent?.includes("神秘商店购买"),"the about dialog must use the same historical data and labels as the homepage");
        localStorage.removeItem(recordKey);
        const highestRow=() => container.querySelector('[data-testid="magic-vine-highest-score"]')!;
        await act(async () => {root.render(<HighestProbe level={14069} scores={{"1m":"1e200"}}/>);});
        assert(highestRow().classList.contains('is-up'),"first observation establishes a green per-level record");
        const stored=JSON.parse(localStorage.getItem(recordKey)!);
        assert(stored['14069'][MAX_TILE_SCORE_METRIC_KEY]==='1'+'0'.repeat(202),"highest base score is persisted exactly, including scale");
        await act(async () => {root.render(<HighestProbe level={14069} scores={{"9m":"9e199"}}/>);});
        assert(highestRow().classList.contains('is-down') && highestRow().querySelector('.home-record-meta')?.textContent?.includes('1.00e200'),"lower base scores turn red and show the previous same-level record");
        assert(container.querySelectorAll('[data-testid="magic-vine-highest-score"]').length===1,"the new metric must not create a duplicate row");
        await act(async () => {root.render(<HighestProbe level={14069} scores={{"1p":"1e200"}}/>);});
        assert(highestRow().classList.contains('is-up'),"matching the record turns green");
        await act(async () => {root.render(<HighestProbe level={14070} scores={{"1p":"9e199"}}/>);});
        assert(highestRow().classList.contains('is-up'),"different levels do not share a comparison baseline");
        return "Magic vine UI and per-level highest base score checks passed (mock IPC only).";
    } finally {
        await act(async () => {root.unmount();});
        container.remove();
        if (savedRecords==null) localStorage.removeItem(recordKey);else localStorage.setItem(recordKey,savedRecords);
        clearMocks();
    }
}

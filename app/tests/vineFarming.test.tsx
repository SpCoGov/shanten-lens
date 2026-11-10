import React, {act} from "react";
import {createRoot} from "react-dom/client";
import {mockIPC, clearMocks} from "@tauri-apps/api/mocks";
import i18n from "i18next";
import {initReactI18next} from "react-i18next";
import AutoRunnerPage from "../src/pages/AutoRunnerPage";
import ActionProviderPanel from "../src/components/ActionProviderPanel";
import VineFarmingOptions from "../src/components/VineFarmingOptions";
import {validateAutoPlayConfig, type VineFarmingProfile} from "../src/lib/autoPlayConfig";
import type {GameStateData} from "../src/lib/gamestate";
import {getAutoRunnerSnapshot, setAutoConfig, setAutoStatus} from "../src/lib/autoRunnerStore";
import zh from "../src/locales/zh-CN.json";

export async function runVineFarmingTests() {
    await i18n.use(initReactI18next).init({lng:"zh-CN",resources:{"zh-CN":{translation:zh}},interpolation:{escapeValue:false}});
    const calls: {command: string; args: unknown}[] = [];
    mockIPC((command,args) => {calls.push({command,args}); return null;}, {shouldMockEvents: true});
    const profile = {id:"vine",name:"Vine",strategy:"vine_farming" as const};
    setAutoConfig({playbook:{version:1,selected_profile_id:profile.id,profiles:[profile]}});
    const container = document.createElement("div");
    document.body.append(container);
    const root = createRoot(container);
    const assert = (value: unknown, message: string) => {if (!value) throw new Error(message);};
    try {
        await act(async () => {root.render(<AutoRunnerPage currentState={null}/>);});
        assert(container.querySelectorAll("nav button").length === 3,"only one monitor tab remains");
        assert(!container.textContent?.includes("玩法模式"),"mode selection panel was removed");
        assert(calls.length === 0,"viewing the monitor must not save configuration or operate a game");
        const vinePanel = Array.from(container.querySelectorAll("section")).find(section => section.textContent?.includes(zh.vine.first_win.split("{{")[0]));
        assert(vinePanel && !vinePanel.querySelector(".hint"),"vine monitor shows the first-win score without the long rule description");
        await act(async () => {setAutoStatus({...getAutoRunnerSnapshot().status,running:true,active_profile:profile});});
        const remake = container.querySelector<HTMLButtonElement>(`button[title="${zh.vine.remake_forbidden}"]`);
        assert(remake?.disabled,"manual remake is disabled during farming");
        await act(async () => {setAutoStatus({...getAutoRunnerSnapshot().status,running:false,current_step:"vine_echo",last_error:"vine.echo_found"});});
        assert(container.textContent?.includes(zh.autoplay.step_vine_echo),"terminal echo status stays visible");
        assert(container.textContent?.includes(zh.vine.echo_found),"handoff reason is visible");
        await act(async () => {setAutoStatus({...getAutoRunnerSnapshot().status,running:false,current_step:"vine_goal",last_error:"vine.goal_met"});});
        assert(container.textContent?.includes(zh.vine.goal_met),"insufficient first-win handoff reason is visible");
        assert(zh.vine.goal_met.includes("不足以通过下一关"),"first-win stop rule must not be reversed");
        const action = (kind: string) => ({id:kind,kind,params:{goods_id:103},availability:"available",reasons:[],cost:null,cost_resource:"coin"});
        const snapshot = {source:"packet",level:14017,node:1,action_report:{stage:9,status:"ready",actions:[action("buy_pack"),action("sell_effect")],global_actions:[action("remake")],issues:[]},
            decision_report:{actions:["buy_pack","sell_effect","remake"].map(kind => ({action_id:kind,total:kind === "buy_pack" ? 99 : -10197,
                rules:[{rule_id:kind === "buy_pack" ? "vine_pack" : "vine_protected",score:0,reason:{code:"vine_protected",values:{}}}]}))}} as unknown as GameStateData;
        await act(async () => {root.render(<ActionProviderPanel state={snapshot} execution={{disabled:false,pending:null,waiting:false,feedback:null,execute:async () => {throw new Error("must not execute");}}}/>);});
        const rows = Array.from(container.querySelectorAll<HTMLButtonElement>("li button"));
        assert(rows.length === 3 && rows.filter(b => !b.disabled).length === 1,"only the gold pack is actionable in the offline snapshot");
        assert(container.textContent?.includes(zh.vine.protected_action),"protected actions have an explicit strategy label");
        let edited: VineFarmingProfile = {...profile};
        const renderOptions = () => root.render(<VineFarmingOptions profile={edited} onChange={next => {edited=next;renderOptions();}}/>);
        await act(async () => {renderOptions();});
        const sliders = () => Array.from(container.querySelectorAll<HTMLButtonElement>('[role="slider"]'));
        assert(sliders().length===2 && sliders().every(slider => slider.getAttribute('aria-valuenow')==='8'),"both independent arrows start at unlimited, without preset caps");
        assert(container.querySelector('img[src$="arrow_up_FuJi.png"]') && container.querySelector('img[src$="arrow_down_Lion.png"]'),"use the requested character images");
        await act(async () => {sliders()[0].dispatchEvent(new KeyboardEvent('keydown',{key:'Home',bubbles:true}));});
        assert(edited.reward_refresh_limit===62 && edited.normal_refresh_limit==null,"reward pointer is independent and cannot reach hidden prices");
        await act(async () => {sliders()[1].dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowLeft',bubbles:true}));});
        assert(edited.normal_refresh_limit===1065,"normal pointer snaps to the final visible interval");
        const capture = HTMLElement.prototype.setPointerCapture;
        HTMLElement.prototype.setPointerCapture=()=>{};
        try {
            const axis=sliders()[0].parentElement!;
            axis.getBoundingClientRect=() => ({left:0,width:1000} as DOMRect);
            await act(async () => {sliders()[0].dispatchEvent(new PointerEvent('pointerdown',{button:0,pointerId:1,bubbles:true}));});
            await act(async () => {sliders()[0].dispatchEvent(new PointerEvent('pointermove',{clientX:625,pointerId:1,bubbles:true}));});
            await act(async () => {sliders()[0].dispatchEvent(new PointerEvent('pointerup',{pointerId:1,bubbles:true}));});
            assert(edited.reward_refresh_limit===473,"dragging near a tick snaps to an interval midpoint, not the tick itself");
        } finally {HTMLElement.prototype.setPointerCapture=capture;}
        await act(async () => {sliders()[0].dispatchEvent(new KeyboardEvent('keydown',{key:'End',bubbles:true}));});
        assert(edited.reward_refresh_limit==null && edited.normal_refresh_limit===1065,"End resets only that arrow to unlimited");
        const cutoff=container.querySelector<HTMLSelectElement>('select')!;
        await act(async () => {cutoff.value='70';cutoff.dispatchEvent(new Event('change',{bubbles:true}));});
        assert(edited.stop_ex===70 && cutoff.options.length===71,"cutoff choices are Ex1 through Ex70 plus the original stop condition");
        const half=container.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
        assert(half.checked,"half-health protection defaults on for legacy profiles");
        await act(async () => {half.click();});
        assert(edited.pause_half_hp===false,"half-health protection can be disabled");
        assert(validateAutoPlayConfig(JSON.parse(JSON.stringify({version:1,selected_profile_id:profile.id,profiles:[edited]})))===null,"all options survive persisted profile validation");
        return "Vine farming UI: single monitor, no mode panel, remake lock and Echo handoff passed.";
    } finally {
        await act(async () => {root.unmount();});
        container.remove(); clearMocks();
    }
}

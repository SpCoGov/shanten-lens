import React, {act} from "react";
import {createRoot} from "react-dom/client";
import {mockIPC, clearMocks} from "@tauri-apps/api/mocks";
import i18n from "i18next";
import {initReactI18next} from "react-i18next";
import AutoRunnerPage from "../src/pages/AutoRunnerPage";
import {getAutoRunnerSnapshot, setAutoConfig, setAutoStatus, patchAutoConfig, markPlaybookSaved} from "../src/lib/autoRunnerStore";
import type {AutoPlayConfig} from "../src/lib/autoPlayConfig";
import zh from "../src/locales/zh-CN.json";

export async function runAutoPlaySelectionTests() {
    await i18n.use(initReactI18next).init({lng:"zh-CN",resources:{"zh-CN":{translation:zh}},interpolation:{escapeValue:false}});
    const calls: {command:string; args:unknown}[]=[];
    let fail=false;
    mockIPC((command,args)=>{calls.push({command,args});if(fail)throw new Error("simulated selection failure");return null;},{shouldMockEvents:true});
    const book: AutoPlayConfig={version:1,selected_profile_id:"a",profiles:[{id:"a",name:"Profile A",strategy:"vine_farming"},{id:"b",name:"Profile B",strategy:"vine_farming"}]};
    setAutoConfig({playbook:book});markPlaybookSaved(book);
    setAutoStatus({...getAutoRunnerSnapshot().status,running:false,current_step:"completed",active_profile:{...book.profiles[1],name:"Old B"}});
    const container=document.createElement("div");document.body.append(container);const root=createRoot(container);
    const assert=(condition:unknown,message:string)=>{if(!condition)throw new Error(message);};
    const tab=(name:string)=>Array.from(container.querySelectorAll<HTMLButtonElement>("nav button")).find(b=>b.textContent?.includes(name))!;
    const select=(name:string)=>Array.from(container.querySelectorAll<HTMLButtonElement>('[aria-labelledby="autoplay-profile-label"] button')).find(b=>b.textContent?.includes(name))!;
    try {
        await act(async()=>root.render(<AutoRunnerPage currentState={null}/>));
        await act(async()=>tab(zh.autoplay.tab_strategy).click());
        await act(async()=>{const draft=structuredClone(book);draft.profiles[0].name="Draft A";patchAutoConfig({playbook:draft});});
        await act(async()=>select("Profile B").click());
        assert(getAutoRunnerSnapshot().config.playbook.selected_profile_id==="b","selection changes immediately");
        assert(JSON.stringify(calls[0])===JSON.stringify({command:"backend_update_config",args:{config:{autorun:{playbook:{selected_profile_id:"b"}}}}}),"only selected id is synchronized");
        assert(getAutoRunnerSnapshot().config.playbook.profiles[0].name==="Draft A" && getAutoRunnerSnapshot().playbookDirty,"unrelated draft remains unsaved");
        await act(async()=>tab(zh.autoplay.tab_monitor).click());
        assert(container.querySelector('h3') && container.textContent?.includes("配置：Profile B"),"monitor follows the selection instead of old completed profile");
        await act(async()=>tab(zh.autoplay.tab_strategy).click());
        fail=true;
        await act(async()=>select("Draft A").click());
        assert(getAutoRunnerSnapshot().config.playbook.selected_profile_id==="b","failed synchronization rolls back the selection");
        fail=false;
        const count=calls.length;
        await act(async()=>setAutoStatus({...getAutoRunnerSnapshot().status,running:true,active_profile:book.profiles[1]}));
        await act(async()=>select("Draft A").click());
        assert(calls.length===count,"changing a future selection does not modify the running configuration");
        await act(async()=>tab(zh.autoplay.tab_monitor).click());
        assert(container.textContent?.includes("配置：Profile B"),"running monitor keeps the active snapshot");
        assert(calls.every(c=>c.command==="backend_update_config"),"no start, stop, or game operation is sent");
        return "Profile selection: immediate monitor update, selection-only scoring sync, draft preservation, rollback and running-snapshot protection passed.";
    } finally {await act(async()=>root.unmount());container.remove();clearMocks();}
}

use super::{event, Services};
use crate::{ipc::PacketLogItem, pipeline::Direction, storage::write_json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use specta::Type;
use std::{collections::{BTreeSet, HashMap}, fs, io::{BufRead, BufReader}, path::Path};
mod farming;
pub use farming::FarmingPeriod;

const RULES_VERSION: u32 = 4;
// Inclusive Sydney cutoff: 2026-11-04 11:00:21 AEDT, including that entire second.
const HISTORY_CUTOFF: u64 = 1_793_750_421_999;

#[derive(Clone, Debug, Default, Deserialize, Serialize, Type, PartialEq)]
#[serde(default)]
pub struct MagicVineGame {
    pub started_at: u64,
    pub updated_at: u64,
    pub level: u64,
    pub node: u64,
    pub finished: bool,
    pub incomplete: bool,
    pub pack_obtained: u64,
    pub shop_obtained: u64,
    pub regeneration_obtained: u64,
    pub other_obtained: u64,
    pub score_triggers: u64,
    pub expansion_extra_triggers: u64,
    pub shops: u64,
    pub packs_bought: u64,
    pub rounds: u64,
    pub highest_ex: u64,
    pub farming_rounds: u64,
    pub farming_obtained: u64,
    pub farming_score_triggers: u64,
    pub farming_status: String,
    pub farming_periods: Vec<FarmingPeriod>,
}

impl MagicVineGame {
    fn obtained(&self) -> u64 {
        self.pack_obtained + self.shop_obtained + self.regeneration_obtained + self.other_obtained
    }

    fn qualifies(&self) -> bool {
        self.finished && self.started_at > HISTORY_CUTOFF && self.highest_ex > 5 && self.obtained() > 20
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Type)]
pub struct MagicVineStats {
    pub current: Option<MagicVineGame>,
    pub history: Vec<MagicVineGame>,
    pub loading: bool,
}

#[derive(Default, Deserialize, Serialize)]
pub(super) struct MagicVineStore {
    #[serde(default)]
    rules_version: u32,
    stats: MagicVineStats,
    last_ts_ms: u64,
    #[serde(default)]
    last_responses: Vec<(String, Option<u32>)>,
    context: Value,
    #[serde(skip)]
    requests: HashMap<(String, u32), Value>,
    #[serde(skip)]
    pending: Vec<PacketLogItem>,
    #[serde(default)]
    farming: farming::Farming,
    #[serde(default)]
    checkpoints: Vec<Checkpoint>,
    #[serde(default)]
    needs_resume_rebuild: bool,
}

#[derive(Deserialize, Serialize)]
struct Checkpoint {
    game: MagicVineGame,
    context: Value,
    farming: farming::Farming,
}

fn unwrapped(value: &Value) -> &Value { value.get("value").unwrap_or(value) }
fn vine(value: &Value) -> bool { matches!(value.as_u64(), Some(3760 | 3761)) }
fn items(value: &Value) -> &[Value] { unwrapped(value).as_array().map(Vec::as_slice).unwrap_or(&[]) }

const FIELDS: &[(&str, &[&str])] = &[
    ("map", &["level", "node"]), ("record", &["roundCount", "packCount"]),
    ("effect", &["effectList"]), ("nodeEvent", &["advancedShopGoods"]),
];

fn snapshot_context(game: &Value) -> Value {
    let mut context = json!({"state":game["state"]});
    for (section, keys) in FIELDS {
        for key in *keys { context[*section][*key] = game[*section][*key].clone(); }
    }
    context
}

// Hook deltas can exceed f64; only the sign/nonzero coefficient matters here.
fn positive(value: &Value) -> bool {
    let text = value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string());
    let mantissa = text.split(['e', 'E']).next().unwrap_or("");
    !mantissa.starts_with('-') && mantissa.bytes().any(|digit| matches!(digit, b'1'..=b'9'))
        && text.parse::<f64>().is_ok()
}

impl MagicVineStore {
    fn remember_current(&mut self) {
        if let Some(game) = self.stats.current.as_ref().filter(|game| game.packs_bought > 0 || game.obtained() > 0) {
            self.checkpoints.retain(|saved| saved.game.started_at != game.started_at);
            self.checkpoints.push(Checkpoint {game:game.clone(),context:self.context.clone(),farming:self.farming.clone()});
            if self.checkpoints.len() > 100 { self.checkpoints.remove(0); }
        }
    }

    fn resume(&mut self, game: &Value) {
        if game.pointer("/state/current").and_then(Value::as_u64) == Some(100) { return; }
        let context = snapshot_context(game);
        // Progress alone is not an identity: require the exact nonempty inventory, including server UIDs/stores.
        let index = self.checkpoints.iter().rposition(|saved| saved.game.packs_bought > 0
            && !items(&context["effect"]["effectList"]).is_empty()
            && ["map", "record", "effect"].iter().all(|key| saved.context[*key] == context[*key]));
        if let Some(index) = index {
            let saved = self.checkpoints.remove(index);
            self.remember_current();
            self.stats.history.retain(|old| old.started_at != saved.game.started_at);
            self.stats.current = Some(saved.game);
            self.context = saved.context;
            self.farming = saved.farming;
        }
    }

    pub(super) fn read(path: &Path) -> Self {
        let mut store: Self = fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        if store.rules_version == RULES_VERSION { store }
        else if store.rules_version == 3 {
            store.rules_version = RULES_VERSION;
            store.needs_resume_rebuild = true;
            store
        }
        else { Self {rules_version: RULES_VERSION, ..Default::default()} }
    }

    pub(super) fn load(path: &Path, logs: &Path) -> Self {
        let mut store = Self::read(path);
        let cached = if store.needs_resume_rebuild && fs::read_dir(logs).into_iter().flatten().flatten().any(|entry|
            entry.file_name().to_string_lossy().starts_with("shanten-lens-packets-") && entry.metadata().is_ok_and(|meta| meta.len() > 0)) {
            Some(std::mem::replace(&mut store, Self {rules_version:RULES_VERSION,..Default::default()}))
        } else { None };
        let mut files: Vec<_> = fs::read_dir(logs).into_iter().flatten().flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("shanten-lens-packets-"))
            .filter(|entry| entry.metadata().and_then(|meta| meta.modified()).ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .is_some_and(|time| time.as_millis() as u64 >= store.last_ts_ms))
            .map(|entry| entry.path()).collect();
        files.sort();
        let before = store.last_ts_ms;
        for file in files {
            let Ok(file) = fs::File::open(file) else { continue };
            for line in BufReader::new(file).lines() {
                let Ok(line) = line else { break };
                if !line.contains("amuletActivity") && !line.contains("fetchAmuletActivityData") { continue; }
                if let Ok(packet) = serde_json::from_str::<PacketLogItem>(&line) { store.observe(&packet); }
            }
        }
        if store.stats.current.is_none() && store.stats.history.is_empty() {
            if let Some(cached) = cached { return cached; }
        }
        if store.last_ts_ms != before {
            if let Err(error) = write_json(path, &store) { tracing::warn!(%error, "Could not save magic vine statistics"); }
        }
        store
    }

    fn finish(&mut self) {
        if let Some(mut game) = self.stats.current.take() {
            self.farming.finish(&mut game, false);
            game.updated_at = self.last_ts_ms;
            game.finished = true;
            if game.qualifies() {
                self.stats.history.insert(0, game);
                self.stats.history.truncate(100);
            }
        }
        self.context = json!({});
    }

    pub(super) fn snapshot(&self) -> MagicVineStats {
        let mut stats = self.stats.clone();
        stats.history.retain(MagicVineGame::qualifies);
        stats.history.truncate(100);
        stats
    }

    // Return true only when visible counters change; ordinary draws/Hu never cause disk writes.
    pub(super) fn observe(&mut self, packet: &PacketLogItem) -> bool {
        if !packet.method.contains("amuletActivity") && !packet.method.ends_with("fetchAmuletActivityData") { return false; }
        if packet.direction == Direction::Outbound && packet.packet_type == "Req" {
            if self.requests.len() > 1000 { self.requests.clear(); }
            if let Some(id) = packet.id { self.requests.insert((packet.method.clone(), id), packet.data.clone()); }
            return false;
        }
        if packet.direction != Direction::Inbound || packet.packet_type != "Res" || packet.data.get("error").is_some()
            || packet.ts_ms < self.last_ts_ms { return false; }
        let key = (packet.method.clone(), packet.id);
        if packet.ts_ms == self.last_ts_ms && self.last_responses.contains(&key) { return false; }
        if packet.ts_ms > self.last_ts_ms { self.last_responses.clear(); }
        self.last_responses.push(key.clone());
        let request = packet.id.and_then(|id| self.requests.remove(&(packet.method.clone(), id)));
        let previous = self.stats.clone();
        self.last_ts_ms = packet.ts_ms;
        let data = &packet.data;
        if packet.method.ends_with("amuletActivityStartGame") {
            self.remember_current();
            self.finish();
            self.stats.current = Some(MagicVineGame {started_at: packet.ts_ms, ..Default::default()});
        }
        if packet.method.ends_with("amuletActivityGiveup") {
            self.finish();
        } else if packet.method.ends_with("fetchAmuletActivityData") {
            if let Some(game) = data.pointer("/data/game").or_else(|| data.get("game")) {
                self.resume(game);
                if self.stats.current.is_none() && game.pointer("/state/current").and_then(Value::as_u64) == Some(100) { return false; }
                let record = &game["record"];
                let reset = self.stats.current.as_ref().is_some_and(|current|
                    record["packCount"].as_u64().is_some_and(|count| count < current.packs_bought)
                    || record["roundCount"].as_u64().is_some_and(|count| count < current.rounds));
                if reset { self.remember_current(); self.finish(); }
                if self.stats.current.is_none() {
                    self.stats.current = Some(MagicVineGame {started_at: packet.ts_ms, incomplete: true, ..Default::default()});
                }
                if let Some(current) = self.stats.current.as_mut() {
                    // A snapshot cannot recover unobserved acquisition hooks.
                    if record["packCount"].as_u64().unwrap_or(0) > current.packs_bought
                        || record["roundCount"].as_u64().unwrap_or(0) > current.rounds {
                        current.incomplete = true;
                        self.farming.finish(current, true);
                    }
                }
                let was_mystery = self.context.pointer("/state/current").and_then(Value::as_u64) == Some(11);
                self.context = snapshot_context(game);
                self.progress();
                if !was_mystery && game.pointer("/state/current").and_then(Value::as_u64) == Some(11) {
                    self.stats.current.as_mut().unwrap().shops += 1;
                }
                if game.pointer("/state/current").and_then(Value::as_u64) == Some(100) { self.finish(); }
            }
        } else {
            for event in items(&data["events"]) {
                if self.stats.current.is_none() { continue; }
                let before = self.context.clone();
                if let Some(game) = event.pointer("/result/newGameResult") { self.context = snapshot_context(game); }
                for (section, keys) in FIELDS {
                    for key in *keys {
                        if let Some(value) = event["valueChanges"][*section].get(*key) {
                            if value["dirty"] != false { self.context[*section][*key] = unwrapped(value).clone(); }
                        }
                    }
                }
                if let Some(state) = event.get("state") { self.context["state"] = state.clone(); }
                self.progress();
                let current = self.stats.current.as_ref().unwrap();
                let (obtained, triggers) = (current.obtained(), current.score_triggers);
                self.count_event(event, &before, request.as_ref());
                let current = self.stats.current.as_mut().unwrap();
                let event_type = event["type"].as_u64().unwrap_or(0);
                if self.context.pointer("/state/current").and_then(Value::as_u64) == Some(11)
                    && (before.pointer("/state/current").and_then(Value::as_u64) != Some(11)
                        || before["map"] != self.context["map"]) { current.shops += 1; }
                if event_type != 100 && event.pointer("/state/current").and_then(Value::as_u64) != Some(100) {
                    self.farming.observe(current, &self.context, event_type, current.obtained() - obtained, current.score_triggers - triggers);
                }
                if event["type"] == 100 || event.pointer("/state/current").and_then(Value::as_u64) == Some(100) { self.finish(); }
            }
        }
        let changed = self.stats.current != previous.current || self.stats.history != previous.history;
        if changed {
            if let Some(game) = self.stats.current.as_mut() { game.updated_at = packet.ts_ms; }
        }
        changed
    }

    fn progress(&mut self) {
        let Some(game) = self.stats.current.as_mut() else { return };
        game.level = self.context.pointer("/map/level").and_then(Value::as_u64).filter(|level| *level > 0).unwrap_or(game.level);
        game.node = self.context.pointer("/map/node").and_then(Value::as_u64).filter(|node| *node > 0).unwrap_or(game.node);
        game.rounds = self.context.pointer("/record/roundCount").and_then(Value::as_u64).unwrap_or(game.rounds);
        game.packs_bought = self.context.pointer("/record/packCount").and_then(Value::as_u64).unwrap_or(game.packs_bought);
        game.highest_ex = game.highest_ex.max(farming::ex_number(game.level));
    }

    fn count_event(&mut self, event: &Value, before: &Value, request: Option<&Value>) {
        let game = self.stats.current.as_mut().unwrap();
        let selected = vine(&event["result"]["selectPackResult"]["id"]);
        if selected { game.pack_obtained += 1; }
        let bought = event["type"] == 34 && items(&self.context["nodeEvent"]["advancedShopGoods"]).iter().any(|good|
            vine(&good["effect"]["id"]) && good["sold"].as_u64().unwrap_or(0) != 0
                && (request.is_some_and(|req| req["type"] == 10 && req["args"][0] == good["id"])
                    || items(&before["nodeEvent"]["advancedShopGoods"]).iter().any(|old| old["id"] == good["id"] && old["sold"] == 0)));
        if bought { game.shop_obtained += 1; }
        let hooks = items(&event["effectedHooks"]);
        let mut regenerated = BTreeSet::new();
        let mut other = BTreeSet::new();
        for hook in hooks {
            for effect in items(&hook["result"]["addEffect"]) {
                if !vine(&effect["id"]) { continue; }
                let uid = effect["uid"].as_u64().unwrap_or(0);
                if hook["id"] == 600080 { regenerated.insert(uid); }
                else if !selected && !bought { other.insert(uid); }
            }
            if !selected && !bought {
                for transformed in items(&hook["result"]["transformEffect"]) {
                    if vine(&transformed["addResult"]["id"]) {
                        other.insert(transformed["addResult"]["uid"].as_u64().unwrap_or(0));
                    }
                }
                for copied in items(&hook["result"]["copyEffect"]) {
                    let uid = copied["uid"].as_u64().unwrap_or(0);
                    if items(&self.context["effect"]["effectList"]).iter()
                        .any(|effect| effect["uid"].as_u64() == Some(uid) && vine(&effect["id"])) { other.insert(uid); }
                }
            }
        }
        game.regeneration_obtained += regenerated.len() as u64;
        game.other_obtained += other.difference(&regenerated).count() as u64;
        let mut triggers = HashMap::<u64, u64>::new();
        for hook in hooks.iter().filter(|hook| vine(&hook["id"])) {
            if items(&hook["result"]["modifyTileScore"]).iter().any(|change| positive(&change["score"])) {
                game.score_triggers += 1;
                *triggers.entry(hook["uid"].as_u64().unwrap_or(0)).or_default() += 1;
            }
        }
        for (uid, count) in triggers {
            let expanded = items(&self.context["effect"]["effectList"]).iter()
                .chain(items(&before["effect"]["effectList"]))
                .any(|effect| effect["uid"].as_u64() == Some(uid) && effect["badge"]["id"] == 600160);
            if expanded { game.expansion_extra_triggers += count.saturating_sub(1); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(ts: u64, method: &str, data: Value) -> PacketLogItem {
        PacketLogItem { ts_ms: HISTORY_CUTOFF + 1000 + ts, direction: Direction::Inbound, packet_type: "Res".into(),
            method: format!(".lq.Lobby.{method}"), id: Some(ts as u32), data }
    }
    fn start(store: &mut MagicVineStore, ts: u64, level: u64) {
        store.observe(&packet(ts, "amuletActivityStartGame", json!({"events":[{"type":1,
            "result":{"newGameResult":{"map":{"level":level,"node":1},"state":{"current":9},
                "record":{"packCount":0,"roundCount":0},"effect":{"effectList":[]}}}}]})));
    }
    fn operate(store: &mut MagicVineStore, ts: u64, event: Value) -> bool {
        store.observe(&packet(ts, "amuletActivityOperate", json!({"events":[event]})))
    }
    fn boost(uid: u64, scores: &[&str]) -> Value {
        json!({"id":3760,"uid":uid,"result":{"modifyTileScore":scores.iter()
            .map(|score| json!({"tile":"1m","score":score})).collect::<Vec<_>>()}})
    }

    #[test]
    fn only_effective_permanent_vine_hooks_count_once_per_hook() {
        let mut store = MagicVineStore::default();
        start(&mut store, 1, 14016);
        operate(&mut store, 2, json!({"type":53,"result":{"selectPackResult":{"id":3760}},
            "effectedHooks":[boost(1, &["10","20","30"]), boost(2, &["0","-1"]),
                {"id":3761,"uid":3,"result":{"modifyTileBaseScore":[{"score":"99"}]}},
                {"id":1501,"uid":4,"result":{"modifyTileScore":[{"score":"20"}]}}]}));
        assert_eq!(store.snapshot().current.unwrap().score_triggers, 1);
        // 白夜升级 / 锤子强化：得到或升级护符不等于触发永久加分。
        assert!(!operate(&mut store, 3, json!({"type":16,"effectedHooks":[boost(1, &[])]})));
        assert!(!operate(&mut store, 4, json!({"type":16,"effectedHooks":[boost(1, &["0e200"])]})));
        operate(&mut store, 5, json!({"type":53,"effectedHooks":[boost(1, &["1.2e400"])]}));
        assert_eq!(store.snapshot().current.unwrap().score_triggers, 2);
        let duplicate = packet(5,"amuletActivityOperate",json!({"events":[{"effectedHooks":[boost(1,&["1"])]}]}));
        assert!(!store.observe(&duplicate));
    }

    #[test]
    fn pack_purchase_regeneration_and_expansion_are_separate() {
        let mut store = MagicVineStore::default();
        start(&mut store, 1, 14016);
        operate(&mut store, 2, json!({"type":53,"result":{"selectPackResult":{"id":3760}},"effectedHooks":[boost(1,&["5"])]}));
        operate(&mut store, 3, json!({"type":33,"state":{"current":11},"valueChanges":{"nodeEvent":{"advancedShopGoods":{"dirty":true,"value":[
            {"id":4,"effect":{"id":3760,"badgeId":600160},"sold":0}]}}}}));
        operate(&mut store, 4, json!({"type":34,"valueChanges":{"nodeEvent":{"advancedShopGoods":{"dirty":true,"value":[
            {"id":4,"effect":{"id":3760,"badgeId":600160},"sold":1}]}},"effect":{"effectList":{"dirty":true,"value":[
            {"id":3760,"uid":2,"badge":{"id":600160}}]}}},"effectedHooks":[boost(2,&["3"]),boost(2,&["8"])]}));
        operate(&mut store, 5, json!({"type":56,"effectedHooks":[
            {"id":600080,"result":{"addEffect":[{"id":3760,"uid":3}]}},boost(3,&["10"])]}));
        let game = store.snapshot().current.unwrap();
        assert_eq!((game.pack_obtained,game.shop_obtained,game.regeneration_obtained),(1,1,1));
        assert_eq!((game.score_triggers,game.expansion_extra_triggers),(4,1));
        operate(&mut store, 6, json!({"type":34,"valueChanges":{"effect":{"effectList":{"dirty":true,"value":[
            {"id":3760,"uid":4,"badge":{"id":600160}}]}}},"effectedHooks":[boost(4,&["0"]),boost(4,&["7"])]}));
        let game = store.snapshot().current.unwrap();
        assert_eq!((game.score_triggers,game.expansion_extra_triggers),(5,1),"zero boost is not an extra trigger");
        operate(&mut store, 7, json!({"type":4,"valueChanges":{"effect":{"effectList":{"dirty":true,"value":[{"id":3761,"uid":5}]}}},
            "effectedHooks":[{"id":600180,"result":{"copyEffect":[{"fromUid":4,"uid":5}]}},boost(5,&["20"])]}));
        operate(&mut store, 8, json!({"type":56,"effectedHooks":[{"id":2311,"result":{"transformEffect":[{"addResult":{"id":3761,"uid":6}}]}},boost(6,&["40"])]}));
        assert_eq!(store.snapshot().current.unwrap().other_obtained,2,"temporary copies and transformed acquisitions are separate from pack purchases");
    }

    #[test]
    fn shop_refreshes_and_fetches_do_not_duplicate_visits_or_packs() {
        let mut store = MagicVineStore::default();
        start(&mut store, 1, 14016);
        for ts in 2..5 { operate(&mut store, ts, json!({"state":{"current":9},"valueChanges":{"record":{"packCount":{"dirty":true,"value":2}}}})); }
        operate(&mut store, 5, json!({"state":{"current":3},"valueChanges":{"map":{"node":{"dirty":true,"value":2}},
            "record":{"roundCount":{"dirty":true,"value":8}}}}));
        operate(&mut store, 6, json!({"type":33,"state":{"current":11}}));
        let context = store.context.clone();
        for ts in 7..10 { store.observe(&packet(ts,"fetchAmuletActivityData",json!({"data":{"game":context}}))); }
        let game = store.snapshot().current.unwrap();
        assert_eq!((game.shops,game.packs_bought,game.rounds),(1,2,8));
        assert!(!game.incomplete);
        // Different replies in the same millisecond must both be counted.
        operate(&mut store, 11, json!({"result":{"selectPackResult":{"id":3760}}}));
        let mut same_ms = packet(11,"amuletActivityOperate",json!({"events":[{"result":{"selectPackResult":{"id":3760}}}]}));
        same_ms.id = Some(99);
        store.observe(&same_ms);
        assert_eq!(store.snapshot().current.unwrap().pack_obtained,2);
        let mut ended = store.context.clone();
        ended["state"]["current"] = json!(100);
        operate(&mut store, 12,json!({"state":{"current":100}}));
        for ts in 13..16 { store.observe(&packet(ts,"fetchAmuletActivityData",json!({"data":{"game":ended}}))); }
        assert!(store.snapshot().history.is_empty(),"short farming runs and repeated ended snapshots must not be recorded");
    }

    #[test]
    fn mystery_shop_visits_exclude_normal_shops_roulette_refreshes_and_allow_revisits() {
        let mut store=MagicVineStore::default();
        start(&mut store,1,14006);
        operate(&mut store,2,json!({"type":24,"state":{"current":9}}));
        operate(&mut store,3,json!({"type":36,"state":{"current":12}}));
        assert_eq!(store.snapshot().current.unwrap().shops,0);
        operate(&mut store,4,json!({"type":33,"state":{"current":11}}));
        operate(&mut store,5,json!({"type":33,"state":{"current":11}}));
        let context=store.context.clone();
        store.observe(&packet(6,"fetchAmuletActivityData",json!({"data":{"game":context}})));
        assert_eq!(store.snapshot().current.unwrap().shops,1);
        operate(&mut store,7,json!({"type":3,"state":{"current":3}}));
        operate(&mut store,8,json!({"type":33,"state":{"current":11}}));
        assert_eq!(store.snapshot().current.unwrap().shops,2,"a new physical visit to the same node counts again");
    }

    #[test]
    fn obsolete_cached_statistics_are_rebuilt_instead_of_reusing_old_shop_and_average_counts() {
        let path=std::env::temp_dir().join(format!("vine-old-cache-{}.json",std::process::id()));
        fs::write(&path,serde_json::to_vec(&MagicVineStore::default()).unwrap()).unwrap();
        let loaded=MagicVineStore::read(&path);
        assert_eq!(loaded.rules_version,RULES_VERSION);
        assert_eq!(loaded.last_ts_ms,0);
        assert!(loaded.snapshot().history.is_empty());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn history_requires_ex6_twenty_one_obtained_and_a_start_after_the_cutoff() {
        let mut store = MagicVineStore::default();
        for n in 0..102 {
            start(&mut store,n*2+1,14006);
            store.stats.current.as_mut().unwrap().other_obtained=21;
        }
        // The live game does not consume one of the hundred finished-history slots.
        assert_eq!(store.snapshot().history.len(),100);
        assert_eq!(store.snapshot().history[0].started_at,HISTORY_CUTOFF+1000+201);
        start(&mut store,205,14005);
        store.stats.current.as_mut().unwrap().pack_obtained=99;
        start(&mut store,207,14006);
        store.stats.current.as_mut().unwrap().pack_obtained=20;
        start(&mut store,209,14006);
        store.stats.current.as_mut().unwrap().started_at=HISTORY_CUTOFF;
        store.stats.current.as_mut().unwrap().pack_obtained=99;
        start(&mut store,211,14006);
        assert_eq!(store.snapshot().history.len(),100);
        assert_eq!(store.snapshot().history[0].started_at,HISTORY_CUTOFF+1000+203);
        assert!(store.snapshot().history.iter().all(|game| game.obtained()>20 && game.started_at>HISTORY_CUTOFF && game.highest_ex>5));
    }

    #[test]
    fn interrupted_login_and_placeholder_game_restore_the_same_saved_run() {
        let mut store = MagicVineStore::default();
        start(&mut store,1,14017);
        operate(&mut store,2,json!({"result":{"selectPackResult":{"id":3760}},"valueChanges":{
            "record":{"packCount":{"value":984},"roundCount":{"value":35}},
            "effect":{"effectList":{"value":[{"id":3760,"uid":242,"badge":{"id":0}}]}}}}));
        let saved = store.context.clone();
        let started = store.stats.current.as_ref().unwrap().started_at;
        store.observe(&packet(3,"fetchAmuletActivityData",json!({"data":{}})));
        assert_eq!(store.stats.current.as_ref().unwrap().pack_obtained,1,"Empty login response must not finish the run");
        start(&mut store,4,0);
        assert_eq!(store.stats.current.as_ref().unwrap().pack_obtained,0);
        let mut reloaded: MagicVineStore = serde_json::from_slice(&serde_json::to_vec(&store).unwrap()).unwrap();
        // Same counters with different inventory must not merge two games.
        let mut different = saved.clone(); different["effect"]["effectList"][0]["uid"] = json!(999);
        reloaded.observe(&packet(5,"fetchAmuletActivityData",json!({"game":different})));
        assert_ne!(reloaded.stats.current.as_ref().unwrap().started_at,started);
        reloaded.observe(&packet(6,"fetchAmuletActivityData",json!({"game":saved})));
        let current = reloaded.stats.current.as_ref().unwrap();
        assert_eq!((current.started_at,current.pack_obtained,current.packs_bought),(started,1,984));
        assert!(!current.finished);
        assert!(!reloaded.stats.history.iter().any(|game| game.started_at == started));
        operate(&mut reloaded,7,json!({"result":{"selectPackResult":{"id":3760}}}));
        assert_eq!(reloaded.stats.current.as_ref().unwrap().pack_obtained,2);
    }

    #[test]
    fn version_three_counters_survive_upgrade_when_packet_logs_are_unavailable() {
        let root = std::env::temp_dir().join(format!("lens-vine-cache-migration-{}",std::process::id()));
        let path = root.join("stats.json");
        fs::create_dir_all(&root).unwrap();
        let mut store = MagicVineStore::default();
        store.rules_version = 3;
        store.stats.current = Some(MagicVineGame {pack_obtained:118,..Default::default()});
        write_json(&path,&store).unwrap();
        let loaded = MagicVineStore::load(&path,&root.join("missing-logs"));
        assert_eq!(loaded.stats.current.unwrap().pack_obtained,118);
        fs::remove_file(path).unwrap(); fs::remove_dir(root).unwrap();
    }

    #[test]
    #[ignore = "opt-in local packet logs only; never connects to a game"]
    fn replay_resumed_local_logs() {
        let logs = std::env::var_os("SHANTEN_VINE_REPLAY_LOGS").expect("set local logs path");
        let root = std::env::temp_dir().join(format!("lens-vine-resume-{}",std::process::id()));
        let path = root.join("stats.json");
        let store = MagicVineStore::load(&path,Path::new(&logs));
        let game = store.snapshot().current.unwrap();
        println!("Offline replay: level={}, packs={}, obtained={}, triggers={}, farming_rounds={}",
            game.level,game.packs_bought,game.obtained(),game.score_triggers,game.farming_rounds);
        assert!(game.obtained() >= 118);
        assert!(!store.stats.history.iter().any(|old| old.started_at == game.started_at));
        fs::remove_file(path).unwrap(); fs::remove_dir(root).unwrap();
    }

    #[test]
    fn persistence_replay_and_missing_packet_gaps_are_explicit() {
        let root = std::env::temp_dir().join(format!("vine-stats-{}-{}",std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let logs = root.join("logs");
        fs::create_dir_all(&logs).unwrap();
        let path = root.join("data/stats.json");
        let packets = [packet(1,"amuletActivityStartGame",json!({"events":[{"type":1,"result":{"newGameResult":{
            "map":{"level":14016,"node":1},"record":{"packCount":0,"roundCount":1},"state":{"current":9}}}}]})),
            packet(2,"amuletActivityOperate",json!({"events":[{"type":53,"result":{"selectPackResult":{"id":3760}},"effectedHooks":[boost(1,&["10"])]}]}))];
        let log = logs.join("shanten-lens-packets-1.log");
        fs::write(&log,packets.iter().map(|packet| serde_json::to_string(packet).unwrap()+"\n").collect::<String>()).unwrap();
        let mut loaded = MagicVineStore::load(&path,&logs);
        assert_eq!(loaded.snapshot().current.unwrap().score_triggers,1);
        let reloaded = MagicVineStore::load(&path,&logs);
        assert_eq!(reloaded.snapshot().current.unwrap().score_triggers,1);
        let mut game = loaded.context.clone();
        game["record"]["packCount"] = json!(3);
        loaded.observe(&packet(3,"fetchAmuletActivityData",json!({"data":{"game":game}})));
        assert!(loaded.snapshot().current.unwrap().incomplete);
        loaded.observe(&packet(4,"amuletActivityGiveup",json!({})));
        assert!(loaded.snapshot().history.is_empty(),"one acquired vine is not enough for retained history");
        assert!(loaded.snapshot().current.is_none());
        fs::remove_file(log).unwrap();
        fs::remove_file(path).unwrap();
        fs::remove_dir(root.join("data")).unwrap();
        fs::remove_dir(logs).unwrap();
        fs::remove_dir(root).unwrap();
    }
}

impl Services {
    pub fn magic_vine_stats(&self) -> MagicVineStats { self.magic_vine.lock().unwrap().snapshot() }

    pub(super) fn import_magic_vine(&self) {
        let data = self.root.parent().unwrap_or(&self.root);
        let logs = data.join("logs");
        if !fs::read_dir(&logs).into_iter().flatten().flatten().any(|entry|
            entry.file_name().to_string_lossy().starts_with("shanten-lens-packets-")
                && entry.metadata().is_ok_and(|meta| meta.len() > 0)) { return; }
        self.magic_vine.lock().unwrap().stats.loading = true;
        let services = self.clone();
        std::thread::spawn(move || {
            let path = services.root.parent().unwrap_or(&services.root).join("data/magic-vine-stats.json");
            let mut imported = MagicVineStore::load(&path, &logs);
            let mut store = services.magic_vine.lock().unwrap();
            for packet in std::mem::take(&mut store.pending) { imported.observe(&packet); }
            imported.stats.loading = false;
            if let Err(error) = write_json(&path, &imported) { tracing::warn!(%error, "Could not save magic vine statistics"); }
            *store = imported;
            let _ = services.events.send(event("magic_vine_stats", json!(store.snapshot())));
        });
    }

    pub(super) fn record_magic_vine(&self, packet: &PacketLogItem) {
        let mut store = self.magic_vine.lock().unwrap();
        if store.stats.loading {
            if packet.method.contains("amuletActivity") || packet.method.ends_with("fetchAmuletActivityData") {
                store.pending.push(packet.clone());
            }
            return;
        }
        if !store.observe(packet) { return; }
        let path = self.root.parent().unwrap_or(&self.root).join("data/magic-vine-stats.json");
        if let Err(error) = write_json(&path, &*store) { tracing::warn!(%error, "Could not save magic vine statistics"); }
        let _ = self.events.send(event("magic_vine_stats", json!(store.snapshot())));
    }
}

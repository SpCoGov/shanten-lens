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
    #[serde(default)]
    pub support_amulet: Option<u64>,
    #[serde(default)]
    pub temporary_support: Option<u64>,
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
    #[serde(default)]
    pub highest_tile_scores: HashMap<String, String>,
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
    pending: Vec<(PacketLogItem, Option<u64>)>,
    #[serde(skip)]
    observed_support: Option<u64>,
    #[serde(default)]
    farming: farming::Farming,
    #[serde(default)]
    checkpoints: Vec<Checkpoint>,
    #[serde(default)]
    needs_resume_rebuild: bool,
    #[serde(default)]
    highest_scores_imported: bool,
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
    ("effect", &["effectList", "packCandidates"]), ("nodeEvent", &["advancedShopGoods"]),
    ("game", &["tileScoreMap"]),
];

fn snapshot_context(game: &Value) -> Value {
    let mut context = json!({"state":game["state"]});
    for (section, keys) in FIELDS {
        for key in *keys { context[*section][*key] = game[*section][*key].clone(); }
    }
    context
}

fn same_run(saved: &Value, incoming: &Value) -> bool {
    let old = items(&saved["effect"]["effectList"]);
    let new = items(&incoming["effect"]["effectList"]);
    if new.is_empty() { return false; }
    if ["map", "record"].iter().all(|key| saved[*key] == incoming[*key]) && old == new { return true; }
    // Progress may advance offline. Three independently stamped server instances identify the run,
    // while temporary stores, inventory order and newly acquired cards do not identify it.
    if ["packCount", "roundCount"].iter().any(|key|
        !saved["record"][*key].as_u64().zip(incoming["record"][*key].as_u64()).is_some_and(|(a,b)|b >= a))
        || !saved["map"]["level"].as_u64().zip(incoming["map"]["level"].as_u64()).is_some_and(|(a,b)|b >= a) {
        return false;
    }
    let anchors: BTreeSet<_> = old.iter().filter(|effect|
        ["uid", "random"].iter().all(|key| effect["badge"][*key].as_u64().is_some_and(|value|value > 0)))
        .filter(|effect| new.iter().any(|other| effect["uid"] == other["uid"] && effect["uid"].as_u64().is_some_and(|uid|uid > 0)
            && effect["id"] == other["id"] && ["id", "uid", "random"].iter().all(|key|effect["badge"][*key] == other["badge"][*key])))
        .filter_map(|effect|effect["uid"].as_u64()).collect();
    anchors.len() >= 3
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
        let index = self.checkpoints.iter().rposition(|saved| saved.game.packs_bought > 0
            && self.stats.current.as_ref().is_none_or(|current|current.started_at != saved.game.started_at)
            && same_run(&saved.context, &context));
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
        let mut repair = None;
        // Repair an already split run from its last checkpoint and replay the local log; do not
        // add the partial counters, which may include another account's intervening opening.
        if fs::read_dir(logs).into_iter().flatten().flatten().any(|entry|
            entry.file_name().to_string_lossy().starts_with("shanten-lens-packets-") && entry.metadata().is_ok_and(|meta|meta.len() > 0)) {
            if let Some(index) = store.checkpoints.iter().rposition(|saved| saved.game.packs_bought > 0
                && store.stats.current.as_ref().is_some_and(|current|current.started_at != saved.game.started_at)
                && same_run(&saved.context, &store.context)) {
                let saved = store.checkpoints.remove(index);
                repair = Some((saved.game.started_at,store.last_ts_ms));
                store.last_ts_ms = saved.game.updated_at.saturating_add(1);
                store.last_responses.clear();
                store.stats.history.retain(|old|old.started_at != saved.game.started_at);
                store.stats.current = Some(saved.game);
                store.context = saved.context;
                store.farming = saved.farming;
            }
        }
        let cached = if store.needs_resume_rebuild && fs::read_dir(logs).into_iter().flatten().flatten().any(|entry|
            entry.file_name().to_string_lossy().starts_with("shanten-lens-packets-") && entry.metadata().is_ok_and(|meta| meta.len() > 0)) {
            Some(std::mem::replace(&mut store, Self {rules_version:RULES_VERSION,..Default::default()}))
        } else { None };
        let mut highest_replay = (!store.highest_scores_imported).then(Self::default);
        let mut files: Vec<_> = fs::read_dir(logs).into_iter().flatten().flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("shanten-lens-packets-"))
            .filter(|entry| entry.metadata().and_then(|meta| meta.modified()).ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .is_some_and(|time| highest_replay.is_some() || time.as_millis() as u64 >= store.last_ts_ms))
            .map(|entry| entry.path()).collect();
        files.sort();
        let before = store.last_ts_ms;
        let mut resumed = false;
        for file in files {
            let Ok(file) = fs::File::open(file) else { continue };
            for line in BufReader::new(file).lines() {
                let Ok(line) = line else { break };
                if !line.contains("amuletActivity") && !line.contains("fetchAmuletActivityData") { continue; }
                if let Ok(packet) = serde_json::from_str::<PacketLogItem>(&line) {
                    if let Some(replay) = highest_replay.as_mut() { replay.observe(&packet); }
                    store.observe(&packet);
                    if packet.direction == Direction::Inbound && packet.packet_type == "Res" && packet.ts_ms >= before
                        && packet.method.ends_with("fetchAmuletActivityData")
                        && packet.data.pointer("/data/game").or_else(||packet.data.get("game"))
                            .is_some_and(|game|!items(&game["effect"]["effectList"]).is_empty())
                        && repair.is_some_and(|(started,_)|
                        store.stats.current.as_ref().is_some_and(|game|game.started_at == started)) {
                        resumed = true;
                    }
                }
            }
        }
        // Rotated or missing logs must not replace cached counters with a partial reconstruction.
        if repair.is_some_and(|(_,last)| !resumed || store.last_ts_ms < last) { return Self::read(path); }
        if store.stats.current.is_none() && store.stats.history.is_empty() {
            if let Some(cached) = cached { store = cached; }
        }
        let imported = highest_replay.is_some();
        if let Some(replay) = highest_replay {
            for (level, score) in replay.stats.highest_tile_scores { store.record_highest(&level, &score); }
            store.highest_scores_imported = !store.stats.highest_tile_scores.is_empty();
        }
        if store.last_ts_ms != before || imported {
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

    fn observe_with_support(&mut self, packet: &PacketLogItem, family: Option<u64>) -> bool {
        let before = self.stats.current.as_ref().map(|game|(game.support_amulet,game.temporary_support));
        self.observed_support = family;
        let changed = self.observe(packet);
        // Remember an intended temporary selection before its acknowledgement, including a lost reply.
        if packet.direction == Direction::Outbound && packet.packet_type == "Req" && packet.method.ends_with("amuletActivityOperate")
            && packet.data["type"] == 16 {
            if let Some(target) = family {
                let chosen = packet.data["args"][0].as_u64().and_then(|index|
                    items(&self.context["effect"]["packCandidates"]).get(index as usize))
                    .and_then(|effect|effect["id"].as_u64()).map(|id|id / 10);
                if let Some(chosen) = chosen.filter(|chosen|*chosen != target && crate::vine_farming::SUPPORTS.contains(chosen)) {
                    if let Some(game) = self.stats.current.as_mut() {
                        game.support_amulet = Some(target);
                        game.temporary_support = Some(chosen);
                    }
                }
            }
        }
        self.observed_support = None;
        changed || before != self.stats.current.as_ref().map(|game|(game.support_amulet,game.temporary_support))
    }

    // Persist only changed visible statistics or a new per-level base-score record.
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
                if self.stats.current.is_none() && game.pointer("/state/current").and_then(Value::as_u64) == Some(100) {
                    self.record_scores(game["map"]["level"].as_u64().unwrap_or(0), &game["game"]["tileScoreMap"]);
                    return self.stats.highest_tile_scores != previous.highest_tile_scores;
                }
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
        let changed = self.stats.current != previous.current || self.stats.history != previous.history
            || self.stats.highest_tile_scores != previous.highest_tile_scores;
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
        if let Some(family) = crate::vine_farming::support_family(items(&self.context["effect"]["effectList"])) {
            if let Some(target) = self.observed_support {
                game.support_amulet = Some(target);
                game.temporary_support = (family != target).then_some(family);
            } else if game.temporary_support != Some(family) {
                game.support_amulet = Some(family);
                game.temporary_support = None;
            }
        } else if let Some(target) = self.observed_support {
            game.support_amulet = Some(target);
        }
        let level = game.level;
        self.record_scores(level, &self.context["game"]["tileScoreMap"].clone());
    }

    fn record_scores(&mut self, level: u64, scores: &Value) {
        for score in items(scores).iter().filter_map(|entry| entry["score"].as_str()) {
            self.record_highest(&level.to_string(), score);
        }
    }

    fn record_highest(&mut self, level: &str, score: &str) {
        use std::cmp::Ordering;
        if level == "0" || crate::vine_farming::compare_points(score, "0").is_none_or(|order| order == Ordering::Less) { return; }
        if self.stats.highest_tile_scores.get(level).is_none_or(|best|
            crate::vine_farming::compare_points(score, best) == Some(Ordering::Greater)) {
            self.stats.highest_tile_scores.insert(level.to_owned(), score.to_owned());
        }
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
    fn support_target_survives_expansion_pause_and_reload_without_crossing_games() {
        for family in [356,351,160] {
            let root = std::env::temp_dir().join(format!("lens-support-{}-{family}",std::process::id()));
            let path = root.join("data/magic-vine-stats.json");
            let mut store = MagicVineStore {rules_version:RULES_VERSION,..Default::default()};
            start(&mut store,1,14015);
            let core: Vec<_> = [3771,2321,1691,1661,1501,621].into_iter()
                .map(|id|json!({"id":id,"uid":id,"volume":1,"badgeId":600070})).collect();
            let mut owned=core.clone();
            owned.push(json!({"id":family * 10 + 1,"uid":204,"volume":1,"badgeId":600070}));
            operate(&mut store,2,json!({"valueChanges":{"effect":{"effectList":{"dirty":true,"value":owned}}}}));
            assert_eq!(store.snapshot().current.unwrap().support_amulet,Some(family));
            owned=core.clone();owned.push(json!({"id":3760,"uid":9000,"volume":2,"badgeId":600160}));
            operate(&mut store,3,json!({"valueChanges":{"effect":{"effectList":{"dirty":true,"value":owned}}}}));
            write_json(&path,&store).unwrap();
            let (events,_)=tokio::sync::broadcast::channel(32);
            let services=Services::load(root.join("configs"),events).unwrap();
            let mut game=json!({"level":14015,"node":1,"effect_list":owned});
            assert_eq!(services.vine_support_amulet(&game),family,"Resume the same family while it is absent");
            game["effect_list"][0]["uid"]=json!(99999);
            assert_eq!(services.vine_support_amulet(&game),356,"Never apply a different inventory's saved family");
            drop(services);
            // A temporary pick must not change the original target, even if its response is lost.
            let temporary=if family == 356 {351} else {356};
            owned=core.clone();owned.push(json!({"id":3760,"uid":9000,"volume":1,"badgeId":0}));
            store.observe_with_support(&packet(4,"amuletActivityOperate",json!({"events":[{
                "valueChanges":{"effect":{"effectList":{"dirty":true,"value":owned},
                    "packCandidates":{"dirty":true,"value":[{"id":temporary * 10,"badgeId":0}]}}}
            }]})),Some(family));
            let mut request=packet(5,"amuletActivityOperate",json!({"type":16,"args":[0]}));
            request.direction=Direction::Outbound;request.packet_type="Req".into();
            assert!(store.observe_with_support(&request,Some(family)));
            assert_eq!(store.snapshot().current.unwrap().temporary_support,Some(temporary));
            write_json(&path,&store).unwrap();
            let mut reloaded=MagicVineStore::read(&path);
            owned.push(json!({"id":temporary * 10,"uid":9010,"volume":1,"badgeId":0}));
            reloaded.observe(&packet(6,"fetchAmuletActivityData",json!({"data":{"game":{
                "map":{"level":14015,"node":1},"state":{"current":9},"effect":{"effectList":owned}
            }}})));
            assert_eq!(reloaded.snapshot().current.unwrap().support_amulet,Some(family));
            write_json(&path,&reloaded).unwrap();
            let (events,_)=tokio::sync::broadcast::channel(32);
            let services=Services::load(root.join("configs"),events).unwrap();
            game=json!({"level":14015,"node":1,"effect_list":owned});
            assert_eq!(services.vine_support_amulet(&game),family,"A temporary must not become the handover target after restarting");
            owned.last_mut().unwrap()["id"]=json!(family * 10 + 1);
            operate(&mut reloaded,7,json!({"valueChanges":{"effect":{"effectList":{"dirty":true,"value":owned}}}}));
            assert_eq!(reloaded.snapshot().current.unwrap().temporary_support,None,"Returning to the original family clears the temporary marker");
            drop(services);fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn highest_base_scores_restore_old_logs_without_changing_cached_counters() {
        let root = std::env::temp_dir().join(format!("lens-highest-base-{}",std::process::id()));
        let logs = root.join("logs");
        fs::create_dir_all(&logs).unwrap();
        let path = root.join("stats.json");
        let mut cached = MagicVineStore::default();
        start(&mut cached,100,14037);
        cached.rules_version = RULES_VERSION;
        cached.stats.current.as_mut().unwrap().pack_obtained = 99;
        write_json(&path,&cached).unwrap();
        let ended = |ts,level,score| packet(ts,"fetchAmuletActivityData",json!({"data":{"game":{
            "state":{"current":100},"map":{"level":level},
            "game":{"tileScoreMap":[{"tile":"9m","score":score},{"tile":"1p","score":"bad"}]}
        }}}));
        let packets = [ended(1,14037,"1.1234567890123456789e200"),ended(2,14038,"2e200"),
            ended(3,14037,"1.1234567890123456788e200")];
        let log = logs.join("shanten-lens-packets-old.log");
        fs::write(&log,packets.iter().map(|p|serde_json::to_string(p).unwrap()+"\n").collect::<String>()).unwrap();
        let mut loaded = MagicVineStore::load(&path,&logs);
        assert_eq!(loaded.snapshot().current.unwrap().pack_obtained,99,"Backfill only adds score records");
        assert_eq!(loaded.snapshot().highest_tile_scores["14037"],"1.1234567890123456789e200");
        assert_eq!(loaded.snapshot().highest_tile_scores["14038"],"2e200");
        assert!(loaded.highest_scores_imported);
        assert!(!operate(&mut loaded,101,json!({"valueChanges":{"game":{"tileScoreMap":{
            "dirty":true,"value":[{"tile":"9m","score":"1e200"}]}}}})),"Lower scores must not replace the record");
        assert!(operate(&mut loaded,102,json!({"valueChanges":{"game":{"tileScoreMap":{
            "dirty":true,"value":[{"tile":"9m","score":"3e200"}]}}}})));
        write_json(&path,&loaded).unwrap();
        fs::remove_file(&log).unwrap();
        let restored = MagicVineStore::load(&path,&logs);
        assert_eq!(restored.snapshot().highest_tile_scores["14037"],"3e200","Records survive without old logs");
        assert_eq!(restored.snapshot().highest_tile_scores["14038"],"2e200");
        fs::remove_file(path).unwrap();
        fs::remove_dir(logs).unwrap();
        fs::remove_dir(root).unwrap();
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
    fn progressed_resume_ignores_transient_stores_but_requires_independent_server_identities() {
        let fixture: Value = serde_json::from_str(include_str!("../../tests/fixtures/vine-resume-progressed.json")).unwrap();
        let saved = &fixture["checkpoint"]["context"];
        let mut returning = snapshot_context(&fixture["returning_game"]);
        assert!(same_run(saved,&returning),"The captured resumed Ex39 has advanced counters and a different Hacker store");
        returning["effect"]["effectList"].as_array_mut().unwrap().reverse();
        returning["map"]["level"] = json!(14040);
        assert!(same_run(saved,&returning),"Inventory order and the next level do not start a new run");
        returning["record"]["roundCount"] = json!(57);
        assert!(!same_run(saved,&returning),"Older or unrelated counters must not rewind a run");
        returning = snapshot_context(&fixture["returning_game"]);
        for effect in returning["effect"]["effectList"].as_array_mut().unwrap() {
            if effect.get("badge").is_some() {effect["badge"]["random"] = json!(999999);}
        }
        assert!(!same_run(saved,&returning),"Matching card types and counters from another account are insufficient");
        returning["effect"]["effectList"] = json!([saved["effect"]["effectList"][0].clone()]);
        assert!(!same_run(saved,&returning),"One matching instance is not enough");
    }

    #[test]
    fn split_cached_run_replays_from_checkpoint_without_adding_another_games_vine() {
        let fixture: Value = serde_json::from_str(include_str!("../../tests/fixtures/vine-resume-progressed.json")).unwrap();
        let checkpoint: Checkpoint = serde_json::from_value(fixture["checkpoint"].clone()).unwrap();
        let original = checkpoint.game.clone();
        let base = original.updated_at;
        let root = std::env::temp_dir().join(format!("lens-progressed-resume-{}",std::process::id()));
        let logs = root.join("logs");fs::create_dir_all(&logs).unwrap();
        let path = root.join("stats.json");
        let make = |offset, method: &str, data| PacketLogItem {ts_ms:base+offset,direction:Direction::Inbound,
            packet_type:"Res".into(),method:format!(".lq.Lobby.{method}"),id:Some(offset as u32),data};
        let packets = [
            make(0,"amuletActivityOperate",json!({"events":[{"result":{"selectPackResult":{"id":3760}}}]})),
            make(10,"fetchAmuletActivityData",json!({"game":{"state":{"current":1},"map":{"level":0,"node":0},
                "record":{"packCount":0,"roundCount":0},"effect":{"effectList":[]}}})),
            make(20,"amuletActivityStartGame",json!({"events":[{"type":1,"result":{"newGameResult":{
                "state":{"current":9},"map":{"level":13001,"node":4},"record":{"packCount":0,"roundCount":0},
                "effect":{"effectList":[{"id":620,"uid":1}]}}}}]})),
            make(30,"amuletActivityOperate",json!({"events":[{"result":{"selectPackResult":{"id":3760}},
                "valueChanges":{"record":{"packCount":{"value":1}}}}]})),
            make(40,"fetchAmuletActivityData",json!({"game":fixture["returning_game"]})),
            make(50,"amuletActivityOperate",json!({"events":[{"result":{"selectPackResult":{"id":3760}},
                "effectedHooks":[boost(4310,&["1"])],"valueChanges":{"record":{"packCount":{"value":1556}}}}]})),
        ];
        let mut wrong = original.clone();wrong.started_at=base+20;wrong.pack_obtained=2;wrong.shop_obtained=0;
        wrong.other_obtained=0;wrong.regeneration_obtained=0;wrong.score_triggers=1;wrong.farming_rounds=0;
        let mut ended = original.clone();ended.finished=true;
        let cache = MagicVineStore {rules_version:RULES_VERSION,last_ts_ms:base+50,highest_scores_imported:true,
            stats:MagicVineStats {current:Some(wrong),history:vec![ended],highest_tile_scores:HashMap::from([("14039".into(),"1e80".into())]),..Default::default()},
            context:snapshot_context(&fixture["returning_game"]),checkpoints:vec![checkpoint],..Default::default()};
        write_json(&path,&cache).unwrap();
        let file=logs.join("shanten-lens-packets-resume.log");
        fs::write(&file,packets.iter().map(|p|serde_json::to_string(p).unwrap()+"\n").collect::<String>()).unwrap();
        let repaired=MagicVineStore::load(&path,&logs);let current=repaired.stats.current.as_ref().unwrap();
        assert_eq!(current.started_at,original.started_at);
        assert_eq!(current.obtained(),original.obtained()+1,"Keep the resumed vine, exclude the other opening and the checkpoint's already-counted packet");
        assert_eq!(current.score_triggers,original.score_triggers+1);
        assert_eq!(current.farming_rounds,original.farming_rounds);
        assert_eq!(current.farming_obtained,original.farming_obtained);
        assert_eq!(current.packs_bought,1556);
        assert!(current.incomplete,"Offline changes cannot be invented from snapshot counters");
        assert!(!repaired.stats.history.iter().any(|g|g.started_at==original.started_at));
        assert_eq!(repaired.stats.highest_tile_scores["14039"],"1e80");
        let again=MagicVineStore::load(&path,&logs);
        assert_eq!(again.stats.current,repaired.stats.current,"Repeated restart must not double count");
        write_json(&path,&cache).unwrap();
        fs::write(&file,serde_json::to_string(&packets[0]).unwrap()+"\n").unwrap();
        let unavailable=MagicVineStore::load(&path,&logs);
        assert_eq!(unavailable.stats.current,cache.stats.current,"Missing resume packets must preserve the cache");
        fs::write(&file,packets[..5].iter().map(|p|serde_json::to_string(p).unwrap()+"\n").collect::<String>()).unwrap();
        let truncated=MagicVineStore::load(&path,&logs);
        assert_eq!(truncated.stats.current,cache.stats.current,"A truncated replay must not discard newer cached acquisitions");
        fs::remove_dir_all(root).unwrap();
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
        if let Some(cache) = std::env::var_os("SHANTEN_VINE_REPLAY_CACHE") {
            fs::create_dir_all(&root).unwrap();
            fs::copy(cache,&path).unwrap();
        }
        let store = MagicVineStore::load(&path,Path::new(&logs));
        let game = store.snapshot().current.unwrap();
        println!("Offline replay: started_at={}, level={}, packs={}, obtained={}, triggers={}, farming_rounds={}",
            game.started_at,game.level,game.packs_bought,game.obtained(),game.score_triggers,game.farming_rounds);
        assert!(std::iter::once(&game).chain(store.stats.history.iter()).any(|game|game.obtained() >= 118));
        assert!(!store.stats.history.iter().any(|old| old.started_at == game.started_at));
        for old in store.stats.history.iter().filter(|game|game.started_at >= 1794350743479) {
            println!("Rebuilt history: started_at={}, level={}, obtained={}, triggers={}, farming_rounds={}",
                old.started_at,old.level,old.obtained(),old.score_triggers,old.farming_rounds);
        }
        if let Some(output) = std::env::var_os("SHANTEN_VINE_REPLAY_OUTPUT") { write_json(Path::new(&output),&store).unwrap(); }
        let reloaded = MagicVineStore::load(&path,Path::new(&logs));
        assert_eq!(reloaded.stats.current,store.stats.current,"Replaying the same packets twice is idempotent");
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
    pub(crate) fn vine_support_amulet(&self, game: &Value) -> u64 {
        let owned = items(&game["effect_list"]);
        let store = self.magic_vine.lock().unwrap();
        // Persist the handed-over family through expansion, pause and reconnect; never use another game's inventory.
        let remembered = store.stats.current.as_ref().filter(|current| !owned.is_empty()
            && game["level"].as_u64() == Some(current.level) && game["node"].as_u64() == Some(current.node)
            && owned == items(&store.context["effect"]["effectList"]));
        let held = crate::vine_farming::support_family(owned);
        let remembered = remembered.filter(|current|held.is_none() || held == current.temporary_support)
            .and_then(|current| current.support_amulet).filter(|family| crate::vine_farming::SUPPORTS.contains(family));
        remembered.or(held).unwrap_or(356)
    }

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
            for (packet,family) in std::mem::take(&mut store.pending) { imported.observe_with_support(&packet,family); }
            imported.stats.loading = false;
            if let Err(error) = write_json(&path, &imported) { tracing::warn!(%error, "Could not save magic vine statistics"); }
            *store = imported;
            let _ = services.events.send(event("magic_vine_stats", json!(store.snapshot())));
        });
    }

    pub(super) fn record_magic_vine(&self, packet: &PacketLogItem) {
        let family = self.vine_farming_profile().and_then(|profile|profile["support_amulet"].as_u64());
        let mut store = self.magic_vine.lock().unwrap();
        if store.stats.loading {
            if packet.method.contains("amuletActivity") || packet.method.ends_with("fetchAmuletActivityData") {
                store.pending.push((packet.clone(),family));
            }
            return;
        }
        if !store.observe_with_support(packet,family) { return; }
        let path = self.root.parent().unwrap_or(&self.root).join("data/magic-vine-stats.json");
        if let Err(error) = write_json(&path, &*store) { tracing::warn!(%error, "Could not save magic vine statistics"); }
        let _ = self.events.send(event("magic_vine_stats", json!(store.snapshot())));
    }
}

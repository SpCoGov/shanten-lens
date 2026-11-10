use super::{items, MagicVineGame};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use std::{collections::{BTreeSet, HashMap}, sync::LazyLock};

const TARGETS: &[u64] = &[377, 232, 169, 166, 150, 62, 356, 376];

// History replay visits this lookup for every event; index the shared catalog once.
static LEVEL_GATES: LazyLock<HashMap<u64, (u64, Option<u64>)>> = LazyLock::new(|| {
    crate::action_provider::LEVELS.as_array().unwrap().iter().map(|entry| {
        let label=entry["label"].as_str().unwrap_or("");
        let ex=label.strip_prefix("Ex").and_then(|n| n.parse::<u64>().ok()).unwrap_or(0);
        let chapter=label.parse::<u64>().unwrap_or(0);
        let first=if ex>0 || chapter>3 { Some(0) } else if chapter==3 {
            entry["nodes"].as_array().unwrap().iter().find(|stage| stage["label"]=="3-1")
                .and_then(|stage| stage["node"].as_u64())
        } else { None };
        (entry["level"].as_u64().unwrap(),(ex,first))
    }).collect()
});

pub(super) fn ex_number(level: u64) -> u64 { LEVEL_GATES.get(&level).map_or(0, |gate| gate.0) }

#[derive(Clone, Debug, Default, Deserialize, Serialize, Type, PartialEq)]
pub struct FarmingPeriod {
    pub start_level: u64,
    pub start_node: u64,
    pub end_level: u64,
    pub end_node: u64,
    pub rounds: u64,
    pub obtained: u64,
    pub score_triggers: u64,
}

#[derive(Clone, Default, Deserialize, Serialize)]
struct Cycle {
    level: u64,
    node: u64,
    end_level: u64,
    end_node: u64,
    ready: bool,
    broken: bool,
    obtained: u64,
    triggers: u64,
}

#[derive(Clone, Default, Deserialize, Serialize)]
pub(super) struct Farming {
    unlocked: bool,
    active: bool,
    cycle: Option<Cycle>,
    observing: Vec<Cycle>,
    stopping: Vec<Cycle>,
}

fn target_count(context: &Value) -> usize {
    items(&context["effect"]["effectList"]).iter()
        .filter(|effect| effect["badge"]["id"] != 600210)
        .filter_map(|effect| effect["id"].as_u64().map(|id| id / 10))
        .filter(|id| TARGETS.contains(id)).collect::<BTreeSet<_>>().len()
}

fn after_three_one(level: u64, node: u64) -> bool {
    LEVEL_GATES.get(&level).and_then(|gate| gate.1).is_some_and(|first| node >= first)
}

impl Farming {
    pub(super) fn observe(&mut self, game: &mut MagicVineGame, context: &Value, event_type: u64, obtained: u64, triggers: u64) {
        self.unlocked |= after_three_one(game.level, game.node);
        if !self.unlocked { return; }
        let count = target_count(context);
        // A procurement cycle ends at the following battle settlement; 3-1 is itself a shop node.
        let cycle = self.cycle.get_or_insert_with(|| Cycle {level: game.level, node: game.node, ..Default::default()});
        cycle.ready |= count >= 5;
        cycle.broken = count <= 4;
        cycle.obtained += obtained;
        cycle.triggers += triggers;
        if event_type == 56 {
            cycle.end_level = game.level;
            cycle.end_node = game.node;
            let cycle = self.cycle.take().unwrap();
            self.complete(game, cycle);
        }
    }

    fn add(game: &mut MagicVineGame, cycle: &Cycle) {
        let period = game.farming_periods.last_mut().unwrap();
        period.rounds += 1;
        period.obtained += cycle.obtained;
        period.score_triggers += cycle.triggers;
        period.end_level = cycle.end_level;
        period.end_node = cycle.end_node;
        game.farming_rounds += 1;
        game.farming_obtained += cycle.obtained;
        game.farming_score_triggers += cycle.triggers;
    }

    fn complete(&mut self, game: &mut MagicVineGame, cycle: Cycle) {
        if self.active {
            if cycle.broken && cycle.obtained == 0 {
                self.stopping.push(cycle);
                game.farming_status = "stopping".into();
                if self.stopping.len() == 3 {
                    self.stopping.clear();
                    self.active = false;
                    game.farming_status = "ended".into();
                }
            } else {
                for pending in self.stopping.drain(..) { Self::add(game, &pending); }
                Self::add(game, &cycle);
                game.farming_status = "active".into();
            }
            return;
        }
        if !cycle.ready || cycle.broken {
            self.observing.clear();
            return;
        }
        self.observing.push(cycle);
        if self.observing.len() > 3 { self.observing.remove(0); }
        game.farming_status = "observing".into();
        if self.observing.len() == 3 && self.observing.iter().filter(|cycle| cycle.obtained > 0).count() >= 2 {
            let first = &self.observing[0];
            game.farming_periods.push(FarmingPeriod {start_level: first.level, start_node: first.node, ..Default::default()});
            for cycle in self.observing.drain(..) { Self::add(game, &cycle); }
            self.active = true;
            game.farming_status = "active".into();
        }
    }

    pub(super) fn finish(&mut self, game: &mut MagicVineGame, interrupted: bool) {
        // An unfinished battle is excluded; confirmed farming cycles awaiting exit confirmation remain valid.
        if self.active {
            for pending in self.stopping.drain(..) { Self::add(game, &pending); }
        }
        game.farming_status = if interrupted { "interrupted" } else if game.farming_rounds > 0 { "ended" } else { "unformed" }.into();
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn context(count: usize) -> Value {
        json!({"effect":{"effectList":TARGETS.iter().take(count).map(|id| json!({"id":id*10})).collect::<Vec<_>>()}})
    }
    fn settle(farming: &mut Farming, game: &mut MagicVineGame, count: usize, obtained: u64) {
        farming.observe(game, &context(count), 56, obtained, obtained);
    }

    #[test]
    fn five_kinds_and_two_productive_cycles_confirm_all_three_including_zero() {
        let mut phase = Farming::default();
        let mut game = MagicVineGame {level:13002,node:8,..Default::default()};
        settle(&mut phase,&mut game,8,4);
        assert_eq!(game.farming_rounds,0);
        game.level=13003; game.node=2;
        for obtained in [0,2,3] { settle(&mut phase,&mut game,5,obtained); }
        assert_eq!((game.farming_rounds,game.farming_obtained),(3,5));
        assert_eq!(game.farming_periods[0].start_node,2);
        for _ in 0..3 { settle(&mut phase,&mut game,5,0); }
        assert_eq!(game.farming_rounds,6,"unlucky zero rounds stay in the average while the lineup remains");
        for _ in 0..3 { settle(&mut phase,&mut game,4,0); }
        assert_eq!(game.farming_rounds,6,"three dismantled zero cycles confirm exit and are rolled back");
        assert_eq!(game.farming_status,"ended");
        for _ in 0..3 { settle(&mut phase,&mut game,5,1); }
        assert_eq!(game.farming_periods.len(),2);
        assert_eq!((game.farming_rounds,game.farming_obtained),(9,8));
    }

    #[test]
    fn recovery_and_gains_cancel_exit_and_end_of_game_keeps_unconfirmed_zero_cycles() {
        let mut phase = Farming::default();
        let mut game = MagicVineGame {level:14006,node:2,..Default::default()};
        for _ in 0..3 { settle(&mut phase,&mut game,5,1); }
        settle(&mut phase,&mut game,4,0);
        settle(&mut phase,&mut game,4,1);
        assert_eq!((game.farming_rounds,game.farming_obtained),(5,4));
        settle(&mut phase,&mut game,4,0);
        settle(&mut phase,&mut game,5,0);
        assert_eq!(game.farming_rounds,7);
        settle(&mut phase,&mut game,4,0);
        phase.observe(&mut game,&context(5),4,9,9);
        phase.finish(&mut game,false);
        assert_eq!((game.farming_rounds,game.farming_obtained),(8,4),"an uncompleted cycle is not a full round");
    }

    #[test]
    fn distinct_permanent_kinds_only_and_gaps_do_not_bridge_confirmation() {
        assert_eq!(target_count(&json!({"effect":{"effectList":[{"id":3770},{"id":3771},{"id":2320,"badge":{"id":600210}}]}})),1);
        let mut phase=Farming::default();
        let mut game=MagicVineGame {level:13003,node:2,..Default::default()};
        for _ in 0..2 { settle(&mut phase,&mut game,5,1); }
        phase.finish(&mut game,true);
        settle(&mut phase,&mut game,5,1);
        assert_eq!(game.farming_rounds,0);
    }
}

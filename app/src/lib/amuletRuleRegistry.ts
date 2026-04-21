import type {AmuletRuleHandler} from "./scoreEngine";

const amuletRuleRegistry: Record<number, AmuletRuleHandler> = {};

export function registerAmuletRule(regIdOrIds: number | number[], handler: AmuletRuleHandler) {
    const ids = Array.isArray(regIdOrIds) ? regIdOrIds : [regIdOrIds];
    ids.forEach((id) => {
        amuletRuleRegistry[id] = handler;
    });
}

export function unregisterAmuletRule(regId: number) {
    delete amuletRuleRegistry[regId];
}

export function getRegisteredAmuletRule(regId: number) {
    return amuletRuleRegistry[regId];
}

export function getRegisteredAmuletRuleExact(regId: number) {
    return amuletRuleRegistry[regId];
}

export function getAllRegisteredAmuletRules() {
    return amuletRuleRegistry;
}

// 嵐星の影分身
registerAmuletRule(700, {
    affectsPoint: true,
    getMaxEffectApplications: () => 1,
    getDefaultConfig: () => ({
        activeOnWin: true,
        effectTarget: "none",
        effectFormula: "",
    }),
    applyEffect: ({index, rules, applyCopiedActivation}) => {
        if (rules[index + 1]) {
            applyCopiedActivation(index + 1);
        }
        return {
            transmissionTriggers: 1,
        };
    },
});

// 嵐星の影分身+
registerAmuletRule(701, {
    affectsPoint: true,
    getMaxEffectApplications: () => 1,
    getDefaultConfig: () => ({
        activeOnWin: true,
        effectTarget: "none",
        effectFormula: "",
    }),
    applyEffect: ({index, rules, state, applyCopiedActivation}) => {
        if (rules[index + 1]) {
            // 复制后方的护身符效果
            applyCopiedActivation(index + 1);
        }
        return {
            fan: state.fan + 1000n,
            transmissionTriggers: 2,
        };
    },
    growData: ({currentData}) => currentData,
});

// 駆けるタイヤ
registerAmuletRule([1460, 1461], {
    getDefaultConfig: () => ({
        activeOnWin: true,
        effectTarget: "none",
        effectFormula: "",
    }),
    applyEffect: ({state}) => {
        return {fan: state.fan};
    },
    growData: ({currentData}) => currentData,
    // 只有手牌有饼子的时候才可以触发
    isActiveOnWin: ({runtime}) => !!runtime.hasPinzuInHand,
});

registerAmuletRule(650, {
    affectsPoint: true,
    getDefaultConfig: () => ({
        activeOnWin: true,
        effectTarget: "none",
        effectFormula: "",
        growthFormula: "data",
    }),
    applyEffect: ({state}) => {
        return {fan: state.fan + 600n};
    },
    growData: ({currentData}) => currentData,
});

registerAmuletRule(651, {
    affectsPoint: true,
    getDefaultConfig: () => ({
        activeOnWin: true,
        effectTarget: "none",
        effectFormula: "",
        growthFormula: "data",
    }),
    applyEffect: ({state}) => {
        return {fan: state.fan + 900n};
    },
    growData: ({currentData}) => currentData,
});

registerAmuletRule(1270, {
    affectsPoint: true,
    getDefaultConfig: () => ({
        activeOnWin: true,
        effectTarget: "none",
        effectFormula: "",
        growthFormula: "data",
    }),
    applyEffect: ({state}) => {
        return {fan: state.fan * 1500n};
    },
    growData: ({currentData}) => currentData,
});

registerAmuletRule(1271, {
    affectsPoint: true,
    getDefaultConfig: () => ({
        activeOnWin: true,
        effectTarget: "none",
        effectFormula: "",
        growthFormula: "data",
    }),
    applyEffect: ({state}) => {
        return {fan: state.fan * 3000n};
    },
    growData: ({currentData}) => currentData,
});

// 能进链但是不会改变番数
registerAmuletRule([2050, 2051, 150, 151, 1600, 1601, 1650, 1651], {
    getDefaultConfig: () => ({
        activeOnWin: true,
        effectTarget: "none",
        effectFormula: "",
        growthFormula: "data",
    })
});

// 不能进链也不会修改分数
registerAmuletRule([1640, 1641, 1660, 1661, 1690, 1691, 2000, 2001, 2240, 2241, 2220, 2221, 1500, 1501, 1630, 1631, 2300, 2301, 2310, 2311, 1590, 1591, 2210, 2211, 2330, 2331, 2250, 2251, 2230, 2231, 2100, 2101, 2190, 2191, 2170, 2171, 2200, 2201, 2180, 2181, 2160, 2161, 50, 51, 20, 21, 10, 11, 90, 91, 30, 31, 120, 121,], {
    getDefaultConfig: () => ({
        activeOnWin: false,
        effectTarget: "none",
        effectFormula: "",
        growthFormula: "data",
    })
});

registerAmuletRule([2270, 2271], {
    affectsPoint: true,
    getDefaultConfig: () => ({
        dataRaw: "100",
        activeOnWin: true,
        effectTarget: "none",
        effectFormula: "",
        growthFormula: "data",
    }),
    applyEffect: ({rule, state}) => {
        const data = BigInt(rule.dataRaw || "100");
        return {
            fan: (data * state.fan) / 100n,
        };
    },
    growData: ({rule, currentData}) => {
        const growthRate = rule.regId === 2271 ? 150n : 130n;
        return (currentData * growthRate) / 100n;
    },
    isActiveOnWin: () => true,
});

registerAmuletRule([2290, 2291], {
    affectsPoint: true,
    getDefaultConfig: () => ({
        dataRaw: "100",
        activeOnWin: true,
        growthAfterRound: true,
        effectTarget: "none",
        effectFormula: "",
        growthFormula: "data",
    }),
    applyEffect: ({vars, state}) => {
        return {
            fan: (vars.dataValues[0] ?? 100n) * state.fan / 100n,
        };
    },
    getGrowthRepeat: ({rule, index, rules}) => {
        if (!rule.hasExtensionSeal) return 1;
        const previousRule = index > 0 ? rules[index - 1] : null;
        return previousRule && (previousRule.regId === 2300 || previousRule.regId === 2301) ? 4 : 2;
    },
    growData: ({rule, currentData}) => {
        const growthRate = rule.regId === 2291 ? 130n : 120n;
        return (currentData * growthRate) / 100n;
    },
    isActiveOnWin: () => true,
});

registerAmuletRule([1580, 1581], {
    affectsPoint: true,
    getDefaultConfig: () => ({
        activeOnWin: true,
        growthAfterRound: true,
        effectTarget: "none",
        effectFormula: "",
        growthFormula: "data",
    }),
    getPreWinActivationCount: () => 1,
    applyEffect: ({rule, state, activationIndex, runtime}) => {
        const data0 = (() => {
            try {
                return BigInt(rule.dataRawList[0] ?? "100");
            } catch {
                return 100n;
            }
        })();
        const data1 = (() => {
            try {
                return BigInt(rule.dataRawList[1] ?? "0");
            } catch {
                return 0n;
            }
        })();
        let fan = state.fan;
        if (activationIndex === 1) {
            fan += BigInt(runtime.soulTileCount ?? 0) * (data1 + 1n) * 100n;
        }
        fan = (fan * data0) / 100n;
        return {fan};
    },
    applyTriggerGrowth: ({rule}) => {
        const data0 = (() => {
            try {
                return BigInt(rule.dataRawList[0] ?? "0");
            } catch {
                return 0n;
            }
        })();
        const data1 = (() => {
            try {
                return BigInt(rule.dataRawList[1] ?? "0");
            } catch {
                return 0n;
            }
        })();
        const data1Growth = rule.regId === 1581 ? 2n : 1n;
        return [
            (data0 + 100n).toString(),
            (data1 + data1Growth).toString(),
            ...rule.dataRawList.slice(2),
        ];
    },
    growData: ({currentData}) => currentData,
    isActiveOnWin: () => true,
});

registerAmuletRule([1560, 1561], {
    affectsPoint: true,
    getDefaultConfig: () => ({
        dataRaw: "100",
        activeOnWin: true,
        effectTarget: "none",
        effectFormula: "",
        growthFormula: "data",
    }),
    applyEffect: ({vars, state}) => {
        return {
            fan: ((vars.dataValues[0] ?? 100n) * state.fan) / 100n,
        };
    },
    applyTriggerGrowth: ({rule}) => {
        const data0 = (() => {
            try {
                return BigInt(rule.dataRawList[0] ?? "100");
            } catch {
                return 100n;
            }
        })();
        const growth = rule.regId === 1561 ? 300n : 200n;
        return [
            (data0 + growth).toString(),
            ...rule.dataRawList.slice(1),
        ];
    },
    growData: ({currentData}) => currentData,
    isActiveOnWin: () => true,
});

registerAmuletRule([1610, 1611], {
    affectsPoint: true,
    getDefaultConfig: () => ({
        dataRaw: "100",
        activeOnWin: true,
        effectTarget: "none",
        effectFormula: "",
        growthFormula: "data",
    }),
    applyEffect: ({vars, state}) => {
        return {
            fan: ((vars.dataValues[0] ?? 100n) * state.fan) / 100n,
        };
    },
    applyTriggerGrowth: ({rule}) => {
        const data0 = (() => {
            try {
                return BigInt(rule.dataRawList[0] ?? "100");
            } catch {
                return 100n;
            }
        })();
        const growthRate = rule.regId === 1611 ? 130n : 120n;
        return [
            ((data0 * growthRate) / 100n).toString(),
            ...rule.dataRawList.slice(1),
        ];
    },
    growData: ({currentData}) => currentData,
    isActiveOnWin: () => true,
});

registerAmuletRule([110, 111], {
    affectsPoint: true,
    getDefaultConfig: () => ({
        activeOnWin: true,
        effectTarget: "none",
        effectFormula: "",
        growthFormula: "data",
    }),
    applyEffect: ({state, vars, rule}) => {
        const rate = rule.regId === 110 ? 8n : 12n;
        const data = vars.dataValues[0] * 100n * rate;
        return {score: state.score + data};
    },
    growData: ({currentData}) => currentData,
});

registerAmuletRule([1570, 1571], {
    affectsPoint: true,
    getDefaultConfig: () => ({
        dataRaw: "100",
        activeOnWin: true,
        effectTarget: "none",
        effectFormula: "",
        growthFormula: "data",
    }),
    applyEffect: ({rule, state}) => {
        const data = BigInt(rule.dataRaw || "100");
        return {
            fan: (data * state.fan) / 100n,
        };
    },
    growData: ({rule, currentData}) => {
        const growthRate = rule.regId === 1571 ? 150n : 130n;
        return (currentData * growthRate) / 100n;
    },
    isActiveOnWin: () => true,
});

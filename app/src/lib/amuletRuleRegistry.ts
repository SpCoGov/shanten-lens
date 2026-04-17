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

// 能进链但是不会改变番数
registerAmuletRule([2050, 2051, 150, 151, 1600, 1601], {
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

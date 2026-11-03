import assert from "node:assert/strict";
import {formatScientificScaledNumber} from "./bigNumber";

const gamePoint = 3976400n * 10n ** 35n;
assert.equal(formatScientificScaledNumber(gamePoint, 2, 6), "3.976400e39");
assert.equal(formatScientificScaledNumber(gamePoint, 2), "3.98e39");
assert.equal(formatScientificScaledNumber(0n, 2, 6), "0.000000e0");
assert.equal(formatScientificScaledNumber(0n, 2), "0.00e0");
assert.equal(formatScientificScaledNumber(1n, 2), "1.00e-2");
assert.equal(formatScientificScaledNumber(99n, 2), "9.90e-1");
assert.equal(formatScientificScaledNumber(100n, 2), "1.00e0");
assert.equal(formatScientificScaledNumber(12345n, 2), "1.23e2");
assert.equal(formatScientificScaledNumber(-12345n, 2), "-1.23e2");
assert.equal(formatScientificScaledNumber(9999n, 2), "1.00e2");
assert.equal(formatScientificScaledNumber(-9999n, 2), "-1.00e2");
assert.equal(formatScientificScaledNumber(999999999999999999999n, 2, 6), "1.000000e19");
assert.equal(formatScientificScaledNumber(123456749999999999999n, 0, 6), "1.234567e20");
assert.equal(formatScientificScaledNumber(10n ** 19607n, 2), "1.00e19605");

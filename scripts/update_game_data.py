#!/usr/bin/env python3
"""Generate liqi and packet-source registries from the Mahjong Soul Unity client."""
from __future__ import annotations

import argparse
import json
import os
import re
from dataclasses import dataclass
from pathlib import Path

from majsoul_client import CLIENT_URL, build_outputs, fetch_from_unity, wanted_excel

ROOT = Path(__file__).resolve().parents[1]
EXCEL = "LuaByte/Lua/Excels/"
AMULET_RARITIES = {1: "PURPLE", 2: "ORANGE", 3: "BLUE", 4: "GREEN", 5: "GRAY"}
BADGE_RARITIES = {1: "BROWN", 2: "BLUE", 3: "RED"}
LANGUAGES = ("chs", "chs_t", "jp", "en", "kr")
REQUIRED_RPC = {
    ".lq.Lobby.fetchAmuletActivityData", ".lq.Lobby.amuletActivityOperate",
    ".lq.Lobby.amuletActivityGameOperate", ".lq.Lobby.amuletActivityGiveup",
    ".lq.Lobby.fetchGameRecord",
}


@dataclass(frozen=True)
class DefaultRange:
    start: int
    end: int


class LuaValues:
    """Parse Excel literals and Pack/Sub/Dmap data without executing Lua."""
    token = re.compile(r'''"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?|[A-Za-z_]\w*|[{}\[\](),;=]''')

    def __init__(self, source: str, position: int = 0):
        self.source, self.position = source, position

    def peek(self) -> str:
        while match := re.match(r"\s+|--[^\n]*", self.source[self.position:]):
            self.position += match.end()
        match = self.token.match(self.source, self.position)
        if not match:
            raise ValueError(f"unsupported Lua literal at {self.position}")
        return match.group()

    def take(self, expected: str | None = None) -> str:
        value = self.peek()
        if expected is not None and value != expected:
            raise ValueError(f"expected {expected}, got {value}")
        self.position += len(value)
        return value

    def value(self):
        token = self.take()
        if token == "{":
            array, mapping = [], {}
            while self.peek() != "}":
                if self.peek() == "[":
                    self.take("[")
                    key = self.value()
                    self.take("]")
                    self.take("=")
                    if key in mapping:
                        raise ValueError("duplicate Lua table key")
                    mapping[key] = self.value()
                elif re.match(r"[A-Za-z_]", self.peek()) and re.match(
                    r"\w+\s*=", self.source[self.position:]
                ):
                    key = self.take()
                    self.take("=")
                    if key in mapping:
                        raise ValueError("duplicate Lua table key")
                    mapping[key] = self.value()
                else:
                    array.append(self.value())
                if self.peek() in (",", ";"):
                    self.take()
                elif self.peek() != "}":
                    raise ValueError("missing Lua table separator")
            self.take("}")
            if mapping and array:
                raise ValueError("mixed Lua tables are unsupported")
            return mapping if mapping else array
        if token[0] in "\"'":
            output = bytearray()
            text, index = token[1:-1], 0
            escapes = {"n": 10, "r": 13, "t": 9, "a": 7, "b": 8, "f": 12, "v": 11,
                       "\\": 92, '"': 34, "'": 39, "\n": 10}
            while index < len(text):
                if text[index] != "\\":
                    output.extend(text[index].encode("utf-8"))
                    index += 1
                    continue
                index += 1
                if decimal := re.match(r"\d{1,3}", text[index:]):
                    output.append(int(decimal.group()))
                    index += decimal.end()
                elif text[index] in escapes:
                    output.append(escapes[text[index]])
                    index += 1
                else:
                    raise ValueError("unsupported Lua string escape")
            return output.decode("utf-8")
        if token == "nil":
            return None
        if token in ("true", "false"):
            return token == "true"
        if re.fullmatch(r"-?\d+", token):
            return int(token)
        if re.match(r"-?\d", token):
            return float(token)
        if self.peek() == "[":
            self.take("[")
            value = self.value()
            self.take("]")
            if type(value) is not int or not 0 < value:
                raise ValueError("invalid Dmap index")
            start, end = divmod(value, 1000)
            if not 0 < start <= end:
                raise ValueError("invalid Dmap range")
            return DefaultRange(start, end)
        if self.peek() == "(":
            self.take("(")
            first = self.value()
            while self.peek() == ",":
                self.take(",")
                self.value()
            self.take(")")
            if not isinstance(first, list):
                raise ValueError("expected Pack/Sub array")
            return first
        return "@" + token


def local_tables(source: str) -> dict:
    return {match[1]: LuaValues(source, match.end() - 1).value()
            for match in re.finditer(r"\blocal\s+(\w+)\s*=\s*\{", source)}


def unpack_rows(source: str, fields: dict, defaults: list) -> list[dict]:
    positions = {name: abs(index) % 1000 for name, index in fields.items()}
    if any(type(index) is not int or index == 0 for index in fields.values()) or any(index == 0 for index in positions.values()):
        raise ValueError("invalid Excel field positions")
    width = max(positions.values())
    if len(defaults) != width:
        raise ValueError("Excel default count does not match fields")
    rows = []
    for match in re.finditer(r"\b\w+\[\d+\]\s*=\s*\w+\s*\(", source):
        values = LuaValues(source, match.end()).value()
        if not isinstance(values, list):
            raise ValueError("invalid packed Excel row")
        result, position, skipped = list(defaults), 1, set()
        for value in values:
            if isinstance(value, DefaultRange):
                if value.end > width:
                    raise ValueError("Dmap extends beyond the Excel row")
                skipped.update(range(value.start, value.end + 1))
                continue
            while position in skipped:
                position += 1
            if position > width:
                raise ValueError("packed Excel row contains extra values")
            if value is not False:
                result[position - 1] = value
            position += 1
        row = {name: result[index - 1] for name, index in positions.items()}
        if row.get("id") != int(re.search(r"\[(\d+)\]", match.group())[1]):
            raise ValueError("packed row ID differs from its key")
        rows.append(row)
    return rows


def load_sheet(sources: dict[str, str], sheet: str) -> list[dict]:
    path = EXCEL + "Data/" + sheet
    source = sources[path + ".lua.bytes"]
    tables = local_tables(source)
    schema = re.search(r'return\s+\w+\([^)]*?,\s*(\w+)\s*,\s*(\w+)(?:\s*,\s*"[^"]+")?\s*\)', source)
    if not schema:
        raise ValueError(f"missing Excel schema: {sheet}")
    fields, defaults = tables[schema[1]], tables[schema[2]]
    if not isinstance(fields, dict) or not isinstance(defaults, list):
        raise ValueError(f"invalid Excel schema: {sheet}")
    rows = unpack_rows(source, fields, defaults)
    if blocks := re.search(r"\bb2i\s*=\s*(\w+)", source):
        boundaries = tables[blocks[1]]
        for index in range(1, len(boundaries) + 1):
            rows.extend(unpack_rows(sources[f"{path}_b{index}.lua.bytes"], fields, defaults))
    if not rows or len({row["id"] for row in rows}) != len(rows):
        raise ValueError(f"empty or duplicate Excel rows: {sheet}")
    return sorted(rows, key=lambda row: row["id"])


def integer(value, name: str, minimum: int = 0) -> int:
    if type(value) is not int or not minimum <= value <= 2**64 - 1:
        raise ValueError(f"invalid {name}: {value}")
    return value


def build_registries(sources: dict[str, str]) -> tuple[list[dict], list[dict]]:
    languages = {}
    for language in LANGUAGES:
        tables = local_tables(sources[f"{EXCEL}Langs/str_event_{language}.lua.bytes"])
        languages[language] = next(iter(tables.values()))
        if not isinstance(languages[language], list):
            raise ValueError(f"invalid language table: {language}")
    texts = {row["id"]: row for row in load_sheet(sources, "str/event")}

    def translations(text_id):
        row = texts[integer(text_id, "text ID", 1)]
        result = {}
        for language in LANGUAGES:
            index = integer(row[language], "language index", 1)
            if index > len(languages[language]):
                raise ValueError(f"language index out of range: {language}/{text_id}")
            text = languages[language][index - 1]
            if not isinstance(text, str):
                raise ValueError(f"invalid translated text: {language}/{text_id}")
            result[language] = text
        return result

    def name(names):
        for language in ("jp", "chs"):
            if names[language].strip():
                return names[language].strip()
        raise ValueError("missing translated name")

    def icon(value, prefix):
        if not isinstance(value, str) or not (match := re.fullmatch(prefix + r"(\d+)", value)):
            raise ValueError(f"invalid icon: {value}")
        return integer(int(match[1]), "icon ID", 1)

    def metadata(row, name_field, desc_field, image_field, prefix, rarities):
        deprecated = integer(row.get("deprecated") or 0, "deprecated flag")
        if deprecated not in (0, 1):
            raise ValueError(f"unsupported deprecated flag: {deprecated}")
        icon_id = icon(row[image_field], prefix)
        return {"data_id": integer(row["id"], "data ID", 1),
                "names": translations(row[name_field]), "descriptions": translations(row[desc_field]),
                "price": integer(row["price"], "price") if row.get("price") is not None else None,
                "sell_price": integer(row["sell_price"], "sell price") if row.get("sell_price") is not None else None,
                "icon_name": row[image_field] + ".png", "icon_id": icon_id,
                "deprecated": bool(deprecated), "rarity": rarities[integer(row["rarity"], "rarity", 1)]}

    def amulet_metadata(row):
        return metadata(row, "name", "desc", "card_image", "fu_", AMULET_RARITIES)

    effects = {row["id"]: row for row in load_sheet(sources, "amulet/amulet_effect")}
    amulets = []
    for raw_id, row in effects.items():
        integer(raw_id, "effect ID", 10)
        if raw_id % 10 not in (0, 1):
            raise ValueError(f"unsupported amulet variant: {raw_id}")
        if raw_id % 10:
            if raw_id - 1 not in effects:
                raise ValueError(f"amulet variant has no base: {raw_id}")
            continue
        details = amulet_metadata(row)
        item = {"id": raw_id // 10, "icon_id": details["icon_id"],
                "name": name(details["names"]), "rarity": details["rarity"],
                "sell_price": integer(row["sell_price"], "sell price"), "metadata": details}
        if upgrade := row["upgrade"]:
            if integer(upgrade, "upgrade ID") != raw_id + 1:
                raise ValueError(f"unsupported amulet upgrade: {raw_id} -> {upgrade}")
            plus = effects[upgrade]
            details = amulet_metadata(plus)
            item.update(plus_name=name(details["names"]),
                        plus_sell_price=integer(plus["sell_price"], "plus sell price"),
                        plus_metadata=details)
        elif raw_id + 1 in effects:
            raise ValueError(f"unlinked amulet variant: {raw_id + 1}")
        amulets.append(item)
    badges = []
    for row in load_sheet(sources, "amulet/amulet_badge"):
        details = metadata(row, "badge_name", "badge_desc", "badge_image", "badge_", BADGE_RARITIES)
        badges.append({"id": details["data_id"], "icon_id": details["icon_id"],
                       "name": name(details["names"]), "rarity": details["rarity"], "metadata": details})
    if not amulets or not badges:
        raise ValueError("empty amulet or badge registry")
    return amulets, badges


def updated_registry(previous: dict, incoming: list[dict], meta: dict) -> dict:
    if previous.get("schema_version") != 1:
        raise ValueError("unsupported registry schema")
    old = {row["id"]: row for row in previous["items"]}
    if len(old) != len(previous["items"]) or len({row["id"] for row in incoming}) != len(incoming):
        raise ValueError("duplicate registry IDs")
    merged = dict(old)
    for item in incoming:
        merged[item["id"]] = {**old.get(item["id"], {}), **item}
    # Keep historical IDs so saved configurations and past game records still resolve.
    items = [merged[key] for key in sorted(merged)]
    if merged == old:
        return previous
    return {**previous, "schema_version": 1, "version": integer(previous["version"], "registry version") + 1,
            "source": {"client_url": CLIENT_URL, **meta, "name_language": "jp"}, "items": items}


def generate(root: Path, lua: dict, config: dict, excels: dict, meta: dict) -> bool:
    if not re.fullmatch(r"[\w.()-]+", meta["product_version"]):
        raise ValueError("invalid client product version")
    if meta["product_version"] != "offline" and not re.fullmatch(r"[0-9a-fA-F]{32,64}", meta.get("bundle_hash", "")):
        raise ValueError("invalid client bundle hash")
    proto, rpc = build_outputs(lua, config)
    if missing := REQUIRED_RPC - rpc.keys():
        raise ValueError(f"required amulet RPCs missing: {sorted(missing)}")
    amulets, badges = build_registries(excels)
    outputs = {root / "proto/liqi.proto": proto.encode("utf-8")}
    for filename, incoming in (("amulets.json", amulets), ("badges.json", badges)):
        path = root / "backend/assets" / filename
        previous = json.loads(path.read_bytes())
        updated = updated_registry(previous, incoming, meta)
        if updated != previous:
            outputs[path] = (json.dumps(updated, ensure_ascii=False, indent=2) + "\n").encode("utf-8")
    changed = False
    # All parsing and validation completes before any generated file is replaced.
    for path, data in outputs.items():
        if path.read_bytes().replace(b"\r\n", b"\n") != data:
            path.write_bytes(data)
            changed = True
            print(f"updated {path.relative_to(root)}")
    values = {"changed": str(changed).lower(), **meta, "amulet_count": len(amulets), "badge_count": len(badges)}
    if output := os.environ.get("GITHUB_OUTPUT"):
        with open(output, "a", encoding="utf-8") as stream:
            for key in ("changed", "product_version", "bundle_hash", "amulet_count", "badge_count"):
                stream.write(f"{key}={values[key]}\n")
    print(json.dumps(values, ensure_ascii=False))
    return changed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--from-raw", type=Path, help="Read an extracted Unity raw/ directory offline")
    parser.add_argument("--output-root", type=Path, default=ROOT)
    args = parser.parse_args()
    if args.from_raw:
        base = args.from_raw / "lua/LuaByte/Lua"
        lua = {path.stem: path.read_text(encoding="utf-8") for path in (base / "Protol").glob("*_pb.lua")}
        config = json.loads((args.from_raw / "assets/MyAssets/docs/proto_config.bytes").read_bytes())
        excels = {"LuaByte/Lua/" + path.relative_to(base).as_posix() + ".bytes": path.read_text(encoding="utf-8")
                  for path in (base / "Excels").rglob("*.lua")
                  if wanted_excel("LuaByte/Lua/" + path.relative_to(base).as_posix() + ".bytes")}
        meta = {"product_version": "offline", "bundle_hash": "offline"}
        generate(args.output_root, lua, config, excels, meta)
    else:
        generate(args.output_root, *fetch_from_unity())


if __name__ == "__main__":
    main()

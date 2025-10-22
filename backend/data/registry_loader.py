from __future__ import annotations

import importlib.resources as ir  # 兼容 PyInstaller, zip 包
import json
import os
import tempfile
from pathlib import Path
from typing import Any, Dict, List, Literal, Tuple

REGISTRY_KINDS = Literal["amulets", "badges"]

SCHEMA_VERSION = 1

_AMULET_RARITIES = {"GREEN", "BLUE", "ORANGE", "PURPLE"}
_BADGE_RARITIES = {"BROWN", "BLUE", "RED"}


def _rarity_set(kind: REGISTRY_KINDS) -> set[str]:
    return _AMULET_RARITIES if kind == "amulets" else _BADGE_RARITIES


def _validate_table(data: Any, kind: REGISTRY_KINDS) -> Tuple[bool, str]:
    if not isinstance(data, dict):
        return False, "root must be object"
    if data.get("schema_version") != SCHEMA_VERSION:
        return False, f"schema_version mismatch: {data.get('schema_version')}"
    items = data.get("items")
    if not isinstance(items, list):
        return False, "items must be list"

    rar_ok = _rarity_set(kind)
    ids, names = set(), set()

    for i, row in enumerate(items):
        if not isinstance(row, dict):
            return False, f"[{i}] not object"
        for k in ("id", "icon_id", "name", "rarity"):
            if k not in row:
                return False, f"[{i}] missing {k}"

        try:
            _ = int(row["id"])
            _ = int(row["icon_id"])
        except Exception:
            return False, f"[{i}] id/icon_id must be int"

        name = str(row["name"]).strip()
        if not name:
            return False, f"[{i}] empty name"

        rarity = str(row["rarity"]).upper()
        if rarity not in rar_ok:
            return False, f"[{i}] invalid rarity: {rarity}; allowed={sorted(rar_ok)}"

        # 去重
        if row["id"] in ids:
            return False, f"[{i}] dup id: {row['id']}"
        key = name.lower()
        if key in names:
            return False, f"[{i}] dup name: {name}"
        ids.add(row["id"])
        names.add(key)

    return True, "ok"


def _atomic_write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile("w", encoding="utf-8", delete=False, dir=str(path.parent)) as tf:
        tf.write(text)
        tmp = tf.name
    os.replace(tmp, path)


def _read_external(path: Path) -> Dict[str, Any] | None:
    if not path.exists():
        return None
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return None


def _read_builtin(pkg: str, filename: str) -> Dict[str, Any]:
    with ir.as_file(ir.files(pkg) / filename) as p:
        return json.loads(p.read_text(encoding="utf-8"))


def load_registry(kind: REGISTRY_KINDS,
                  external_dir: Path | None = None,
                  write_back_if_missing: bool = True) -> Dict[str, Any]:
    filename = f"{kind}.json"  # amulets.json / badges.json

    builtin = _read_builtin("backend.data.assets", filename)
    ok_builtin, _ = _validate_table(builtin, kind)
    if not ok_builtin:
        raise ValueError(f"内置资源 {filename} 校验失败，请检查 assets 文件。")

    if external_dir:
        ext_path = external_dir / filename
        ext = _read_external(ext_path)
        if ext:
            ok, _ = _validate_table(ext, kind)
            if ok:
                return ext
        if write_back_if_missing and (not ext_path.exists()):
            _atomic_write_text(ext_path, json.dumps(builtin, ensure_ascii=False, indent=2))

    return builtin


def load_registry_list(kind: REGISTRY_KINDS,
                       external_dir: Path | None = None,
                       write_back_if_missing: bool = True) -> List[Dict[str, Any]]:
    obj = load_registry(kind, external_dir, write_back_if_missing)
    return obj["items"]

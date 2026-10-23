import contextlib
import copy
import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from majsoul_client import wanted_excel
from update_game_data import EXCEL, build_action_rules, build_levels, generate, load_sheet


class GameDataTests(unittest.TestCase):
    def test_grouped_rows_and_composite_keys_in_blocks(self):
        sources = {
            EXCEL + "Data/amulet/amulet_node.lua.bytes": '''
local fields = {["id"]=1,["node"]=2,["type"]=3,["node_name"]=1004}
local defaults = {20001,1,1,"1-START"}
local get = function(a,b) return PackGet(a,b,fields,defaults) end
local boundaries = {20001}
return {tb={},get=get,b2i=boundaries}''',
            EXCEL + "Data/amulet/amulet_node_b1.lua.bytes": '''
rows[20001]={Pack({false,false,false,false},get),Pack({false,2,2,"1-1"},get)}''',
        }
        rows = load_sheet(sources, "amulet/amulet_node", ("id", "node"))
        self.assertEqual([(row["node"], row["node_name"]) for row in rows], [(1, "1-START"), (2, "1-1")])
        with self.assertRaises(ValueError):
            load_sheet(sources, "amulet/amulet_node")
        sources[EXCEL + "Data/amulet/amulet_node_b1.lua.bytes"] = 'rows[999]={Pack({20001},get)}'
        with self.assertRaisesRegex(ValueError, "ID differs"):
            load_sheet(sources, "amulet/amulet_node", ("id", "node"))

    def sheets(self):
        return {
            "amulet/amulet_activity": [{"activity_id": 261081, "init_level": 13001}],
            "amulet/amulet_level": [
                {"activity_id": 261081, "level": 14001, "node_id": 20101, "next_level": 0},
                {"activity_id": 261081, "level": 13001, "node_id": 20001, "next_level": 14001},
            ],
            "amulet/amulet_node": [
                {"id": 20001, "node": 1, "type": 1, "node_name": "1-START"},
                {"id": 20001, "node": 2, "type": 2, "node_name": "1-1", "node_sub_id": 1, "hp_rate": "100"},
                {"id": 20001, "node": 3, "type": 3, "node_name": "1-2"},
                {"id": 20101, "node": 1, "type": 4, "node_name": "Ex1-BOSS", "node_sub_id": 1, "hp_rate": "9.999e+3000"},
            ],
            "amulet/amulet_enemy": [{"id": 3001, "group": 1, "hp": "120"}],
        }

    def test_progression_noncombat_and_exact_large_scores(self):
        sheets = self.sheets()
        with patch("update_game_data.load_sheet", side_effect=lambda sources, sheet, *args: sheets[sheet]):
            result = build_levels({})
        self.assertEqual([row["level"] for row in result["levels"]], [13001, 14001])
        start, battle, event = result["levels"][0]["nodes"]
        self.assertIsNone(start["hp_rate"])
        self.assertIsNone(event["target_points"])
        self.assertEqual(battle["target_points"], {"120": "120"})
        self.assertEqual(result["levels"][1]["nodes"][0]["target_points"], {"120": "1.19988E+3001"})
        for key, value in (("hp_rate", "nan"), ("node_sub_id", 999)):
            bad = copy.deepcopy(sheets)
            bad["amulet/amulet_node"][1][key] = value
            with patch("update_game_data.load_sheet", side_effect=lambda sources, sheet, *args: bad[sheet]):
                with self.assertRaises(ValueError):
                    build_levels({})
        sheets["amulet/amulet_level"][0]["next_level"] = 13001
        with patch("update_game_data.load_sheet", side_effect=lambda sources, sheet, *args: sheets[sheet]):
            with self.assertRaisesRegex(ValueError, "cyclic"):
                build_levels({})

    def test_required_assets_selected(self):
        for name in ("level", "node", "node_b8", "enemy", "activity", "character", "shop_upgrade", "forge", "transport", "trade_item", "gamble_selection"):
            self.assertTrue(wanted_excel(EXCEL + "Data/amulet/amulet_" + name + ".lua.bytes"))
        self.assertFalse(wanted_excel(EXCEL + "Data/mmo/mmo_level.lua.bytes"))

    def test_acquisition_damage_uses_effect_args(self):
        effects = [dict(id=effect_id, args=args, upgrade=None, duplicate_enabled=None,
                        effect_group=None, can_sell=1, rarity=2)
                   for effect_id, args in ((3550, [100, 6]), (3551, [200, 12]), (3650, [300, 9]))]
        with patch("update_game_data.load_sheet", side_effect=lambda sources, sheet, *args:
                   effects if sheet == "amulet/amulet_effect" else []):
            damage = build_action_rules({})["acquire_damage"]
        self.assertEqual(damage, {"3550": 6, "3551": 12})
        asset = Path(__file__).resolve().parents[1] / "backend/assets/action_rules.json"
        self.assertEqual(json.loads(asset.read_bytes())["rules"]["acquire_damage"], damage)

    def test_gamble_selection_ids_and_effect_arguments(self):
        expected = {"6": {"type": 2, "args": [10]}, "26": {"type": 10, "args": [10, 3]},
                    "13": {"type": 4, "args": [927]}, "11": {"type": 3, "args": [75]},
                    "42": {"type": 17, "args": [25]}}
        rows = [dict(id=50201, selection_id=int(key), **value) for key, value in expected.items()]
        with patch("update_game_data.load_sheet", side_effect=lambda sources, sheet, *args:
                   rows if sheet == "amulet/amulet_gamble_selection" else []):
            self.assertEqual(build_action_rules({})["gamble_selections"], expected)
            rows.append(dict(rows[0], id=50202))
            with self.assertRaisesRegex(ValueError, "duplicate gamble selection"):
                build_action_rules({})
        asset = Path(__file__).resolve().parents[1] / "backend/assets/action_rules.json"
        actual = json.loads(asset.read_bytes())["rules"]["gamble_selections"]
        self.assertEqual({key: actual[key] for key in expected}, expected)

    def test_generation_is_idempotent_and_validates_before_writing(self):
        from update_game_data import REQUIRED_RPC
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stdout(io.StringIO()):
            root = Path(directory)
            (root / "proto").mkdir()
            (root / "backend/assets").mkdir(parents=True)
            (root / "proto/liqi.proto").write_text("old")
            for name in ("amulets", "badges"):
                (root / ("backend/assets/" + name + ".json")).write_text(json.dumps({"schema_version": 1, "version": 1, "items": [{"id": 1}]}))
            with patch("update_game_data.build_outputs", return_value=("proto", dict.fromkeys(REQUIRED_RPC))), patch("update_game_data.build_registries", return_value=([{"id": 1}], [{"id": 1}])), patch("update_game_data.build_action_rules", return_value={"effects": {}}), patch("update_game_data.build_levels", return_value={"levels": [{"nodes": []}], "enemies": []}) as levels:
                meta = {"product_version": "offline", "bundle_hash": "offline"}
                self.assertTrue(generate(root, {}, {}, {}, meta))
                self.assertFalse(generate(root, {}, {}, {}, meta))
                levels.side_effect = ValueError("bad stages")
                with self.assertRaises(ValueError):
                    generate(root, {}, {}, {}, meta)
                self.assertEqual((root / "proto/liqi.proto").read_text(), "proto")


if __name__ == "__main__":
    unittest.main()

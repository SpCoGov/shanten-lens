"""Offline regression tests for the client-data updater."""
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import majsoul_client as client
import update_game_data as updater


class GameDataTests(unittest.TestCase):
    def test_literals_preserve_unicode_defaults_and_do_not_execute_code(self):
        parser = updater.LuaValues(r'{"日本語", "\228\184\173", nil, false, -2, Sub({3}), Dmap[2004]}')
        values = parser.value()
        self.assertEqual(values[:6], ["日本語", "中", None, False, -2, [3]])
        self.assertEqual(values[6], updater.DefaultRange(2, 4))
        self.assertEqual(updater.LuaValues('{["id"]=1, name=2}').value(), {"id": 1, "name": 2})
        for source in ('{os.execute("command")}', '{["id"]=1,["id"]=2}', '{Dmap[4002]}'):
            with self.assertRaises(ValueError):
                updater.LuaValues(source).value()

    def test_packed_rows_resolve_skipped_and_false_defaults(self):
        fields = {"id": 1, "deprecated": 2, "rarity": 3, "name": -4, "icon": 1005, "args": 6}
        defaults = [None, 0, 4, None, "fu_0001", []]
        source = 'rows[30]=Pack({Dmap[2003],30,51,false,Sub({4,6})},getter)'
        self.assertEqual(updater.unpack_rows(source, fields, defaults), [{
            "id": 30, "deprecated": 0, "rarity": 4, "name": 51, "icon": "fu_0001", "args": [4, 6],
        }])
        for bad in ('rows[31]=Pack({30},getter)', 'rows[30]=Pack({Dmap[2009],30},getter)'):
            with self.assertRaises(ValueError):
                updater.unpack_rows(bad, fields, defaults)
        with self.assertRaises(ValueError):
            updater.unpack_rows(source, {"id": 1000}, [None])

    def test_sheet_requires_every_declared_block_and_unique_ids(self):
        path = updater.EXCEL + "Data/amulet/amulet_effect"
        header = 'local fields={id=1} local defaults={nil} local blocks={30,40} return Get(a,b,fields,defaults) return {b2i=blocks}'
        sources = {path + ".lua.bytes": header, path + "_b1.lua.bytes": 'r[30]=Pack({30},get)'}
        with self.assertRaises(KeyError):
            updater.load_sheet(sources, "amulet/amulet_effect")
        sources[path + "_b2.lua.bytes"] = 'r[30]=Pack({30},get)'
        with self.assertRaises(ValueError):
            updater.load_sheet(sources, "amulet/amulet_effect")
        sources[path + "_b2.lua.bytes"] = 'r[40]=Pack({40},get)'
        self.assertEqual(updater.load_sheet(sources, "amulet/amulet_effect"), [{"id": 30}, {"id": 40}])

    def test_registries_preserve_all_languages_variants_and_deprecated_rows(self):
        texts = [{"id": index, **dict.fromkeys(updater.LANGUAGES, index)} for index in range(1, 7)]
        effects = [
            {"id": 30, "card_image": "fu_0004", "name": 1, "desc": 4, "rarity": 2,
             "price": 18, "sell_price": 9, "upgrade": 31},
            {"id": 31, "card_image": "fu_0005", "name": 2, "desc": 5, "rarity": 1,
             "price": 24, "sell_price": 12, "deprecated": 1},
            {"id": 40, "card_image": "fu_0006", "name": 1, "desc": 4, "rarity": 3,
             "price": 0, "sell_price": 0, "deprecated": 1, "upgrade": None},
        ]
        badges = [{"id": 600010, "badge_image": "badge_600010", "badge_name": 3, "badge_desc": 6, "rarity": 1},
                  {"id": 600020, "badge_image": "badge_600020", "badge_name": 3,
                   "badge_desc": 6, "rarity": 2, "deprecated": 1}]
        sources = {}
        for lang in updater.LANGUAGES:
            values = ["" if lang == "jp" else f"base-{lang}", f"plus-{lang}", f"badge-{lang}",
                      f"<b>desc-{lang}</b>\n{{0}}", f"plus-desc-{lang}", "" if lang == "kr" else f"badge-desc-{lang}"]
            sources[updater.EXCEL + f"Langs/str_event_{lang}.lua.bytes"] = "local texts=" + json.dumps(values, ensure_ascii=False).replace("[", "{").replace("]", "}")
        tables = {"str/event": texts, "amulet/amulet_effect": effects, "amulet/amulet_badge": badges}
        with patch.object(updater, "load_sheet", side_effect=lambda _, name: tables[name]):
            amulets, seals = updater.build_registries(sources)
            base, retired = amulets
            self.assertEqual((base["id"], base["name"], base["plus_name"], base["plus_sell_price"]), (3, "base-chs", "plus-jp", 12))
            self.assertEqual(base["metadata"], {
                "data_id": 30, "names": {lang: "" if lang == "jp" else f"base-{lang}" for lang in updater.LANGUAGES},
                "descriptions": {lang: f"<b>desc-{lang}</b>\n{{0}}" for lang in updater.LANGUAGES},
                "price": 18, "sell_price": 9, "icon_name": "fu_0004.png", "icon_id": 4,
                "deprecated": False, "rarity": "ORANGE",
            })
            plus = base["plus_metadata"]
            self.assertEqual((plus["data_id"], plus["price"], plus["sell_price"], plus["icon_name"], plus["rarity"]), (31, 24, 12, "fu_0005.png", "PURPLE"))
            self.assertEqual(plus["descriptions"]["en"], "plus-desc-en")
            self.assertTrue(plus["deprecated"])
            self.assertTrue(retired["metadata"]["deprecated"])
            self.assertEqual(retired["metadata"]["sell_price"], 0)
            self.assertEqual([seal["id"] for seal in seals], [600010, 600020])
            self.assertEqual(seals[0]["name"], seals[1]["name"])
            self.assertTrue(seals[1]["metadata"]["deprecated"])
            self.assertIsNone(seals[0]["metadata"]["sell_price"])
            self.assertIsNone(seals[0]["metadata"]["price"])
            self.assertEqual(seals[0]["metadata"]["descriptions"]["kr"], "")
            effects[0]["rarity"] = True
            with self.assertRaises(ValueError):
                updater.build_registries(sources)
            effects[0]["rarity"] = 2
            texts[0]["en"] = 99
            with self.assertRaises(ValueError):
                updater.build_registries(sources)
            texts[0]["en"] = 1
            effects[0]["upgrade"] = 99
            with self.assertRaises(ValueError):
                updater.build_registries(sources)

    def test_versions_only_change_when_items_change_and_preserve_historical_ids(self):
        old = {"schema_version": 1, "version": 7, "items": [
            {"id": 3, "name": "護符", "rarity": "GREEN", "icon_id": 4},
            {"id": 9, "name": "歴史", "rarity": "BLUE", "icon_id": 9},
        ]}
        incoming = [dict(old["items"][0])]
        meta = {"product_version": "4.0.47", "bundle_hash": "abc"}
        self.assertIs(updater.updated_registry(old, incoming, meta), old)
        incoming[0]["rarity"] = "ORANGE"
        new = updater.updated_registry(old, incoming, meta)
        self.assertEqual(new["version"], 8)
        self.assertEqual(new["items"][1], old["items"][1])
        self.assertIs(updater.updated_registry(new, incoming, {**meta, "bundle_hash": "other"}), new)
        incoming[0]["metadata"] = {"descriptions": {"en": "new description"}}
        updated = updater.updated_registry(new, incoming, meta)
        self.assertEqual(updated["version"], 9)
        self.assertIs(updater.updated_registry(updated, incoming, meta), updated)
        same_name = [{"id": 10, "name": "歴史", "deprecated": True}]
        self.assertEqual(len(updater.updated_registry(old, same_name, meta)["items"]), 3)
        with self.assertRaises(ValueError):
            updater.updated_registry(old, incoming * 2, meta)

    def test_failed_validation_leaves_all_outputs_untouched(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            paths = [root / "proto/liqi.proto", root / "backend/assets/amulets.json", root / "backend/assets/badges.json"]
            for path in paths:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"original")
            with patch.object(updater, "build_outputs", return_value=("new", dict.fromkeys(updater.REQUIRED_RPC))), \
                 patch.object(updater, "build_registries", side_effect=ValueError("missing rows")):
                with self.assertRaises(ValueError):
                    updater.generate(root, {}, {}, {}, {"product_version": "4.0.47", "bundle_hash": "a" * 40})
            self.assertEqual([path.read_bytes() for path in paths], [b"original"] * 3)

    def test_proto_references_are_resolved_and_unresolved_fields_fail(self):
        source = '''local p=require("protobuf")
            CHILD=p.Descriptor() CHILD.name="Child" CHILD.full_name=".lq.Child" CHILD.fields={}
            PARENT=p.Descriptor() PARENT.name="Parent" PARENT.full_name=".lq.Parent"
            FIELD=p.FieldDescriptor() FIELD.name="children" FIELD.number=7 FIELD.label=3 FIELD.type=11 FIELD.message_type=CHILD
            PARENT.fields={FIELD}'''
        module = client.parse_lua_module("test_pb", source)
        client.resolve_types({"test_pb": module})
        proto = client.render_proto({"test_pb": module}, {"Lobby": {"example": {"request": "Child", "response": "Parent"}}})
        self.assertIn("repeated .lq.Child children = 7;", proto)
        self.assertIn("rpc example (Child) returns (Parent);", proto)
        module.messages_by_var["PARENT"].fields[0].type_fqn = None
        with self.assertRaises(ValueError):
            client.render_proto({"test_pb": module}, {})

    def test_asset_selection_ignores_unrelated_tables(self):
        for path in ("Data/amulet/amulet_effect.lua.bytes", "Data/amulet/amulet_effect_b8.lua.bytes",
                     "Data/amulet/amulet_badge.lua.bytes", "Data/str/event_b31.lua.bytes", "Langs/str_event_jp.lua.bytes"):
            self.assertTrue(client.wanted_excel(updater.EXCEL + path))
        self.assertFalse(client.wanted_excel(updater.EXCEL + "Data/amulet/amulet_goods.lua.bytes"))
        for lang in updater.LANGUAGES:
            self.assertTrue(client.wanted_excel(updater.EXCEL + f"Langs/str_event_{lang}.lua.bytes"))

    def test_client_metadata_cannot_inject_workflow_outputs(self):
        for meta in ({"product_version": "4.0.47\nchanged=false", "bundle_hash": "a" * 40},
                     {"product_version": "4.0.47", "bundle_hash": "hash\nchanged=false"}):
            with self.assertRaises(ValueError):
                updater.generate(Path("unused"), {}, {}, {}, meta)


if __name__ == "__main__":
    unittest.main()

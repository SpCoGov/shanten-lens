import os
from pathlib import Path

from backend.config.manager import ConfigManager
from backend.config.table import ConfigTable


def build_manager(conf_dir: Path) -> ConfigManager:
    """Build the default config tables."""
    mgr = ConfigManager(conf_dir)
    mgr.add_table(
        ConfigTable("game", file=conf_dir / "game.json")
        .add("modify_announcement", True, desc="modify announcement", kind="bool")
        .add("public_all", False, desc="public all tiles", kind="bool")
        # .add("auto_tsumo", False, desc="auto tsumo", kind="bool")
    )
    mgr.add_table(
        ConfigTable("general", file=conf_dir / "general.json")
        .add("debug", False, desc="debug mode", kind="bool")
        .add("error_code_test", 0, desc="error code test", kind="number")
    )
    mgr.add_table(
        ConfigTable("backend", file=conf_dir / "backend.json")
        .add("host", "127.0.0.1", kind="string")
        .add("port", 8787, kind="number")
        .add("mitm_port", 10999, kind="number")
    )
    mgr.add_table(
        ConfigTable("fuse", file=conf_dir / "fuse.json")
        .add(
            "guard_skip_contains",
            {"amulets": [], "badges": []},
            desc="guard skip contains",
            kind="object",
        )
        .add("enable_skip_guard", True, kind="bool")
        .add("enable_shop_force_pick", False, kind="bool")
        .add("enable_prestart_kavi_guard", True, kind="bool")
        .add("conduction_min_count", 3, kind="int")
        .add("enable_anti_steal_eat", True, kind="bool")
        .add("enable_missing_hand_tile_guard", True, kind="bool")
        .add("enable_kavi_plus_buffer_guard", True, kind="bool")
        .add("enable_hanabi_win_guard", True, kind="bool")
        .add("enable_exit_coin_guard", True, kind="bool")
        .add("enable_exit_life_guard", False, kind="bool")
    )
    mgr.add_table(
        ConfigTable("autorun", file=conf_dir / "autorun.json")
        .add("end_count", 1, desc="target count", kind="int")
        .add("targets", [], desc="targets", kind="object")
        .add("cutoff_level", 0, desc="cutoff level", kind="int")
        .add("op_interval_ms", 1000, desc="operation interval", kind="int")
        .add("need_pionner_badge_count", 4, desc="required pionner badge count", kind="int")
        .add(
            "email_notify",
            {
                "enabled": False,
                "host": "",
                "port": 587,
                "ssl": False,
                "from": "",
                "pass": "",
                "to": "",
            },
            desc="email notify",
            kind="object",
        )
    )
    mgr.load_all()
    return mgr

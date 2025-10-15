from pathlib import Path
from backend.config.manager import ConfigManager
from backend.config.table import ConfigTable


def build_manager(conf_dir: Path) -> ConfigManager:
    """
    唯一的“默认配置表定义处”。以后新增/修改配置项只改这里。
    """
    mgr = ConfigManager(conf_dir)
    mgr.add_table(
        ConfigTable("game", file=conf_dir / "game.json")
        .add("modify_announcement", True, desc="修改公告", kind="bool")
        .add("public_all", False, desc="公开全部", kind="bool")
        .add("auto_discard", False, desc="自动打牌", kind="bool")
        .add("auto_tsumo", False, desc="自动自摸", kind="bool")
    )
    mgr.add_table(
        ConfigTable("general", file=conf_dir / "general.json")
        .add("language", "zh-CN", desc="界面语言", kind="string")
        .add("theme", "system", desc="主题", kind="string")
        .add("debug", False, desc="调试模式", kind="bool")
    ).add_table(
        ConfigTable("backend", file=conf_dir / "backend.json")
        .add("host", "127.0.0.1", kind="string")
        .add("port", 8787, kind="number")
        .add("mitm_port", 10999, kind="number")
    )
    mgr.load_all()  # 生成/补全/合并
    return mgr

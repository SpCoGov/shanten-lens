import json
import re
from pathlib import Path


def read_app_version() -> str:
    text = Path("backend/version.py").read_text(encoding="utf-8")
    match = re.search(r'APP_VERSION\s*=\s*["\']([^"\']+)["\']', text)
    if not match:
        raise SystemExit("APP_VERSION not found in backend/version.py")
    return match.group(1)


def main() -> None:
    version = read_app_version()
    app_dir = Path("app")

    tauri_config = app_dir / "src-tauri" / "tauri.conf.json"
    tauri_data = json.loads(tauri_config.read_text(encoding="utf-8"))
    tauri_data["version"] = version
    tauri_config.write_text(
        json.dumps(tauri_data, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )

    frontend_version = app_dir / "src" / "lib" / "version.ts"
    version_text = frontend_version.read_text(encoding="utf-8")
    version_text = re.sub(
        r'APP_VERSION\s*=\s*"[^"]+"',
        f'APP_VERSION = "{version}"',
        version_text,
        count=1,
    )
    frontend_version.write_text(version_text, encoding="utf-8")
    print(version)


if __name__ == "__main__":
    main()

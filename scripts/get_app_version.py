import re
from pathlib import Path


def main() -> None:
    text = Path("backend/version.py").read_text(encoding="utf-8")
    match = re.search(r'APP_VERSION\s*=\s*["\']([^"\']+)["\']', text)
    if not match:
        raise SystemExit("APP_VERSION not found in backend/version.py")
    print(match.group(1))


if __name__ == "__main__":
    main()

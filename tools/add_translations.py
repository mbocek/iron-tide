"""Adds or removes interface texts in every translation file, keeping the files' order and layout.

Usage: python3 tools/add_translations.py add "English" "Slovak" "Czech"
       python3 tools/add_translations.py remove "English"
"""
import json
import sys
from pathlib import Path

FILES = {"sk": 0, "cs": 1}
DIRECTORY = Path(__file__).resolve().parent.parent / "assets" / "i18n"


def rewrite(code, change):
    path = DIRECTORY / f"{code}.json"
    table = json.loads(path.read_text(encoding="utf-8"))
    change(table)
    rows = [f"  {json.dumps(english, ensure_ascii=False)}: {json.dumps(text, ensure_ascii=False)}"
            for english, text in table.items()]
    path.write_text("{\n" + ",\n".join(rows) + "\n}\n", encoding="utf-8")


def main(arguments):
    action, english, *translations = arguments
    for code, column in FILES.items():
        if action == "add":
            rewrite(code, lambda table: table.__setitem__(english, translations[column]))
        elif action == "remove":
            rewrite(code, lambda table: table.pop(english, None))
        else:
            sys.exit(__doc__)


if __name__ == "__main__":
    main(sys.argv[1:])

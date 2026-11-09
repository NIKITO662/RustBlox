"""Validates news.json before you publish it.   python tools/validate_news.py path/to/news.json"""
import json
import re
import sys

path = sys.argv[1] if len(sys.argv) > 1 else "assets/news.json"
errors = []
with open(path, encoding="utf-8") as f:
    data = json.load(f)

posts = data.get("posts")
if not isinstance(posts, list):
    sys.exit("error: 'posts' must be a list")

seen = set()
for i, p in enumerate(posts):
    where = f"post #{i + 1} ({p.get('id', '?')})"
    for key in ("id", "date", "title", "body"):
        if not isinstance(p.get(key), str) or not p[key].strip():
            errors.append(f"{where}: '{key}' is required")
    if p.get("id") in seen:
        errors.append(f"{where}: duplicate id")
    seen.add(p.get("id"))
    if not re.fullmatch(r"\d{4}-\d{2}-\d{2}", str(p.get("date", ""))):
        errors.append(f"{where}: date must be YYYY-MM-DD")
    if p.get("link") and not str(p["link"]).startswith("https://"):
        errors.append(f"{where}: link must start with https://")
    if len(str(p.get("title", ""))) > 80:
        errors.append(f"{where}: title is over 80 characters")

if errors:
    print("\n".join("error: " + e for e in errors))
    sys.exit(1)
print(f"ok: {len(posts)} post(s)")

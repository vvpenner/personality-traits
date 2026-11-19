#!/usr/bin/env python3
"""Мгновенная выгрузка статистики «Центрального ключа» в Google Sheets.

Перезаписывает только вкладку «Центральный бот»: почта, Telegram, имя,
регистрация и даты первых нажатий на каждую кнопку курса.
"""
import re
import sqlite3
from datetime import datetime, timedelta, timezone
from pathlib import Path

import gspread
import yaml
from google.oauth2.service_account import Credentials

BASE = Path("/opt/central")
DB = BASE / "bot.db"
EVENTS = BASE / "central.yaml"
SA_FILE = Path("/opt/perkeys/gsa.json")
SHEET_ID = "1lLMlRCMK3U-TLyW32MKqKHpuAqDwmK3kIJdDh-jS71M"
SHEET_NAME = "Центральный бот"
MSK = timezone(timedelta(hours=3))


def msk(ts: str | None) -> str:
    if not ts:
        return ""
    try:
        dt = datetime.fromisoformat(ts)
        if dt.tzinfo is None:
            dt = dt.replace(tzinfo=timezone.utc)
        return dt.astimezone(MSK).strftime("%d.%m.%Y %H:%M")
    except Exception:
        return ts[:16]


def event_label(eid: str) -> str:
    m = re.search(r"day_(\d+)", eid)
    if m:
        if "practice" in eid:
            suffix = "практика"
        elif "outro" in eid:
            suffix = "выходное"
        else:
            suffix = "инфо"
        return f"День {m.group(1)} · {suffix}"
    m = re.fullmatch(r"survey_(\d+)", eid)
    if m:
        return f"День {m.group(1)} · опрос"
    return {
        "start": "Старт",
        "posttest": "Пост-тест",
        "posttest_survey": "Пост-тест · опрос",
    }.get(eid, eid)


def main() -> None:
    course = yaml.safe_load(EVENTS.read_text(encoding="utf-8"))
    events = course["messages"]

    columns: list[tuple[str, str, str]] = []
    seen: set[tuple[str, str]] = set()
    for ev in events:
        eid = ev.get("id", "")
        for b in ev.get("buttons") or []:
            if b.get("url"):
                continue
            key = (eid, b.get("id", ""))
            if key in seen:
                continue
            seen.add(key)
            columns.append((eid, key[1], f"{event_label(eid)} · {b.get('text', '')}"))

    con = sqlite3.connect(DB)
    con.row_factory = sqlite3.Row
    users = con.execute(
        "SELECT telegram_id, email, username, first_name, registered_at "
        "FROM users WHERE email != '' ORDER BY registered_at"
    ).fetchall()

    first_press: dict[tuple, str] = {}
    for r in con.execute(
        "SELECT telegram_id, message_id, button_id, MIN(pressed_at) AS p "
        "FROM button_events GROUP BY telegram_id, message_id, button_id"
    ):
        first_press[(r["telegram_id"], r["message_id"], r["button_id"])] = r["p"]

    header = ["Почта", "Telegram", "Имя", "Регистрация"] + [h for _, _, h in columns]
    rows = [header]
    for u in users:
        row = [
            u["email"],
            f"@{u['username']}" if u["username"] else "",
            u["first_name"] or "",
            msk(u["registered_at"]),
        ]
        for mid, bid, _ in columns:
            row.append(msk(first_press.get((u["telegram_id"], mid, bid))))
        rows.append(row)

    creds = Credentials.from_service_account_file(
        SA_FILE, scopes=["https://www.googleapis.com/auth/spreadsheets"]
    )
    gc = gspread.authorize(creds)
    sheet = gc.open_by_key(SHEET_ID).worksheet(SHEET_NAME)
    sheet.clear()
    sheet.update(rows, "A1")
    sheet.freeze(rows=1)
    print(f"OK: {SHEET_NAME}; участников {len(rows) - 1}, столбцов {len(header)}")


if __name__ == "__main__":
    main()

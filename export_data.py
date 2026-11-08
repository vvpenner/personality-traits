#!/usr/bin/env python3
"""Выгрузка данных бота практикума LESSSTRESS из bot.db в Excel.

Запуск:  python3 export_data.py [путь_к_bot.db] [выходной_файл.xlsx]
По умолчанию: bot.db -> export.xlsx в текущей папке.
"""
import sqlite3
import sys
from datetime import datetime, timezone, timedelta

import pandas as pd

DB = sys.argv[1] if len(sys.argv) > 1 else "bot.db"
OUT = sys.argv[2] if len(sys.argv) > 2 else "export.xlsx"

MSK = timezone(timedelta(hours=3), "МСК")


def to_msk(ts):
    """RFC3339 (UTC) -> строка московского времени."""
    if not ts:
        return ""
    try:
        dt = datetime.fromisoformat(str(ts).replace("Z", "+00:00"))
        return dt.astimezone(MSK).strftime("%d.%m.%Y %H:%M:%S")
    except Exception:
        return str(ts)


con = sqlite3.connect(DB)

users = pd.read_sql_query(
    "SELECT telegram_id, phone, email, username, first_name, registered_at FROM users",
    con,
)
users["registered_at"] = users["registered_at"].map(to_msk)
users.columns = ["Telegram ID", "Телефон", "Email", "Username", "Имя", "Регистрация (МСК)"]

buttons = pd.read_sql_query(
    """SELECT telegram_id, session_id, message_id, button_id, action,
              event_target, pressed_at
       FROM button_events ORDER BY pressed_at""",
    con,
)
buttons["pressed_at"] = buttons["pressed_at"].map(to_msk)
buttons.columns = [
    "Telegram ID", "Сессия", "Сообщение сценария", "Кнопка",
    "Действие", "Следующее событие", "Нажатие (МСК)",
]

sessions = pd.read_sql_query(
    """SELECT id, telegram_id, started_at, finished_at, current_index
       FROM course_sessions ORDER BY id""",
    con,
)
sessions["started_at"] = sessions["started_at"].map(to_msk)
sessions["finished_at"] = sessions["finished_at"].map(to_msk)
sessions.columns = [
    "Сессия", "Telegram ID", "Начало (МСК)", "Завершение (МСК)", "Текущий шаг (индекс)",
]

with pd.ExcelWriter(OUT, engine="openpyxl") as w:
    users.to_excel(w, sheet_name="Участники", index=False)
    buttons.to_excel(w, sheet_name="Нажатия кнопок", index=False)
    sessions.to_excel(w, sheet_name="Сессии", index=False)

print(f"Готово: {OUT}")
print(f"  Участников: {len(users)}, нажатий: {len(buttons)}, сессий: {len(sessions)}")

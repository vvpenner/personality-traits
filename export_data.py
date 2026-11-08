#!/usr/bin/env python3
"""Выгрузка данных бота практикума LESSSTRESS из bot.db в Excel.

Запуск:  python3 export_data.py [путь_к_bot.db] [выходной_файл.xlsx]
По умолчанию: bot.db -> export.xlsx в текущей папке.

Листы:
  - "Прогресс по дням": участники x дни практикума, в ячейках — дата/время
    нажатий кнопок в этот день (московское время).
  - "Участники": контакты и дата регистрации.
  - "Нажатия кнопок": все нажатия подряд.
  - "Сессии": старт/завершение прохождений курса.
"""
import re
import sqlite3
import sys
from datetime import datetime, timezone, timedelta

import pandas as pd

DB = sys.argv[1] if len(sys.argv) > 1 else "bot.db"
OUT = sys.argv[2] if len(sys.argv) > 2 else "export.xlsx"

MSK = timezone(timedelta(hours=3), "МСК")

# Событие сценария -> название столбца на листе «Прогресс по дням».
SURVEY_COLUMNS = {
    "survey_7": "Опрос нед. 1",
    "survey_14": "Опрос нед. 2",
    "survey_21": "Опрос нед. 3",
    "survey_28": "Выходной тест",
    "posttest_survey": "Пост-тест",
}
DAY_RE = re.compile(r"^day_(\d+)_")


def to_msk(ts):
    """RFC3339 (UTC) -> datetime в московском времени (или None)."""
    if not ts:
        return None
    try:
        return datetime.fromisoformat(str(ts).replace("Z", "+00:00")).astimezone(MSK)
    except Exception:
        return None


def fmt_dt(dt):
    return dt.strftime("%d.%m.%Y %H:%M:%S") if dt else ""


def column_for(message_id):
    """Определяет столбец прогресса по id события сценария."""
    if not message_id:
        return None
    m = DAY_RE.match(message_id)
    if m:
        return f"{int(m.group(1))} день"
    return SURVEY_COLUMNS.get(message_id)


con = sqlite3.connect(DB)

users = pd.read_sql_query(
    "SELECT telegram_id, phone, email, username, first_name, registered_at FROM users",
    con,
)
users["registered_at"] = users["registered_at"].map(lambda t: fmt_dt(to_msk(t)))
users.columns = ["Telegram ID", "Телефон", "Email", "Username", "Имя", "Регистрация (МСК)"]

buttons = pd.read_sql_query(
    """SELECT telegram_id, session_id, message_id, button_id, action,
              event_target, pressed_at
       FROM button_events ORDER BY pressed_at""",
    con,
)
buttons["pressed_at"] = buttons["pressed_at"].map(lambda t: fmt_dt(to_msk(t)))
buttons.columns = [
    "Telegram ID", "Сессия", "Сообщение сценария", "Кнопка",
    "Действие", "Следующее событие", "Нажатие (МСК)",
]

sessions = pd.read_sql_query(
    """SELECT id, telegram_id, started_at, finished_at, current_index
       FROM course_sessions ORDER BY id""",
    con,
)
sessions["started_at"] = sessions["started_at"].map(lambda t: fmt_dt(to_msk(t)))
sessions["finished_at"] = sessions["finished_at"].map(lambda t: fmt_dt(to_msk(t)))
sessions.columns = [
    "Сессия", "Telegram ID", "Начало (МСК)", "Завершение (МСК)", "Текущий шаг (индекс)",
]

# --- Лист «Прогресс по дням» -----------------------------------------------
raw = pd.read_sql_query(
    "SELECT telegram_id, message_id, pressed_at FROM button_events", con
)

day_columns = [f"{d} день" for d in range(1, 29)] + list(SURVEY_COLUMNS.values())

progress_rows = []
for _, u in users.iterrows():
    tg = u["Telegram ID"]
    row = {
        "Email": u["Email"],
        "Телефон": u["Телефон"],
        "Имя": u["Имя"],
        "Telegram ID": tg,
    }
    presses = {}
    for _, b in raw[raw["telegram_id"] == tg].iterrows():
        col = column_for(b["message_id"])
        if col is None:
            continue
        dt = to_msk(b["pressed_at"])
        if dt is None:
            continue
        presses.setdefault(col, []).append(dt)
    for col in day_columns:
        times = presses.get(col)
        if not times:
            row[col] = ""
        elif len(times) == 1:
            row[col] = fmt_dt(times[0])
        else:
            row[col] = f"{fmt_dt(times[0])} → {times[-1].strftime('%H:%M:%S')}"
    progress_rows.append(row)

progress = pd.DataFrame(
    progress_rows, columns=["Email", "Телефон", "Имя", "Telegram ID"] + day_columns
)

with pd.ExcelWriter(OUT, engine="openpyxl") as w:
    progress.to_excel(w, sheet_name="Прогресс по дням", index=False)
    users.to_excel(w, sheet_name="Участники", index=False)
    buttons.to_excel(w, sheet_name="Нажатия кнопок", index=False)
    sessions.to_excel(w, sheet_name="Сессии", index=False)

print(f"Готово: {OUT}")
print(f"  Участников: {len(users)}, нажатий: {len(buttons)}, сессий: {len(sessions)}")

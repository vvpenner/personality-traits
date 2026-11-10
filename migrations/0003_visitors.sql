-- Журнал посетителей: каждый, кто написал боту, даже без регистрации.
CREATE TABLE IF NOT EXISTS visitors (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    telegram_id INTEGER NOT NULL,
    username TEXT,
    first_name TEXT,
    text TEXT,
    seen_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_visitors_tg ON visitors(telegram_id);

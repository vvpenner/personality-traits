CREATE TABLE IF NOT EXISTS users (
    telegram_id   INTEGER PRIMARY KEY,
    phone         TEXT NOT NULL DEFAULT '',
    email         TEXT NOT NULL DEFAULT '',
    username      TEXT,
    first_name    TEXT,
    registered_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS course_sessions (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    telegram_id     INTEGER NOT NULL,
    started_at      TEXT NOT NULL,
    finished_at     TEXT,
    current_index   INTEGER NOT NULL DEFAULT 0,
    last_action_at  TEXT NOT NULL,
    FOREIGN KEY (telegram_id) REFERENCES users(telegram_id)
);

CREATE INDEX IF NOT EXISTS idx_sessions_active
    ON course_sessions(telegram_id) WHERE finished_at IS NULL;

CREATE TABLE IF NOT EXISTS scheduled_messages (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id  INTEGER NOT NULL,
    telegram_id INTEGER NOT NULL,
    message_id  TEXT NOT NULL,
    send_after  TEXT NOT NULL,
    sent_at     TEXT,
    FOREIGN KEY (session_id) REFERENCES course_sessions(id)
);

CREATE INDEX IF NOT EXISTS idx_pending_scheduled
    ON scheduled_messages(send_after) WHERE sent_at IS NULL;

CREATE TABLE IF NOT EXISTS button_events (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    telegram_id INTEGER NOT NULL,
    session_id  INTEGER,
    message_id  TEXT NOT NULL,
    button_id   TEXT NOT NULL,
    action      TEXT NOT NULL,
    event_target TEXT,
    pressed_at  TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_button_events_user
    ON button_events(telegram_id, pressed_at);

use std::str::FromStr;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

pub type Pool = SqlitePool;

pub async fn connect(url: &str) -> Result<Pool> {
    let opts = SqliteConnectOptions::from_str(url)?.create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("run migrations")?;
    Ok(pool)
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct User {
    pub telegram_id: i64,
    pub phone: String,
    pub email: String,
}

impl User {
    /// Регистрация завершена: указан email.
    pub fn is_registered(&self) -> bool {
        !self.email.is_empty()
    }
}

pub async fn get_user(pool: &Pool, tg_id: i64) -> Result<Option<User>> {
    let row: Option<(i64, String, String)> = sqlx::query_as(
        "SELECT telegram_id, phone, email FROM users WHERE telegram_id = ?",
    )
    .bind(tg_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(telegram_id, phone, email)| User {
        telegram_id,
        phone,
        email,
    }))
}

/// Регистрация по email: создаёт пользователя (если нового) и сохраняет почту.
pub async fn upsert_email(
    pool: &Pool,
    tg_id: i64,
    email: &str,
    username: Option<&str>,
    first_name: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO users (telegram_id, email, username, first_name, registered_at) \
         VALUES (?, ?, ?, ?, ?) \
         ON CONFLICT(telegram_id) DO UPDATE SET \
            email = excluded.email, \
            username = excluded.username, \
            first_name = excluded.first_name",
    )
    .bind(tg_id)
    .bind(email)
    .bind(username)
    .bind(first_name)
    .bind(Utc::now().to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

/// Шаг 1 регистрации: сохранить номер телефона (из Telegram contact).
#[allow(dead_code)]
pub async fn upsert_phone(
    pool: &Pool,
    tg_id: i64,
    phone: &str,
    username: Option<&str>,
    first_name: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO users (telegram_id, phone, username, first_name, registered_at) \
         VALUES (?, ?, ?, ?, ?) \
         ON CONFLICT(telegram_id) DO UPDATE SET \
            phone = excluded.phone, \
            username = excluded.username, \
            first_name = excluded.first_name",
    )
    .bind(tg_id)
    .bind(phone)
    .bind(username)
    .bind(first_name)
    .bind(Utc::now().to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

/// Шаг 2 регистрации: сохранить email.
#[allow(dead_code)]
pub async fn set_email(pool: &Pool, tg_id: i64, email: &str) -> Result<()> {
    sqlx::query("UPDATE users SET email = ? WHERE telegram_id = ?")
        .bind(email)
        .bind(tg_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Session {
    pub id: i64,
    pub telegram_id: i64,
    pub current_index: i64,
}

pub async fn active_session(pool: &Pool, tg_id: i64) -> Result<Option<Session>> {
    let row: Option<(i64, i64, i64)> = sqlx::query_as(
        "SELECT id, telegram_id, current_index FROM course_sessions \
         WHERE telegram_id = ? AND finished_at IS NULL \
         ORDER BY id DESC LIMIT 1",
    )
    .bind(tg_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(id, telegram_id, current_index)| Session {
        id,
        telegram_id,
        current_index,
    }))
}

pub async fn start_session(pool: &Pool, tg_id: i64) -> Result<Session> {
    let now = Utc::now().to_rfc3339();
    let id = sqlx::query(
        "INSERT INTO course_sessions (telegram_id, started_at, current_index, last_action_at) \
         VALUES (?, ?, ?, ?)",
    )
    .bind(tg_id)
    .bind(&now)
    .bind(0_i64)
    .bind(&now)
    .execute(pool)
    .await?
    .last_insert_rowid();
    Ok(Session {
        id,
        telegram_id: tg_id,
        current_index: 0,
    })
}

pub async fn finish_all_active(pool: &Pool, tg_id: i64) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE scheduled_messages SET sent_at = ? \
         WHERE sent_at IS NULL AND session_id IN \
             (SELECT id FROM course_sessions WHERE telegram_id = ? AND finished_at IS NULL)",
    )
    .bind(&now)
    .bind(tg_id)
    .execute(pool)
    .await?;
    sqlx::query(
        "UPDATE course_sessions SET finished_at = ? \
         WHERE telegram_id = ? AND finished_at IS NULL",
    )
    .bind(&now)
    .bind(tg_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn finish_session(pool: &Pool, session_id: i64) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE scheduled_messages SET sent_at = ? WHERE session_id = ? AND sent_at IS NULL")
        .bind(&now)
        .bind(session_id)
        .execute(pool)
        .await?;
    sqlx::query("UPDATE course_sessions SET finished_at = ? WHERE id = ?")
        .bind(&now)
        .bind(session_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_session_index(pool: &Pool, session_id: i64, index: i64) -> Result<()> {
    sqlx::query("UPDATE course_sessions SET current_index = ?, last_action_at = ? WHERE id = ?")
        .bind(index)
        .bind(Utc::now().to_rfc3339())
        .bind(session_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn record_button(
    pool: &Pool,
    tg_id: i64,
    session_id: Option<i64>,
    message_id: &str,
    button_id: &str,
    action: &str,
    event_target: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO button_events \
            (telegram_id, session_id, message_id, button_id, action, event_target, pressed_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(tg_id)
    .bind(session_id)
    .bind(message_id)
    .bind(button_id)
    .bind(action)
    .bind(event_target)
    .bind(Utc::now().to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn schedule(
    pool: &Pool,
    session_id: i64,
    tg_id: i64,
    message_id: &str,
    send_after: DateTime<Utc>,
) -> Result<bool> {
    // INSERT OR IGNORE + уникальный индекс (миграция 0002): повторная
    // постановка того же сообщения в ту же сессию молча игнорируется —
    // защита от двойных нажатий на кнопки.
    let res = sqlx::query(
        "INSERT OR IGNORE INTO scheduled_messages (session_id, telegram_id, message_id, send_after) \
         VALUES (?, ?, ?, ?)",
    )
    .bind(session_id)
    .bind(tg_id)
    .bind(message_id)
    .bind(send_after.to_rfc3339())
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
}

/// Журнал посетителей: фиксируем каждое входящее сообщение (кто, когда, что).
pub async fn log_visitor(
    pool: &Pool,
    tg_id: i64,
    username: Option<&str>,
    first_name: Option<&str>,
    text: &str,
) -> Result<()> {
    let text: String = text.chars().take(120).collect();
    sqlx::query(
        "INSERT INTO visitors (telegram_id, username, first_name, text, seen_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(tg_id)
    .bind(username)
    .bind(first_name)
    .bind(&text)
    .bind(Utc::now().to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

/// Было ли такое же нажатие этой кнопки в последние `within_secs` секунд.
pub async fn recent_button_exists(
    pool: &Pool,
    tg_id: i64,
    message_id: &str,
    button_id: &str,
    within_secs: i64,
) -> Result<bool> {
    let cutoff = (Utc::now() - chrono::Duration::seconds(within_secs)).to_rfc3339();
    let row: Option<(i64,)> = sqlx::query_as(
        "SELECT id FROM button_events \
         WHERE telegram_id = ? AND message_id = ? AND button_id = ? AND pressed_at > ? \
         LIMIT 1",
    )
    .bind(tg_id)
    .bind(message_id)
    .bind(button_id)
    .bind(cutoff)
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some())
}

#[derive(Debug)]
pub struct ScheduledMessage {
    pub id: i64,
    pub session_id: i64,
    pub telegram_id: i64,
    pub message_id: String,
}

pub async fn due_messages(pool: &Pool, now: DateTime<Utc>) -> Result<Vec<ScheduledMessage>> {
    let rows: Vec<(i64, i64, i64, String)> = sqlx::query_as(
        "SELECT id, session_id, telegram_id, message_id FROM scheduled_messages \
         WHERE sent_at IS NULL AND send_after <= ? \
         ORDER BY send_after ASC LIMIT 50",
    )
    .bind(now.to_rfc3339())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(id, session_id, telegram_id, message_id)| ScheduledMessage {
                id,
                session_id,
                telegram_id,
                message_id,
            },
        )
        .collect())
}

pub async fn mark_sent(pool: &Pool, id: i64) -> Result<()> {
    sqlx::query("UPDATE scheduled_messages SET sent_at = ? WHERE id = ?")
        .bind(Utc::now().to_rfc3339())
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

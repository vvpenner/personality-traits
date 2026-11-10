use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use once_cell::sync::Lazy;
use regex::Regex;
use teloxide::prelude::*;
use teloxide::types::{
    ButtonRequest, InlineKeyboardButton, InlineKeyboardMarkup, KeyboardButton,
    KeyboardMarkup, KeyboardRemove, MaybeInaccessibleMessage, ReplyMarkup,
};

use crate::db::{self, Pool};
use crate::events::Course;
use crate::media;

static EMAIL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}$").unwrap()
});

const ASK_PHONE: &str = "Здравствуйте! Это бот практикума LESSSTRESS 🌿\n\n\
     Для регистрации, пожалуйста, поделитесь своим номером телефона.\n\n\
     👇 Нажмите кнопку «Поделиться номером телефона» — она находится ПОД полем ввода \
     сообщения, внизу экрана (если её не видно, нажмите на значок клавиатуры рядом с полем ввода).\n\n\
     Либо просто отправьте номер сообщением, например: +79001234567";

static PHONE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\+?[0-9][0-9\s\-\(\)]{8,17}[0-9]$").unwrap()
});

/// Нормализует введённый вручную номер: оставляет цифры, приводит 8... к +7...
fn normalize_phone(raw: &str) -> Option<String> {
    if !PHONE_RE.is_match(raw) {
        return None;
    }
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 10 || digits.len() > 15 {
        return None;
    }
    let normalized = if digits.len() == 11 && digits.starts_with('8') {
        format!("+7{}", &digits[1..])
    } else if digits.len() == 11 && digits.starts_with('7') {
        format!("+{}", digits)
    } else if digits.len() == 10 {
        format!("+7{}", digits)
    } else {
        format!("+{}", digits)
    };
    Some(normalized)
}

const ASK_EMAIL: &str = "Спасибо! Теперь отправьте, пожалуйста, ваш email одним сообщением.\n\n\
     ❗️ Важно: указывайте ту же почту, что вы указали в заявке на практикум, — \
     с этой же почты вы будете проходить опросы. Так мы сможем идентифицировать вас \
     для предоставления обратной связи. Обещаем строгую конфиденциальность!";

pub async fn on_message(
    bot: Bot,
    msg: Message,
    pool: Pool,
    course: Arc<Course>,
) -> Result<(), teloxide::RequestError> {
    if let Err(e) = on_message_inner(&bot, &msg, &pool, &course).await {
        tracing::error!("message handler error: {:?}", e);
        if let Some(user) = msg.from.as_ref() {
            let _ = bot
                .send_message(
                    ChatId(user.id.0 as i64),
                    "Что-то пошло не так. Попробуйте /start.",
                )
                .await;
        }
    }
    Ok(())
}

async fn on_message_inner(bot: &Bot, msg: &Message, pool: &Pool, course: &Course) -> Result<()> {
    let Some(user) = msg.from.as_ref() else {
        return Ok(());
    };
    let tg_id = user.id.0 as i64;
    let chat = msg.chat.id;
    let username = user.username.as_deref();
    let first_name = Some(user.first_name.as_str());

    // Журнал посетителей: фиксируем факт обращения (до любых проверок).
    {
        let preview: String = msg
            .text()
            .or_else(|| msg.contact().map(|_| "[контакт]"))
            .unwrap_or("[медиа/другое]")
            .chars()
            .take(120)
            .collect();
        if let Err(e) = db::log_visitor(pool, tg_id, username, first_name, &preview).await {
            tracing::warn!("visitor log failed: {:?}", e);
        }
    }

    // Шаг 1 регистрации: пользователь поделился контактом.
    if let Some(contact) = msg.contact() {
        return handle_contact(bot, chat, tg_id, username, first_name, contact, pool).await;
    }

    let text = msg.text().unwrap_or("").trim();
    let known = db::get_user(pool, tg_id).await?;

    match text {
        "/start" => handle_start(bot, chat, tg_id, pool, course, known.as_ref()).await,
        "/restart" => handle_restart(bot, chat, tg_id, pool, course, known.as_ref()).await,
        "/stats" => handle_stats(bot, chat, tg_id, pool, course).await,
        "/help" => {
            bot.send_message(
                chat,
                "Команды:\n\
                 /start — начать или продолжить курс\n\
                 /restart — начать курс заново",
            )
            .await?;
            Ok(())
        }
        _ => match &known {
            None => {
                if let Some(phone) = normalize_phone(text) {
                    db::upsert_phone(pool, tg_id, &phone, username, first_name).await?;
                    tracing::info!("user {} saved typed phone (new user)", tg_id);
                    bot.send_message(chat, "Номер сохранён ✅").await?;
                    bot.send_message(chat, ASK_EMAIL).await?;
                } else {
                    ask_phone(bot, chat).await?;
                }
                Ok(())
            }
            Some(u) if u.phone.is_empty() => {
                if let Some(phone) = normalize_phone(text) {
                    db::upsert_phone(pool, tg_id, &phone, username, first_name).await?;
                    tracing::info!("user {} saved typed phone", tg_id);
                    bot.send_message(chat, "Номер сохранён ✅").await?;
                    bot.send_message(chat, ASK_EMAIL).await?;
                } else {
                    ask_phone(bot, chat).await?;
                }
                Ok(())
            }
            Some(u) if u.email.is_empty() => {
                handle_email_input(bot, chat, tg_id, text, pool, course).await
            }
            Some(_) => {
                bot.send_message(
                    chat,
                    "Пожалуйста, используйте кнопки. Команды: /start, /restart, /help.",
                )
                .await?;
                Ok(())
            }
        },
    }
}

/// Просит поделиться номером телефона (reply-клавиатура с request_contact).
async fn ask_phone(bot: &Bot, chat: ChatId) -> Result<()> {
    let kb = KeyboardMarkup::new(vec![vec![
        KeyboardButton::new("📱 Поделиться номером телефона").request(ButtonRequest::Contact),
    ]])
    .resize_keyboard()
    .input_field_placeholder("👇 Кнопка «Поделиться номером» — внизу экрана".to_string())
    .one_time_keyboard();
    bot.send_message(chat, ASK_PHONE).reply_markup(kb).await?;
    Ok(())
}

async fn handle_contact(
    bot: &Bot,
    chat: ChatId,
    tg_id: i64,
    username: Option<&str>,
    first_name: Option<&str>,
    contact: &teloxide::types::Contact,
    pool: &Pool,
) -> Result<()> {
    // Защита от пересылки чужого контакта.
    if let Some(contact_owner) = contact.user_id {
        if contact_owner.0 != tg_id as u64 {
            bot.send_message(
                chat,
                "Пожалуйста, поделитесь именно своим номером — через кнопку ниже.",
            )
            .await?;
            return ask_phone(bot, chat).await;
        }
    }

    db::upsert_phone(pool, tg_id, &contact.phone_number, username, first_name).await?;
    tracing::info!("user {} saved phone", tg_id);

    // Убираем reply-клавиатуру и просим email.
    bot.send_message(chat, "Номер сохранён ✅")
        .reply_markup(ReplyMarkup::KeyboardRemove(KeyboardRemove::new()))
        .await?;
    bot.send_message(chat, ASK_EMAIL).await?;
    Ok(())
}

async fn handle_email_input(
    bot: &Bot,
    chat: ChatId,
    tg_id: i64,
    text: &str,
    pool: &Pool,
    course: &Course,
) -> Result<()> {
    if EMAIL_RE.is_match(text) {
        db::set_email(pool, tg_id, text).await?;
        bot.send_message(
            chat,
            format!(
                "Email сохранён: {}\n\n\
                 Пожалуйста, указывайте эту же почту при прохождении всех опросов практикума.",
                text
            ),
        )
        .await?;
        send_intro(bot, chat, course, false).await?;
    } else {
        bot.send_message(
            chat,
            "Похоже, это не email. Пожалуйста, отправьте корректный email одним сообщением \
             (например, name@example.com). Он должен совпадать с почтой из вашей заявки.",
        )
        .await?;
    }
    Ok(())
}

async fn handle_start(
    bot: &Bot,
    chat: ChatId,
    tg_id: i64,
    pool: &Pool,
    course: &Course,
    known: Option<&db::User>,
) -> Result<()> {
    match known {
        None => {
            ask_phone(bot, chat).await?;
        }
        Some(u) if u.phone.is_empty() => {
            ask_phone(bot, chat).await?;
        }
        Some(u) if u.email.is_empty() => {
            bot.send_message(chat, ASK_EMAIL).await?;
        }
        Some(_) => {
            let has_session = db::active_session(pool, tg_id).await?.is_some();
            send_intro(bot, chat, course, has_session).await?;
        }
    }
    Ok(())
}

async fn handle_restart(
    bot: &Bot,
    chat: ChatId,
    tg_id: i64,
    pool: &Pool,
    course: &Course,
    known: Option<&db::User>,
) -> Result<()> {
    match known {
        Some(u) if u.is_registered() => send_intro(bot, chat, course, true).await?,
        _ => return handle_start(bot, chat, tg_id, pool, course, known).await,
    }
    Ok(())
}

async fn send_intro(bot: &Bot, chat: ChatId, course: &Course, restart_mode: bool) -> Result<()> {
    let Some(intro) = course.first() else {
        bot.send_message(chat, "Курс пуст. Свяжитесь с администратором.")
            .await?;
        return Ok(());
    };

    let files: Vec<_> = intro
        .files
        .iter()
        .filter_map(|f| course.resolve_file(f))
        .collect();

    let (button_text, callback_data) = if restart_mode {
        ("Начать курс заново".to_string(), "crs:restart".to_string())
    } else {
        let label = intro
            .buttons
            .first()
            .map(|b| b.text.clone())
            .unwrap_or_else(|| "Начать курс".to_string());
        (label, "crs:start".to_string())
    };

    let kb = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
        button_text,
        callback_data,
    )]]);

    let text = if intro.text.trim().is_empty() {
        "Добро пожаловать!"
    } else {
        intro.text.as_str()
    };
    media::send_event_message(bot, chat, &files, text, Some(kb)).await;
    Ok(())
}

pub async fn on_callback(
    bot: Bot,
    q: CallbackQuery,
    pool: Pool,
    course: Arc<Course>,
) -> Result<(), teloxide::RequestError> {
    if let Err(e) = on_callback_inner(&bot, &q, &pool, &course).await {
        tracing::error!("callback handler error: {:?}", e);
    }
    let _ = bot.answer_callback_query(q.id.clone()).await;
    Ok(())
}

async fn on_callback_inner(
    bot: &Bot,
    q: &CallbackQuery,
    pool: &Pool,
    course: &Course,
) -> Result<()> {
    let tg_id = q.from.id.0 as i64;
    let chat = ChatId(tg_id);
    let data = q.data.clone().unwrap_or_default();

    if let Some(msg) = q.message.as_ref() {
        let (cid, mid) = match msg {
            MaybeInaccessibleMessage::Regular(m) => (m.chat.id, m.id),
            MaybeInaccessibleMessage::Inaccessible(im) => (im.chat.id, im.message_id),
        };
        let _ = bot.edit_message_reply_markup(cid, mid).await;
    }

    match data.as_str() {
        "crs:start" => handle_course_start(bot, chat, tg_id, pool, course, false).await,
        "crs:restart" => handle_course_start(bot, chat, tg_id, pool, course, true).await,
        d if d.starts_with("btn:") => handle_event_button(bot, chat, tg_id, pool, course, d).await,
        _ => {
            tracing::warn!("unknown callback data: {}", data);
            Ok(())
        }
    }
}

async fn handle_course_start(
    bot: &Bot,
    chat: ChatId,
    tg_id: i64,
    pool: &Pool,
    course: &Course,
    restart: bool,
) -> Result<()> {
    let registered = db::get_user(pool, tg_id)
        .await?
        .map(|u| u.is_registered())
        .unwrap_or(false);
    if !registered {
        bot.send_message(
            chat,
            "Пожалуйста, сначала зарегистрируйтесь: отправьте /start.",
        )
        .await?;
        return Ok(());
    }

    db::finish_all_active(pool, tg_id).await?;
    let session = db::start_session(pool, tg_id).await?;

    let intro = course
        .first()
        .ok_or_else(|| anyhow::anyhow!("course has no events"))?;
    let button = intro.buttons.first();
    let target = button.map(|b| b.event.as_str()).unwrap_or("");

    db::record_button(
        pool,
        tg_id,
        Some(session.id),
        &intro.id,
        button.map(|b| b.id.as_str()).unwrap_or("0"),
        if restart { "restart_course" } else { "start_course" },
        if target.is_empty() { None } else { Some(target) },
    )
    .await?;

    if let Some(next_idx) = course.next_after_button(0, target) {
        schedule_next(pool, &session, course, next_idx, Utc::now()).await?;
        if restart {
            bot.send_message(chat, "Курс начат заново. Скоро вы получите первое сообщение.")
                .await?;
        } else {
            bot.send_message(chat, "Курс начат. Скоро вы получите первое сообщение.")
                .await?;
        }
    } else {
        db::finish_session(pool, session.id).await?;
        bot.send_message(chat, "В курсе нет следующих сообщений после стартового.")
            .await?;
    }
    Ok(())
}

async fn handle_event_button(
    bot: &Bot,
    chat: ChatId,
    tg_id: i64,
    pool: &Pool,
    course: &Course,
    data: &str,
) -> Result<()> {
    let parts: Vec<&str> = data.splitn(3, ':').collect();
    if parts.len() != 3 {
        return Ok(());
    }
    let event_idx: usize = parts[1].parse().unwrap_or(usize::MAX);
    let btn_idx: usize = parts[2].parse().unwrap_or(usize::MAX);

    let Some(session) = db::active_session(pool, tg_id).await? else {
        bot.send_message(chat, "Нет активного курса. Нажмите /start, чтобы начать.")
            .await?;
        return Ok(());
    };

    let Some(event) = course.at(event_idx) else {
        tracing::warn!("callback references unknown event idx {}", event_idx);
        return Ok(());
    };
    let Some(button) = event.buttons.get(btn_idx) else {
        tracing::warn!(
            "callback references unknown button idx {} on event {}",
            btn_idx,
            event.id
        );
        return Ok(());
    };

    // Игнорируем повторное нажатие той же кнопки в течение 3 секунд
    // (двойной тап / повторная доставка callback Telegram).
    if db::recent_button_exists(pool, tg_id, &event.id, &button.id, 3).await? {
        tracing::info!(
            "duplicate press ignored: user {} event {} button {}",
            tg_id,
            event.id,
            button.id
        );
        return Ok(());
    }

    // URL-кнопки не генерируют callback — сюда попадают только сценарные.
    let target = button.event.as_str();
    db::record_button(
        pool,
        tg_id,
        Some(session.id),
        &event.id,
        &button.id,
        &button.action,
        if target.is_empty() { None } else { Some(target) },
    )
    .await?;

    let Some(next_idx) = course.next_after_button(event_idx, target) else {
        db::finish_session(pool, session.id).await?;
        bot.send_message(chat, "На этом всё! Благодарим вас и остаёмся на связи :)")
            .await?;
        return Ok(());
    };

    schedule_next(pool, &session, course, next_idx, Utc::now()).await?;
    Ok(())
}

async fn schedule_next(
    pool: &Pool,
    session: &db::Session,
    course: &Course,
    next_idx: usize,
    trigger_at: chrono::DateTime<Utc>,
) -> Result<()> {
    let next = course
        .at(next_idx)
        .ok_or_else(|| anyhow::anyhow!("invalid next_idx {}", next_idx))?;
    let send_after = trigger_at + Course::delay_of(next);
    let inserted = db::schedule(pool, session.id, session.telegram_id, &next.id, send_after).await?;
    if inserted {
        tracing::info!(
            "scheduled '{}' (idx {}) for user {} at {} (delay {} min {} s)",
            next.id,
            next_idx,
            session.telegram_id,
            send_after,
            next.delay_minutes,
            next.delay_seconds
        );
    } else {
        tracing::info!(
            "'{}' for user {} already scheduled — double press ignored",
            next.id,
            session.telegram_id
        );
    }
    Ok(())
}

pub async fn send_event_with_keyboard(
    bot: &Bot,
    chat: ChatId,
    course: &Course,
    event_idx: usize,
) -> Result<()> {
    let event = course
        .at(event_idx)
        .ok_or_else(|| anyhow::anyhow!("event idx {} not found", event_idx))?;

    let files: Vec<_> = event
        .files
        .iter()
        .filter_map(|f| course.resolve_file(f))
        .collect();

    let rows: Vec<Vec<InlineKeyboardButton>> = event
        .buttons
        .iter()
        .enumerate()
        .filter_map(|(i, b)| {
            if !b.url.is_empty() {
                // URL-кнопка (например, ссылка на Google-форму).
                match url::Url::parse(&b.url) {
                    Ok(u) => Some(vec![InlineKeyboardButton::url(b.text.clone(), u)]),
                    Err(e) => {
                        tracing::warn!("invalid url '{}' on button {}: {}", b.url, b.id, e);
                        None
                    }
                }
            } else {
                Some(vec![InlineKeyboardButton::callback(
                    b.text.clone(),
                    format!("btn:{}:{}", event_idx, i),
                )])
            }
        })
        .collect();

    let keyboard = if rows.is_empty() {
        None
    } else {
        Some(InlineKeyboardMarkup::new(rows))
    };
    media::send_event_message(bot, chat, &files, &event.text, keyboard).await;
    Ok(())
}

/// Админы: чьи telegram_id перечислены в ADMIN_IDS (через запятую). По умолчанию — Валерия.
fn is_admin(tg_id: i64) -> bool {
    let ids = std::env::var("ADMIN_IDS").unwrap_or_else(|_| "926310975".to_string());
    ids.split(',')
        .filter_map(|s| s.trim().parse::<i64>().ok())
        .any(|id| id == tg_id)
}

/// RFC3339 (UTC) -> "dd.mm HH:MM" по Москве.
fn msk(ts: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(ts)
        .map(|t| {
            t.with_timezone(&chrono::FixedOffset::east_opt(3 * 3600).unwrap())
                .format("%d.%m %H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| ts.chars().take(16).collect())
}

/// "day_12_practice" -> "День 12 · практика"; "week_2_survey" -> "Опрос недели 2" и т.п.
fn event_label(event_id: &str) -> String {
    static DAY_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"day_(\d+)").unwrap());
    static WEEK_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"week_(\d+)").unwrap());
    if let Some(c) = DAY_RE.captures(event_id) {
        let suffix = if event_id.contains("practice") {
            " · практика"
        } else if event_id.contains("outro") {
            " · выходное"
        } else {
            " · инфо"
        };
        return format!("День {}{}", &c[1], suffix);
    }
    if let Some(c) = WEEK_RE.captures(event_id) {
        return format!("Опрос недели {}", &c[1]);
    }
    match event_id {
        "start" => "Старт".to_string(),
        "posttest" => "Пост-тест".to_string(),
        other => other.to_string(),
    }
}

/// /stats — статистика по участникам (только для админов).
async fn handle_stats(bot: &Bot, chat: ChatId, tg_id: i64, pool: &Pool, course: &Course) -> Result<()> {
    if !is_admin(tg_id) {
        bot.send_message(chat, "Эта команда доступна только организаторам практикума.")
            .await?;
        return Ok(());
    }

    let users: Vec<(i64, String, String, Option<String>, Option<String>, String)> = sqlx::query_as(
        "SELECT telegram_id, phone, email, username, first_name, registered_at \
         FROM users ORDER BY registered_at",
    )
    .fetch_all(pool)
    .await?;

    let (visitor_count,): (i64,) = sqlx::query_as("SELECT COUNT(DISTINCT telegram_id) FROM visitors")
        .fetch_one(pool)
        .await?;
    let (unregistered,): (i64,) = sqlx::query_as(
        "SELECT COUNT(DISTINCT v.telegram_id) FROM visitors v \
         LEFT JOIN users u ON u.telegram_id = v.telegram_id \
         WHERE u.telegram_id IS NULL",
    )
    .fetch_one(pool)
    .await?;

    let mut out = format!(
        "📊 Статистика практикума\n\nВсего написали боту: {}\nЗарегистрировано: {}\nЗашли, но не зарегистрировались: {}\n",
        visitor_count,
        users.len(),
        unregistered
    );

    for (i, (uid, phone, email, username, first_name, registered_at)) in users.iter().enumerate() {
        let (sessions,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM course_sessions WHERE telegram_id = ?")
                .bind(uid)
                .fetch_one(pool)
                .await?;
        let (presses,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM button_events WHERE telegram_id = ?")
                .bind(uid)
                .fetch_one(pool)
                .await?;
        let last_press: Option<(String,)> = sqlx::query_as(
            "SELECT MAX(pressed_at) FROM button_events WHERE telegram_id = ?",
        )
        .bind(uid)
        .fetch_optional(pool)
        .await?;
        let cur_idx: Option<(i64,)> = sqlx::query_as(
            "SELECT current_index FROM course_sessions WHERE telegram_id = ? ORDER BY id DESC LIMIT 1",
        )
        .bind(uid)
        .fetch_optional(pool)
        .await?;
        let pending: Option<(String, String)> = sqlx::query_as(
            "SELECT message_id, send_after FROM scheduled_messages \
             WHERE telegram_id = ? AND sent_at IS NULL ORDER BY send_after LIMIT 1",
        )
        .bind(uid)
        .fetch_optional(pool)
        .await?;

        let name = first_name.clone().unwrap_or_else(|| "—".to_string());
        let uname = username
            .as_ref()
            .map(|u| format!("@{}", u))
            .unwrap_or_else(|| "—".to_string());
        let position = cur_idx
            .and_then(|(idx,)| course.at(idx as usize))
            .map(|e| event_label(&e.id))
            .unwrap_or_else(|| "ещё не начал курс".to_string());
        let pending_str = pending
            .map(|(mid, after)| format!("След. сообщение: {} в {}\n", event_label(&mid), msk(&after)))
            .unwrap_or_default();
        let last_str = last_press
            .map(|(ts,)| format!("Последняя активность: {}\n", msk(&ts)))
            .unwrap_or_default();

        out.push_str(&format!(
            "\n{}. {} ({})\n📱 {} · ✉️ {}\nРегистрация: {}\nСейчас: {}\n{}Сессий: {} · нажатий кнопок: {}\n{}",
            i + 1,
            name,
            uname,
            phone,
            email,
            msk(registered_at),
            position,
            pending_str,
            sessions,
            presses,
            last_str
        ));
    }

    if users.is_empty() {
        out.push_str("\nПока никто не зарегистрировался.");
    }

    // Telegram ограничивает сообщение 4096 символами — делим при необходимости.
    let mut rest = out.as_str();
    while !rest.is_empty() {
        let take = rest.char_indices().nth(4000).map(|(i, _)| i).unwrap_or(rest.len());
        bot.send_message(chat, &rest[..take]).await?;
        rest = &rest[take..];
    }
    Ok(())
}

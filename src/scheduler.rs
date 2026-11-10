use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use teloxide::prelude::*;
use tokio::time::sleep;

use crate::db::{self, Pool};
use crate::events::Course;
use crate::handlers;

// ТЕСТОВЫЙ РЕЖИМ: тик каждую секунду. Для продакшена достаточно 10–30 секунд.
const TICK_INTERVAL: Duration = Duration::from_secs(1);

pub async fn run(bot: Bot, pool: Pool, course: Arc<Course>) {
    tracing::info!("scheduler started, tick every {:?}", TICK_INTERVAL);
    loop {
        if let Err(e) = tick(&bot, &pool, &course).await {
            tracing::error!("scheduler tick error: {:?}", e);
        }
        sleep(TICK_INTERVAL).await;
    }
}

async fn tick(bot: &Bot, pool: &Pool, course: &Course) -> Result<()> {
    let due = db::due_messages(pool, Utc::now()).await?;
    for sm in due {
        if let Err(e) = deliver(bot, pool, course, &sm).await {
            tracing::error!(
                "failed to deliver message id={} user={}: {:?}",
                sm.id,
                sm.telegram_id,
                e
            );
        }
    }
    Ok(())
}

async fn deliver(
    bot: &Bot,
    pool: &Pool,
    course: &Course,
    sm: &db::ScheduledMessage,
) -> Result<()> {
    let Some(idx) = course.index_of(&sm.message_id) else {
        tracing::warn!(
            "scheduled message id '{}' not found in current yaml; skipping",
            sm.message_id
        );
        db::mark_sent(pool, sm.id).await?;
        return Ok(());
    };

    let chat = ChatId(sm.telegram_id);
    handlers::send_event_with_keyboard(bot, chat, course, idx).await?;
    db::mark_sent(pool, sm.id).await?;
    db::update_session_index(pool, sm.session_id, idx as i64).await?;

    let event = course.at(idx).expect("idx valid: just looked it up");
    // Событие без сценарных кнопок => цепочка продолжается автоматически.
    let has_scenario_buttons = event.buttons.iter().any(|b| b.url.is_empty());
    if !has_scenario_buttons {
        match course.next_in_order(idx) {
            Some(next_idx) => {
                let next = course.at(next_idx).expect("idx valid");
                let send_after = Utc::now() + Course::delay_of(next);
                let inserted =
                    db::schedule(pool, sm.session_id, sm.telegram_id, &next.id, send_after).await?;
                if inserted {
                    tracing::info!(
                        "auto-scheduled '{}' (idx {}) for user {} at {} (delay {} min {} s)",
                        next.id,
                        next_idx,
                        sm.telegram_id,
                        send_after,
                        next.delay_minutes,
                        next.delay_seconds
                    );
                }
            }
            None => {
                // Сценарий завершён: последнее сообщение уже отправлено,
                // ничего дополнительно не шлём.
                db::finish_session(pool, sm.session_id).await?;
            }
        }
    }
    Ok(())
}

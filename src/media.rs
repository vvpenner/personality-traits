use std::path::{Path, PathBuf};

use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardMarkup, InputFile, ParseMode};

const CAPTION_LIMIT: usize = 1024;

/// Отправляет событие: вложения прикреплены к сообщению через caption,
/// а не идут отдельно. Если текст длиннее лимита подписи Telegram (1024),
/// первая часть уходит подписью к первому вложению, остаток — текстовым
/// сообщением следом (клавиатура крепится к последнему сообщению).
pub async fn send_event_message(
    bot: &Bot,
    chat: ChatId,
    files: &[PathBuf],
    text: &str,
    keyboard: Option<InlineKeyboardMarkup>,
) {
    if files.is_empty() {
        send_text(bot, chat, text, keyboard).await;
        return;
    }

    let (caption, rest) = split_caption(text);

    // Первое вложение — с подписью; клавиатуру вешаем на него,
    // только если весь текст поместился в caption.
    let kb_for_first = if rest.is_empty() { keyboard.clone() } else { None };
    match send_one_with_caption(bot, chat, &files[0], &caption, kb_for_first).await {
        Ok(()) => {
            // Остальные вложения — без подписи.
            for path in &files[1..] {
                send_one(bot, chat, path).await;
            }
            // Не влезший в caption остаток текста — отдельным сообщением с клавиатурой.
            if !rest.is_empty() {
                send_text(bot, chat, &rest, keyboard).await;
            }
        }
        Err(e) => {
            tracing::warn!(
                "failed to send {} with caption: {}; falling back to separate messages",
                files[0].display(),
                e
            );
            // Фолбэк: текст целиком — отдельным сообщением, вложения — следом.
            send_text(bot, chat, text, keyboard).await;
            for path in files {
                send_one(bot, chat, path).await;
            }
        }
    }
}

/// Делит текст на caption (≤1024 символов) и остаток.
/// Разрыв — по последнему переводу строки внутри лимита, чтобы не резать абзац.
fn split_caption(text: &str) -> (String, String) {
    if text.chars().count() <= CAPTION_LIMIT {
        return (text.to_string(), String::new());
    }
    // Байтовая граница, не превышающая лимит по символам.
    let mut byte_idx = 0;
    for (count, (i, _)) in text.char_indices().enumerate() {
        if count >= 1000 {
            break;
        }
        byte_idx = i;
    }
    let cut = text[..byte_idx]
        .rfind('\n')
        .map(|p| p + 1)
        .filter(|&p| p > 200) // не делать слишком короткую подпись
        .unwrap_or(byte_idx);
    (text[..cut].trim_end().to_string(), text[cut..].trim().to_string())
}

async fn send_text(bot: &Bot, chat: ChatId, text: &str, keyboard: Option<InlineKeyboardMarkup>) {
    let text = if text.trim().is_empty() {
        "(пустое сообщение)"
    } else {
        text
    };
    let result = match keyboard.clone() {
        Some(kb) => bot
            .send_message(chat, text)
            .parse_mode(ParseMode::Html)
            .reply_markup(kb)
            .await
            .map(|_| ()),
        None => bot
            .send_message(chat, text)
            .parse_mode(ParseMode::Html)
            .await
            .map(|_| ()),
    };
    if let Err(e) = result {
        tracing::error!(
            "failed to send text to {}: {}; retrying without formatting",
            chat,
            e
        );
        // Фолбэк без разметки: убираем теги и отправляем plain text.
        let plain = strip_tags(text);
        let retry = match keyboard {
            Some(kb) => bot
                .send_message(chat, plain)
                .reply_markup(kb)
                .await
                .map(|_| ()),
            None => bot.send_message(chat, plain).await.map(|_| ()),
        };
        if let Err(e2) = retry {
            tracing::error!("failed to send plain text to {}: {}", chat, e2);
        }
    }
}

/// Убирает HTML-теги форматирования (для фолбэка без parse_mode).
fn strip_tags(s: &str) -> String {
    s.replace("<b>", "").replace("</b>", "")
}

async fn send_one_with_caption(
    bot: &Bot,
    chat: ChatId,
    path: &Path,
    caption: &str,
    keyboard: Option<InlineKeyboardMarkup>,
) -> Result<(), teloxide::RequestError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "jpg" | "jpeg" | "png" | "webp" => {
            let req = bot
                .send_photo(chat, InputFile::file(path))
                .caption(caption)
                .parse_mode(ParseMode::Html);
            match keyboard {
                Some(kb) => req.reply_markup(kb).await?,
                None => req.await?,
            };
        }
        "mp4" => {
            let req = bot
                .send_video(chat, InputFile::file(path))
                .caption(caption)
                .parse_mode(ParseMode::Html);
            match keyboard {
                Some(kb) => req.reply_markup(kb).await?,
                None => req.await?,
            };
        }
        "mp3" | "m4a" => {
            let req = bot
                .send_audio(chat, InputFile::file(path))
                .caption(caption)
                .parse_mode(ParseMode::Html);
            match keyboard {
                Some(kb) => req.reply_markup(kb).await?,
                None => req.await?,
            };
        }
        "ogg" => {
            let req = bot
                .send_voice(chat, InputFile::file(path))
                .caption(caption)
                .parse_mode(ParseMode::Html);
            match keyboard {
                Some(kb) => req.reply_markup(kb).await?,
                None => req.await?,
            };
        }
        _ => {
            let req = bot
                .send_document(chat, InputFile::file(path))
                .caption(caption)
                .parse_mode(ParseMode::Html);
            match keyboard {
                Some(kb) => req.reply_markup(kb).await?,
                None => req.await?,
            };
        }
    }
    Ok(())
}

async fn send_one(bot: &Bot, chat: ChatId, path: &Path) {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let result = match ext.as_str() {
        "jpg" | "jpeg" | "png" | "webp" => bot
            .send_photo(chat, InputFile::file(path))
            .await
            .map(|_| ()),
        "mp4" => bot
            .send_video(chat, InputFile::file(path))
            .await
            .map(|_| ()),
        "mp3" | "m4a" => bot
            .send_audio(chat, InputFile::file(path))
            .await
            .map(|_| ()),
        "ogg" => bot
            .send_voice(chat, InputFile::file(path))
            .await
            .map(|_| ()),
        _ => bot
            .send_document(chat, InputFile::file(path))
            .await
            .map(|_| ()),
    };

    if let Err(e) = result {
        tracing::warn!(
            "failed to send {} as {}: {}; retrying as document",
            path.display(),
            ext,
            e
        );
        if let Err(e2) = bot.send_document(chat, InputFile::file(path)).await {
            tracing::error!("failed to send {} as document: {}", path.display(), e2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::split_caption;

    #[test]
    fn short_text_fits_caption() {
        let (cap, rest) = split_caption("привет");
        assert_eq!(cap, "привет");
        assert!(rest.is_empty());
    }

    #[test]
    fn long_text_is_split() {
        let para = "а".repeat(600);
        let tail = "б".repeat(600);
        let text = format!("{}\n\n{}", para, tail);
        let (cap, rest) = split_caption(&text);
        assert!(cap.chars().count() <= 1024);
        assert_eq!(cap, para);
        assert_eq!(rest, tail);
    }
}

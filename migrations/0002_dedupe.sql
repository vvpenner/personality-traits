-- Защита от двойной отправки: одно и то же сообщение нельзя запланировать
-- дважды для одной сессии, пока оно не отправлено.
CREATE UNIQUE INDEX IF NOT EXISTS idx_unique_pending_msg
    ON scheduled_messages(session_id, message_id) WHERE sent_at IS NULL;

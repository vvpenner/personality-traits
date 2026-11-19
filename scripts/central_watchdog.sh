#!/bin/bash
# Watchdog «Центрального ключа»: личные алерты владельцу при падении/восстановлении.
# Cron: * * * * * /opt/central/watchdog.sh
TOKEN=$(grep '^TELOXIDE_TOKEN=' /opt/central/.env | cut -d= -f2-)
CHAT_ID=926310975
STATE=/var/lib/central-watchdog-state
LOG=/var/log/central-watchdog.log

ACTIVE=$(systemctl is-active central-bot 2>/dev/null)
PREV=$(cat "$STATE" 2>/dev/null)

send() {
  curl -s --max-time 10 "https://api.telegram.org/bot${TOKEN}/sendMessage" \
    -d chat_id="$CHAT_ID" --data-urlencode "text=$1" >/dev/null
}

NOW=$(date -u '+UTC %d.%m %H:%M')

if [ "$ACTIVE" != "active" ] && [ "$PREV" = "active" ]; then
  send "🔴 Бот «Центральный ключ» упал (статус: ${ACTIVE}). Сервер перезапускает его автоматически. ${NOW}
Если через пару минут не придёт 🟢 — напиши разработчику."
  echo "$NOW DOWN ($ACTIVE)" >> "$LOG"
fi

if [ "$ACTIVE" = "active" ] && [ -n "$PREV" ] && [ "$PREV" != "active" ]; then
  send "🟢 Бот «Центральный ключ» снова работает. ${NOW}"
  echo "$NOW UP" >> "$LOG"
fi

echo "$ACTIVE" > "$STATE"

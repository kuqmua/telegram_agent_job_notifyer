# Telegram Agent Job Notifyer

Workspace из двух крейтов:
- `server` — принимает `POST /notify` и отправляет текст в Telegram
- `client` — отправляет событие выполнения задачи на сервер

Цель: запустить `server` и `client` локально так, чтобы сообщение из клиента пришло в ваш Telegram-чат.

## 1. Требования

- Rust toolchain (в проекте используется nightly, см. `rust-toolchain.toml`)
- Доступ в интернет (сервер отправляет запросы в Telegram Bot API)
- Telegram бот и его токен (`TELEGRAM_BOT_TOKEN`)

## 2. Подготовка `.env`

```env
TELEGRAM_BOT_TOKEN=ваш_токен_бота
TELEGRAM_CHAT_ID=ваш_chat_id
HOST=127.0.0.1
PORT=8080
```

`TELEGRAM_CHAT_ID` обязателен: сервер без него не сможет отправлять сообщения.

## 3. Как получить `TELEGRAM_CHAT_ID` (подробно)

1. Откройте вашего бота в Telegram.
2. Отправьте ему любое сообщение (`/start` достаточно).
3. Выполните:

```bash
source .env
curl "https://api.telegram.org/bot${TELEGRAM_BOT_TOKEN}/deleteWebhook?drop_pending_updates=true"
curl "https://api.telegram.org/bot${TELEGRAM_BOT_TOKEN}/getUpdates"
```

4. В JSON-ответе найдите `message.chat.id` и запишите это число в `TELEGRAM_CHAT_ID`.

Если `getUpdates` вернул пустой `result`, отправьте боту сообщение еще раз и повторите запрос.

## 4. Запуск

Терминал 1:

```bash
cargo run -p server
```

Терминал 2:

```bash
cargo run -p client
```

После запуска клиента в Telegram приходит текст из поля `result`.

## 5. Проверки

Проверка сервера:

```bash
curl -i http://127.0.0.1:8080/health
```

Ручная отправка уведомления:

```bash
curl -X POST http://127.0.0.1:8080/notify \
  -H 'content-type: application/json' \
  -d '{
    "result":"hello from curl"
  }'
```

## 6. Частые проблемы

### `Status(503)` у клиента

Причина: не задан или неверен `TELEGRAM_CHAT_ID`.

Решение:
1. Убедиться, что `TELEGRAM_CHAT_ID` есть в `.env`.
2. Перезапустить `server`.

### `getUpdates` возвращает `409 Conflict`

Причина: ранее был установлен webhook.

Решение:

```bash
curl "https://api.telegram.org/bot${TELEGRAM_BOT_TOKEN}/deleteWebhook?drop_pending_updates=true"
```

### `Address already in use`

Причина: порт уже занят.

Решение: остановить процесс на порту `8080` или сменить `PORT` в `.env`.

## 7. codex_cli

В workspace добавлен библиотечный крейт `codex_cli` (обертка над `codex`).
Использование в проекте происходит из `client::notify` через `codex_cli::exec_prompt`.

Полная инструкция:
- [codex_cli/README.md](/home/kuqmua/Projects/telegram_agent_job_notifyer/codex_cli/README.md)

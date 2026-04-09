# Telegram Agent Job Notifyer

Workspace из двух крейтов:
- `server` — принимает `POST /notify` и отправляет текст в Telegram
- `client` — отправляет событие выполнения задачи на сервер

Цель: запустить `server` и `client` локально так, чтобы сообщение из клиента пришло в ваш Telegram-чат.

## 1. Требования

- Rust toolchain (в проекте используется nightly, см. `rust-toolchain.toml`)
- Доступ в интернет (сервер отправляет запросы в Telegram Bot API)
- Telegram бот и его токен (`TELEGRAM_BOT_TOKEN`)

## 2. Подготовка окружения

В корне проекта создайте/проверьте `.env`:

```env
TELEGRAM_BOT_TOKEN=ваш_токен_бота
HOST=127.0.0.1
PORT=8080
# Рекомендуется указать сразу (см. шаг 3)
TELEGRAM_CHAT_ID=123456789
```

Пояснения:
- `TELEGRAM_BOT_TOKEN` — обязателен
- `TELEGRAM_CHAT_ID` — если задан, сервер сразу умеет отправлять в ваш чат без webhook/туннеля

## 3. Получение `TELEGRAM_CHAT_ID` (без туннеля, рекомендуемый путь)

1. Откройте вашего бота в Telegram и отправьте любое сообщение (например `/start`).
2. Если webhook ранее был включен, отключите его:

```bash
source .env
curl "https://api.telegram.org/bot${TELEGRAM_BOT_TOKEN}/deleteWebhook?drop_pending_updates=true"
```

3. Получите апдейты:

```bash
curl "https://api.telegram.org/bot${TELEGRAM_BOT_TOKEN}/getUpdates"
```

4. Найдите в JSON поле `message.chat.id` и запишите его в `.env` как `TELEGRAM_CHAT_ID`.

Важно: если `result: []`, значит бот ещё не получил сообщение от вас. Отправьте сообщение и повторите `getUpdates`.

## 4. Запуск сервера

Из корня проекта:

```bash
cargo run -p server
```

Ожидаемо в логах:
- `Listening on 127.0.0.1:8080`
- `msg=chat_id_loaded_from_env chat_id=...`

Проверка health:

```bash
curl -i http://127.0.0.1:8080/health
```

Должно вернуть `HTTP/1.1 200 OK` и `OK`.

## 5. Запуск клиента и отправка сообщения

Во втором терминале, из корня проекта:

```bash
cargo run -p client
```

Если всё настроено, команда завершается без ошибок, а в Telegram приходит сообщение вида:
- `COMPLETED`
- `Agent: data-pipeline`
- `Status: completed`
- `Result: MEOW`

## 6. Проверка через API вручную (опционально)

Можно проверить без клиента:

```bash
curl -X POST http://127.0.0.1:8080/notify \
  -H 'content-type: application/json' \
  -d '{
    "agent_name":"manual-test",
    "status":"completed",
    "result":"hello from curl"
  }'
```

## 7. Частые проблемы

### Ошибка клиента `Status(503)`

Причина: сервер не знает, куда отправлять (`No registered chat`).

Решение:
- задать корректный `TELEGRAM_CHAT_ID` в `.env`
- перезапустить `server`

### `getUpdates` возвращает `409 Conflict`

Причина: активен webhook.

Решение:

```bash
curl "https://api.telegram.org/bot${TELEGRAM_BOT_TOKEN}/deleteWebhook?drop_pending_updates=true"
```

### `Address already in use` при запуске сервера

Причина: порт `8080` уже занят другим процессом.

Решение: остановить старый процесс или изменить `PORT` в `.env`.

## 8. Вариант с туннелем (если хотите webhook)

Этот вариант не обязателен, если у вас уже есть `TELEGRAM_CHAT_ID`.

1. Запустите сервер: `cargo run -p server`
2. Поднимите туннель:

```bash
cloudflared tunnel --url http://localhost:8080
```

3. Установите webhook:

```bash
curl "https://api.telegram.org/botYOUR_TOKEN/setWebHook?url=https://YOUR_TUNNEL_URL/webhook/telegram"
```

4. Отправьте сообщение боту — сервер зарегистрирует чат через `/webhook/telegram`.


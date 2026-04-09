# Telegram Agent Job Notifyer Server

Сервер для получения уведомлений о выполнении заданий и отправки их в Telegram.

## Установка

```bash
cargo build --release
```

## Переменные окружения

- `TELEGRAM_BOT_TOKEN` — токен бота Telegram (обязательно)
- `TELEGRAM_CHAT_ID` — id Telegram чата (опционально, если задан, webhook не нужен)
- `HOST` — хост для прослушивания (по умолчанию `0.0.0.0`)
- `PORT` — порт для прослушивания (по умолчанию `8080`)

## Запуск

```bash
export TELEGRAM_BOT_TOKEN="ваш_токен"
# Опционально: если знаете chat_id, укажите его
export TELEGRAM_CHAT_ID="123456789"
cargo run -p server
```

## Вариант A: Без туннеля (рекомендуется)

Если у вас уже есть `chat_id`, просто задайте `TELEGRAM_CHAT_ID` и запускайте сервер.

Как получить `chat_id` один раз:
1. Напишите вашему боту в Telegram
2. Откройте:
```bash
curl "https://api.telegram.org/botYOUR_TOKEN/getUpdates"
```
3. Возьмите `message.chat.id` из ответа и сохраните в `TELEGRAM_CHAT_ID`

После этого `POST /notify` будет работать без webhook и без туннеля.

## Вариант B: Через webhook и tunnel (Cloudflare)

Используйте этот вариант, если не хотите вручную искать `chat_id`.

### Шаг 1: Поднимите tunnel

```bash
cloudflared tunnel --url http://localhost:8080
```

Вы получите публичный URL, например:
```
https://a1b2c3d4.trycloudflare.com
```

### Шаг 2: Установите webhook

```bash
curl "https://api.telegram.org/botYOUR_TOKEN/setWebHook?url=https://YOUR_TUNNEL_URL/webhook/telegram"
```

Ожидаемый ответ:
```json
{"ok":true}
```

### Шаг 3: Зарегистрируйте чат

1. Откройте бота
2. Отправьте любое сообщение (например, `/start`)
3. Сервер сохранит `chat_id` в памяти и бот ответит `Chat registered`

После этого tunnel можно остановить, уведомления по `/notify` будут работать, пока сервер не перезапущен.

## Важно про перезапуск

Если `TELEGRAM_CHAT_ID` не задан, `chat_id` хранится только в памяти и после перезапуска сбрасывается.

Чтобы не настраивать webhook каждый раз, укажите `TELEGRAM_CHAT_ID` в `.env`.

## Эндпоинты

### `GET /health`
Проверка здоровья сервера.

**Ответ:** `OK`

### `POST /notify`
Отправка уведомления о выполнении задания.

**Тело запроса:**
```json
{
  "agent_name": "data-pipeline",
  "status": "completed",
  "result": "Processed 1500 records",
  "error": null,
  "elapsed_ms": 4250
}
```

### `POST /webhook/telegram`
Webhook для регистрации `chat_id`.

## Пример использования с клиентом

```bash
# В другом терминале
cargo run -p client
```

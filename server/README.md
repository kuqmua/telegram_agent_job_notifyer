# Telegram Agent Job Notifyer Server

Сервер принимает уведомления по `POST /notify` и отправляет текст в Telegram.

## Переменные окружения

- `TELEGRAM_BOT_TOKEN` — токен бота Telegram (обязательно)
- `TELEGRAM_CHAT_ID` — id чата, куда отправлять сообщения (обязательно)
- `HOST` — хост сервера (по умолчанию `0.0.0.0`)
- `PORT` — порт сервера (по умолчанию `8080`)

## Как получить `TELEGRAM_CHAT_ID` (подробно)

### Шаг 1. Напишите боту в Telegram

1. Откройте вашего бота в Telegram.
2. Отправьте ему любое сообщение, например `/start`.

Без этого шага `chat_id` не появится в апдейтах.

### Шаг 2. Отключите webhook (если был включен раньше)

`getUpdates` не работает при активном webhook и возвращает ошибку `409 Conflict`.

```bash
source .env
curl "https://api.telegram.org/bot${TELEGRAM_BOT_TOKEN}/deleteWebhook?drop_pending_updates=true"
```

### Шаг 3. Получите апдейты от Telegram

```bash
source .env
curl "https://api.telegram.org/bot${TELEGRAM_BOT_TOKEN}/getUpdates"
```

Пример ответа:

```json
{
  "ok": true,
  "result": [
    {
      "message": {
        "chat": {
          "id": 1709165228,
          "type": "private"
        },
        "text": "/start"
      }
    }
  ]
}
```

### Шаг 4. Возьмите значение `message.chat.id`

В примере выше это `1709165228`.

### Шаг 5. Запишите в `.env`

```env
TELEGRAM_BOT_TOKEN=ваш_токен
TELEGRAM_CHAT_ID=1709165228
HOST=127.0.0.1
PORT=8080
```

Если `result` пустой, отправьте боту сообщение еще раз и повторите `getUpdates`.

## Запуск сервера

```bash
cargo run -p server
```

Проверка:

```bash
curl -i http://127.0.0.1:8080/health
```

Ожидается `HTTP/1.1 200 OK` и `OK`.

## Эндпоинты

### `GET /health`
Проверка доступности сервера.

### `POST /notify`
Принимает payload от клиента и отправляет сообщение в Telegram.

Пример:

```json
{
  "result": "done",
  "error": null
}
```

## Пример с клиентом

В одном терминале:

```bash
cargo run -p server
```

Во втором терминале:

```bash
cargo run -p client
```

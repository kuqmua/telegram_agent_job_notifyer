# Telegram Agent Job Notifyer Server

Сервер для получения уведомлений о выполнении заданий и отправки их в Telegram.

## Установка

```bash
cargo build --release
```

## Переменные окружения

- `TELEGRAM_BOT_TOKEN` — токен бота Telegram (обязательно)
- `HOST` — хост для прослушивания (по умолчанию `0.0.0.0`)
- `PORT` — порт для прослушивания (по умолчанию `8080`)

## Запуск

```bash
export TELEGRAM_BOT_TOKEN="ваш_токен"
cargo run -p server
```

## Регистрация чата

Перед отправкой уведомлений нужно зарегистрировать ваш Telegram чат. Это делается **один раз** через вебхук.

### Шаг 1: Создайте бота

1. Найдите [@BotFather](https://t.me/BotFather) в Telegram
2. Отправьте `/newbot` и следуйте инструкциям
3. Сохраните полученный токен (вида `123456:ABC-DEF...`)

### Шаг 2: Запустите тунель (только для регистрации)

Telegram должен иметь доступ к вашему серверу для отправки вебхуков. Для локальной разработки используйте туннелирование:

**Вариант A: cloudflared**
```bash
# Установка (Linux)
wget https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64.deb
sudo dpkg -i cloudflared-linux-amd64.deb

# Запуск туннеля
cloudflared tunnel --url http://localhost:8080
```

**Вариант B: localhost.run** (без установки)
```bash
ssh -R 80:localhost:8080 localhost.run
```

После запуска вы увидите публичный URL, например:
```
https://a1b2c3d4.trycloudflare.com
```

### Шаг 3: Настройте вебхук

Замените `YOUR_TOKEN` и `YOUR_TUNNEL_URL` на реальные значения:

```bash
curl "https://api.telegram.org/botYOUR_TOKEN/setWebHook?url=https://YOUR_TUNNEL_URL/webhook/telegram"
```

Ожидаемый ответ:
```json
{"ok":true}
```

### Шаг 4: Зарегистрируйте чат

1. Откройте Telegram и найдите своего бота
2. Отправьте любое сообщение (например, `/start`)
3. Вы получите ответ: `Chat registered`

### Шаг 5: Закройте тунель

После регистрации чата тунель **больше не нужен** — он требовался только для вебхука Telegram.

Ваш chat_id теперь сохранён в памяти сервера, и уведомления будут приходить напрямую.

## Важно: Потеря chat_id

Chat_id хранится в **оперативной памяти** сервера. После перезапуска сервера он **сбрасывается**.

**Что делать при перезапуске:**
1. Снова запустите тунель
2. Снова настройте вебхук
3. Снова отправьте сообщение боту

**Решение:** Для сохранения chat_id между перезагрузками можно добавить персистентность (файл/БД).

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

**Поля:**
- `agent_name` — имя агента (обязательно)
- `status` — статус: `completed`, `failed`, `started` и т.д. (обязательно)
- `result` — результат выполнения (опционально)
- `error` — текст ошибки (опционально)
- `elapsed_ms` — время выполнения в мс (опционально)

**Ответ в Telegram:**
```
COMPLETED
Agent: data-pipeline
Status: completed
Result: Processed 1500 records
Time: 4250ms
```

### `POST /webhook/telegram`
Вебхук для получения сообщений от Telegram. Используется для регистрации chat_id.

**Не вызывается вручную** — Telegram отправляет запросы автоматически при настройке вебхука.

## Пример использования с клиентом

```bash
# В другом терминале
cargo run -p client
```

Клиент отправит уведомление на `http://localhost:8080/notify`, и сервер передаст его в ваш Telegram чат.

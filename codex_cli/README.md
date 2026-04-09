# codex_cli
CLI-обертка над `codex`/`codex-cli`.

## Что делает
- Проксирует команды в установленный `codex` или `codex-cli`.
- Если передан обычный текст без команды, запускает `codex exec "<текст>"`.
- Перед рабочими командами проверяет авторизацию через `codex login status`.

## Требования
- Rust nightly (см. `rust-toolchain.toml` в корне workspace).
- Установленный Codex CLI.

Установка Codex CLI:
```bash
npm i -g @openai/codex
```

Проверка:
```bash
codex --version
```

## Авторизация
```bash
codex login
```

Если `codex` не в `PATH`, задайте:
```bash
export CODEX_BIN=/полный/путь/к/codex
```

## Запуск

### 1) Прямой прокси-режим
```bash
cargo run -p codex_cli -- --version
cargo run -p codex_cli -- login status
cargo run -p codex_cli -- exec "explain this repo"
```

### 2) Prompt-режим
Любая строка без явной команды превращается в `codex exec`:
```bash
cargo run -p codex_cli -- сгенерируй простой html файл
```

Эквивалент:
```bash
codex exec "сгенерируй простой html файл"
```

## Частые проблемы
`codex binary not found`:
- Установите Codex CLI.
- Или задайте `CODEX_BIN=/полный/путь/к/codex`.

`codex authentication check failed`:
- Выполните `codex login` в том же окружении, где запускаете `cargo run -p codex_cli`.

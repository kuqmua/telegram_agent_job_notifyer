# codex_cli
Библиотека-обертка над `codex`/`codex-cli`.

## Что делает
- Ищет бинарь `codex` (`CODEX_BIN`, затем `codex`/`codex-cli`).
- Проверяет авторизацию через `codex login status`.
- Выполняет `codex exec "<prompt>"`.

## Публичный API

```rust
pub fn exec_prompt(prompt: &str) -> std::io::Result<()>;
```

## Использование

```rust
codex_cli::exec_prompt("explain this repo")?;
```

В этом проекте вызов выполняется из `client::notify`.

## Требования
- Установленный Codex CLI.
- Авторизация `codex login`.

Установка:
```bash
npm i -g @openai/codex
```

Проверка:
```bash
codex --version
```

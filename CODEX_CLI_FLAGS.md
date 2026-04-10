# Codex CLI flags (`codex-cli 0.118.0`)

Ниже собраны флаги, которые доступны в вашей локальной версии CLI.

## Важно

- `codex cli` как отдельной подкоманды нет.
- `codex cli --help` выводит тот же help, что и `codex --help`.

## Глобальные флаги `codex`

- `-c, --config <key=value>`: точечное переопределение значений из `~/.codex/config.toml`.
- `--enable <FEATURE>`: включить feature-флаг (повторяемый).
- `--disable <FEATURE>`: выключить feature-флаг (повторяемый).
- `--remote <ADDR>`: подключение TUI к удаленному app server (`ws://` или `wss://`).
- `--remote-auth-token-env <ENV_VAR>`: переменная окружения с bearer token для remote app server.
- `-i, --image <FILE>...`: приложить изображения к начальному промпту.
- `-m, --model <MODEL>`: выбрать модель.
- `--oss`: использовать локального open-source провайдера.
- `--local-provider <OSS_PROVIDER>`: выбрать локального провайдера (`lmstudio` или `ollama`).
- `-p, --profile <CONFIG_PROFILE>`: профиль из `config.toml`.
- `-s, --sandbox <SANDBOX_MODE>`: режим sandbox (`read-only`, `workspace-write`, `danger-full-access`).
- `-a, --ask-for-approval <APPROVAL_POLICY>`: политика подтверждений команд (`untrusted`, `on-failure`, `on-request`, `never`).
- `--full-auto`: shorthand для `-a on-request` + `--sandbox workspace-write`.
- `--dangerously-bypass-approvals-and-sandbox`: запуск без подтверждений и без sandbox (очень опасно).
- `-C, --cd <DIR>`: рабочая директория агента.
- `--search`: включить live web search tool.
- `--add-dir <DIR>`: дополнительные директории с правом записи.
- `--no-alt-screen`: отключить alternate screen mode в терминале.
- `-h, --help`: показать help.
- `-V, --version`: показать версию.

## Часто полезные флаги

- Безопасный рабочий режим: `-s workspace-write -a on-request`
- Автоматизация/скрипты: `codex exec --json --output-last-message <FILE> --color never`
- Эфемерный запуск без сохранения сессии: `codex exec --ephemeral`
- Запуск вне git-репозитория: `codex exec --skip-git-repo-check`
- Мультидиректории для монорепы: `--add-dir <DIR>`

## Подкоманда `codex exec`

- `--skip-git-repo-check`: разрешить запуск вне git-репозитория.
- `--ephemeral`: не сохранять сессию на диск.
- `--output-schema <FILE>`: JSON Schema для финального ответа модели.
- `--color <always|never|auto>`: режим цветного вывода.
- `--json`: выводить события JSONL в stdout.
- `-o, --output-last-message <FILE>`: записать последнее сообщение агента в файл.

## Подкоманда `codex review`

- `--uncommitted`: ревью staged/unstaged/untracked изменений.
- `--base <BRANCH>`: ревью относительно базовой ветки.
- `--commit <SHA>`: ревью изменений конкретного коммита.
- `--title <TITLE>`: заголовок в summary отчета.

## Подкоманда `codex resume`

- `--last`: продолжить последнюю сессию без picker.
- `--all`: показать все сессии (без фильтра по текущему cwd).
- `--include-non-interactive`: включить non-interactive сессии в picker и `--last`.

## Подкоманда `codex mcp`

### `codex mcp add`

- `codex mcp add <NAME> --url <URL>`: добавить streamable HTTP MCP сервер.
- `codex mcp add <NAME> -- <COMMAND>...`: добавить stdio MCP сервер через команду запуска.
- `--env <KEY=VALUE>`: переменные окружения для stdio MCP сервера.
- `--bearer-token-env-var <ENV_VAR>`: переменная с токеном для HTTP MCP сервера.

### `codex mcp list`

- `--json`: вывести конфигурацию серверов в JSON.

### `codex mcp login`

- `--scopes <SCOPE,SCOPE>`: OAuth scopes для аутентификации.

## Подкоманда `codex apply`

- `codex apply <TASK_ID>`: применить последний diff задачи в текущий git working tree.

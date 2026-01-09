# mcz-url-shortener

Простой URL-shortener на Rust (axum + sqlx + PostgreSQL) с базовой аналитикой кликов (event-based) через фонового воркера и очередь.

## Возможности

- `POST /shorten` принимает одну ссылку или массив ссылок и возвращает сокращённые.
- Нормализация + дедупликация: одинаковые URL после нормализации получают один и тот же `code`.
- `GET /{code}` делает редирект на исходную ссылку (307 Temporary Redirect) и увеличивает общий счётчик кликов.[^1]
- Каждое обращение к `GET /{code}` также пишет событие клика в очередь (in-memory) и фоновый воркер сохраняет его в таблицу `link_clicks` (referer / user-agent / ip).
- `GET /stats` возвращает список всех ссылок со статистикой + пагинация + `total`.
- `GET /stats/{code}` возвращает список кликов по конкретному коду из `link_clicks` с пагинацией + `total`.
- Ошибки API возвращаются в JSON-формате (`AppError`) с `code/message/details`.
- Access-log в стиле nginx (IP, метод, путь, статус, referer, user-agent, latency).


## Структура проекта (модули)

Примерная раскладка:

- `src/main.rs` — старт сервера, конфиг, сборка роутера.
- `src/state.rs` — `AppState` (PgPool, base_url, Sender для очереди кликов).
- `src/error.rs` — `AppError` (+ маппинг `sqlx::Error -> AppError`).
- `src/routes.rs` — сборка `Router` и маршрутов.
- `src/handlers/` — HTTP-хендлеры (`shorten`, `redirect`, `stats`, `stats_by_code`).
- `src/dto/` — структуры запросов/ответов (Serialize/Deserialize).
- `src/domain/` — доменная логика (ClickEvent, click worker).
- `src/utils/` — утилиты (нормализация URL, генерация кода, распознавание DB ошибок).
- `src/middlewares/` — access-log middleware.
- `migrations/` — SQL-миграции для PostgreSQL (sqlx-cli).
- `.sqlx/` — кэш запросов для offline-режима `sqlx::query!` (генерируется `cargo sqlx prepare`).


## Требования

- Rust toolchain (stable) + cargo
- PostgreSQL (локально) или Docker
- (опционально) `sqlx-cli` для миграций и `cargo sqlx prepare`


## Конфигурация (env)

- `DATABASE_URL` — строка подключения к Postgres.
- `BASE_URL` — базовый URL для формирования short_url (например `https://s.test.com/`).
- `LISTEN` — адрес/порт, который слушает сервер (например `0.0.0.0:3000`).
- `RUST_LOG` — уровень логов (например `info`).

Пример `.env`:

```env
DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5432/shorty
BASE_URL=https://s.test.com/
LISTEN=0.0.0.0:3000
RUST_LOG=info
```


## Локальный запуск (без Docker)

1) Установить sqlx-cli (один раз)
```bash
cargo install sqlx-cli --no-default-features --features postgres
```

2) Создать БД и применить миграции
```bash
sqlx database create
sqlx migrate run
```

3) Сгенерировать `.sqlx` для offline-сборок (рекомендуется)
```bash
unset SQLX_OFFLINE
cargo sqlx prepare -- --bin mcz-url-shortener
```

4) Запустить сервис
```bash
cargo run
```


## API

### Получить короткие ссылки

`POST /shorten`

Тело (JSON):

- либо строка URL
- либо массив строк URL

Пример (одна ссылка):

```bash
curl -s -X POST http://127.0.0.1:3000/shorten \
  -H 'content-type: application/json' \
  -d '"https://example.com"' | jq
```

Пример (массив):

```bash
curl -s -X POST http://127.0.0.1:3000/shorten \
  -H 'content-type: application/json' \
  -d '["https://example.com","https://example.org"]' | jq
```

Ответ:

```json
{
  "items": [
    {
      "long_url": "https://example.com",
      "code": "3c1930ac8e",
      "short_url": "https://s.test.com/3c1930ac8e"
    }
  ]
}
```


### Перейти по короткой ссылке

`GET /{code}`

Редирект на исходную ссылку и увеличение общего счётчика кликов (307 Temporary Redirect).[^1]

```bash
curl -i http://127.0.0.1:3000/3c1930ac8e
```


### Список всех ссылок (с пагинацией)

`GET /stats?page=1&page_size=25&from=2026-01-01T00:00:00Z&to=2026-01-09T23:59:59Z`

- `page` по умолчанию 1
- `page_size` по умолчанию 25, диапазон 10..50
- `total` — общее количество ссылок
- `from` — фильтр от даты (RFC3339)
- `to` — фильт по дату (RFC3339)

```bash
curl -s "http://127.0.0.1:3000/stats?page=1&page_size=25" \
  -H "Authorization: Bearer <token>" | jq
```

Пример ответа:

```json
{
  "page": 1,
  "page_size": 25,
  "total": 123,
  "items": [
    {
      "long_url": "https://example.com",
      "code": "3c1930ac8e",
      "clicks": 2,
      "created_at": "2026-01-09T09:57:14.919942Z"
    }
  ]
}
```


### Клики по коду (event-based, с пагинацией)

`GET /stats/{code}?page=1&page_size=25&from=2026-01-01T00:00:00Z&to=2026-01-09T23:59:59Z`

Возвращает события кликов из таблицы `link_clicks`:

- `total` — общее число кликов по коду
- `items` — список кликов (clicked_at/referer/user_agent/ip)
- `from` — фильтр от даты (RFC3339)
- `to` — фильт по дату (RFC3339)

```bash
curl -s "http://127.0.0.1:3000/stats/3c1930ac8e?page=1&page_size=25" \
  -H "Authorization: Bearer <token>" | jq
```

Пример ответа:

```json
{
  "code": "3c1930ac8e",
  "page": 1,
  "page_size": 25,
  "total": 2,
  "items": [
    {
      "id": 101,
      "clicked_at": "2026-01-09T10:01:00.000000Z",
      "referer": "https://news.ycombinator.com/",
      "user_agent": "curl/8.6.0",
      "ip": "203.0.113.10"
    }
  ]
}
```

## Аутентификация (Bearer token)

Статистические эндпоинты защищены долгоживущим токеном:

- `GET /stats`
- `GET /stats/{code}`

Токен передаётся заголовком:

`Authorization: Bearer <token>`


### Формат ошибок

Любая ошибка возвращается как JSON:

```json
{
  "error": {
    "code": "validation_error",
    "message": "page_size must be in [10..50]",
    "details": { "field": "page_size", "min": 10, "max": 50 }
  }
}
```

### Ошибки аутентификации
Если заголовок Authorization отсутствует:

```json
{
  "error": {
    "code": "unauthorized",
    "message": "Unauthorized",
    "details": { "reason": "Authorization header is missing" }
  }
}
```

Если заголовок есть, но токен неверный/отозван:

```json
{
  "error": {
    "code": "unauthorized",
    "message": "Unauthorized",
    "details": { "reason": "Bearer token is invalid" }
  }
}
```
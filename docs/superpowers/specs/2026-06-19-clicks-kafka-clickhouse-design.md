# Дизайн: клики на Kafka + ClickHouse (Фаза 1)

Дата: 2026-06-19
Статус: согласован, готов к плану реализации

## Контекст и цель

Сейчас клики пишутся асинхронно через in-process `mpsc`-канал → фоновый воркер
(`domain/click_worker.rs`) → PostgreSQL-таблица `link_clicks`. Чтение статистики —
SQL `JOIN` `links` с `link_clicks` в `PgStatsRepository`.

Цель Фазы 1 — перевести клики на **Kafka + ClickHouse**, полностью убрав хранение
кликов в PostgreSQL, и добавить **деградированный режим** для новых (некритичных)
зависимостей. Метаданные ссылок/доменов/токенов остаются в PostgreSQL.

Сравнение с эталонным Python-проектом (`mcz-url-shortener`) показало, что он уже
использует Kafka + ClickHouse, но через ClickHouse Kafka Engine. Здесь сознательно
выбран **Rust-консьюмер** (см. «Принятые решения»).

Это первая из трёх фаз. Не входят (отдельные спеки): массовая деактивация ссылок
(Фаза 2); Sentry / Elastic APM / Grafana dashboard (Фаза 3).

## Принятые решения

1. **Ингест Kafka→ClickHouse — собственный Rust-консьюмер** (не Kafka Engine).
   Причина: полный контроль над батчингом/ретраями/обработкой битых сообщений,
   наблюдаемость в тех же `metrics`/`tracing`, что и весь сервис, юнит-тестируемость.
   Цена: дополнительный код и зависимость `librdkafka` (C) в Docker.
2. **Ключ клика — `link_id`** (как `url_id` в Python). Redirect уже знает `link.id`;
   ре-резолв `domain+code → link_id` на каждый клик убирается. Консьюмер не обращается
   к PostgreSQL вообще.
3. **`link_id` зашивается в кэш-значение редиректа**, чтобы он был доступен и при
   cache HIT (когда линк из БД не грузится).
4. **Порты разделяются по сторонам CQRS**: `ClickPublisher` (write → Kafka) и
   `ClickStatsReader` (read → ClickHouse). `StatsService` склеивает метаданные из PG
   с агрегатами из ClickHouse.
5. **`get_all_stats` — merge в приложении**, а не SQL-`JOIN` (кросс-сторовый запрос).
6. **`rdkafka`** как Kafka-клиент (зрелые consumer groups + ручной commit офсетов).
   **`clickhouse`** (официальный crate) как клиент ClickHouse.
7. **Семантика доставки — at-least-once**: офсет коммитится только после успешного
   батч-инсерта. ClickHouse-таблица — обычный `MergeTree`; редкие дубли при сбоях
   допустимы (как в Python).
8. **Критичность для health**: критичен только PostgreSQL. Kafka, ClickHouse, Redis —
   некритичны (сервис продолжает обслуживать редиректы).

## Архитектура

### Поток данных

```
redirect ──► ClickPublisher (Kafka producer) ──► topic "clicks"
                                                       │
              click_consumer (rdkafka StreamConsumer)  │
                 батчинг по размеру/таймауту           │
                                                       │ batch INSERT (HTTP)
                                          ClickHouse MergeTree "clicks"
                                                       ▲
              StatsService (чтение) ──────────────────┘  ClickStatsReader
                       │
                       └─► PgLinkRepository / PgDomainRepository (метаданные ссылок)
```

### Удаляется

- `src/domain/click_worker.rs` (in-process воркер).
- `src/domain/click_event.rs` `mpsc`-семантика (структура переезжает, см. ниже).
- **`PgStatsRepository` целиком** и таблица `link_clicks` (миграция-дроп). Его
  не-кликовый метод `count_all_links` переезжает в `LinkRepository` (PG).
- Трейт `StatsRepository` заменяется на `ClickPublisher` + `ClickStatsReader`.
- Поле `click_sender: mpsc::Sender<ClickEvent>` в `AppState`.

### Добавляется

| Компонент | Файл | Назначение |
|---|---|---|
| Reconnecting Kafka producer | `infrastructure/messaging/kafka_producer.rs` | lazy-connect + cooldown, `publish(ClickEvent)` |
| Click consumer | `infrastructure/messaging/click_consumer.rs` | Kafka→ClickHouse батч-консьюмер как фоновая задача |
| Reconnecting ClickHouse client | `infrastructure/persistence/clickhouse_client.rs` | lazy-connect + cooldown, общий для reader и consumer |
| ClickHouse stats reader | `infrastructure/persistence/clickhouse_stats_reader.rs` | реализация `ClickStatsReader` |
| Порт publisher | `domain/repositories/click_publisher.rs` | трейт `ClickPublisher` |
| Порт reader | `domain/repositories/click_stats_reader.rs` | трейт `ClickStatsReader` |

## Детали компонентов

### `ClickEvent` (domain)

Добавить поле `link_id: i64`, вывести `serde::{Serialize, Deserialize}` для
сериализации в Kafka (JSON, формат совместим с тем, что читает консьюмер). Поля:
`link_id`, `ip`, `user_agent`, `referer`, `clicked_at` (проставляется в момент
редиректа, не в консьюмере). `domain`/`code` из события удаляются — больше не нужны.

### Порт `ClickPublisher` (write side)

```
#[async_trait]
trait ClickPublisher: Send + Sync {
    async fn publish(&self, event: ClickEvent) -> Result<(), AppError>;
}
```

Реализация — обёртка над reconnecting Kafka producer. `publish` неблокирующий по
смыслу: при недоступности Kafka логирует + инкрементит метрику и возвращает `Ok(())`
(клик дропается, редирект не страдает). Redirect-хендлер вызывает `publish` и
игнорирует «потерю» клика так же, как сейчас при переполнении очереди.

### Порт `ClickStatsReader` (read side)

```
#[async_trait]
trait ClickStatsReader: Send + Sync {
    async fn count_clicks(&self, link_id: i64, filter: &StatsFilter) -> Result<i64, AppError>;
    async fn list_clicks(&self, link_id: i64, filter: &StatsFilter) -> Result<Vec<Click>, AppError>;
    async fn counts_for_links(&self, link_ids: &[i64], filter: &StatsFilter)
        -> Result<HashMap<i64, i64>, AppError>;
}
```

При недоступности ClickHouse все методы возвращают `AppError::ServiceUnavailable`.

### `StatsService` (merge-логика)

- `get_stats_by_code(code, filter)`:
  1. PG: `PgLinkRepository` резолвит `code` (+ опционально `domain_id`) → `Link` (или `None`).
  2. ClickHouse: `count_clicks` + `list_clicks` по `link.id`.
  3. Собрать `DetailedStats { link, total, items }`.
- `get_all_stats(filter)`:
  1. PG: страница ссылок (`id, code, domain, long_url, created_at`) с пагинацией и
     фильтром по домену, `ORDER BY created_at DESC`.
  2. ClickHouse: `counts_for_links(<ids страницы>, filter)`.
  3. Merge в `Vec<LinkStats>`; ссылки без кликов → `total = 0`.
- `count_all_links()`: переезжает в `LinkRepository` (PG-подсчёт `links`).

Для шага 1 `get_all_stats` нужен метод листинга ссылок страницей в `LinkRepository`
(уже есть пагинация — переиспользуем её).

### Click consumer

Фоновая задача (`tokio::spawn`), `rdkafka` `StreamConsumer` с consumer group:

- Читает сообщения, десериализует JSON в `ClickEvent`.
- **Батчинг**: накапливает до `CLICK_BATCH_SIZE` либо до истечения
  `CLICK_BATCH_FLUSH_MS` (что раньше), затем один батч-INSERT в ClickHouse.
- **Commit офсетов вручную** после успешного инсерта (at-least-once).
- **Битое сообщение** (ошибка десериализации): залогировать + метрика
  `click_consumer_invalid_total`, пропустить (не блокировать партицию). DLQ в Фазе 1
  не делаем — только skip + метрика.
- **ClickHouse недоступен**: батч не коммитится, ретрай с backoff; сообщения будут
  перечитаны после восстановления (reconnecting client).
- **Graceful shutdown**: по сигналу завершения дослать накопленный батч и
  закоммитить, затем выйти. Координация с основным `shutdown_signal()` через
  `CancellationToken` или закрываемый канал.
- **Метрики**: `click_consumer_received_total`, `click_consumer_inserted_total`,
  `click_consumer_invalid_total`, `click_consumer_insert_failed_total`,
  `click_consumer_batch_size` (histogram), лаг по возможности.

### Reconnecting-клиенты

Обёртки в духе Python `ReconnectingKafkaProducer` / `ReconnectingClickHouseClient`:
ленивое подключение, при недоступности — cooldown перед повторной попыткой, метод
`get()` возвращает `Option<Client>`. Это позволяет сервису стартовать даже при
лежащих Kafka/ClickHouse и автоматически восстанавливаться без рестарта.

## Изменения данных и инфраструктуры

### ClickHouse-схема (`docker/clickhouse/init/01_schema.sql`)

```sql
CREATE DATABASE IF NOT EXISTS url_shortener;

CREATE TABLE IF NOT EXISTS url_shortener.clicks (
    link_id    UInt64,
    ip         Nullable(String),
    user_agent Nullable(String),
    referer    Nullable(String),
    clicked_at DateTime64(3, 'UTC')
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(clicked_at)
ORDER BY (link_id, clicked_at);
```

Kafka Engine и Materialized View **не создаются** — пишет Rust-консьюмер.

### PostgreSQL-миграция

Новая миграция `DROP TABLE link_clicks;` (необратимая запись кликов в PG убирается).
Существующие миграции не трогаем.

### `docker-compose.yml`

Добавить сервисы:
- `kafka` (KRaft-режим, без ZooKeeper; healthcheck по доступности брокера).
- `clickhouse` (`clickhouse-server`; init-схема монтируется в
  `/docker-entrypoint-initdb.d/`; healthcheck `SELECT 1`).
- `app` получает env-переменные новых зависимостей и зависит от них (`depends_on`
  с `condition: service_healthy`, при этом старт приложения не должен жёстко падать,
  если они недоступны — деградированный режим).

### Конфигурация (`config.rs` + `.env.example`)

| Переменная | Назначение | Дефолт |
|---|---|---|
| `KAFKA_BROKERS` | список брокеров | — |
| `KAFKA_CLICKS_TOPIC` | топик кликов | `clicks` |
| `KAFKA_CONSUMER_GROUP` | группа консьюмера | `url_shortener_clicks` |
| `CLICKHOUSE_URL` | HTTP-эндпоинт ClickHouse | — |
| `CLICKHOUSE_DATABASE` | БД | `url_shortener` |
| `CLICKHOUSE_USER` / `CLICKHOUSE_PASSWORD` | креды | — |
| `CLICK_BATCH_SIZE` | размер батча инсерта | 500 |
| `CLICK_BATCH_FLUSH_MS` | макс. задержка флаша | 1000 |

Старые `CLICK_QUEUE_CAPACITY` / `CLICK_WORKER_CONCURRENCY` удаляются.

### Сборка / Docker

`rdkafka` тянет `librdkafka` (C). В `Dockerfile` (release-сборка, Rust 1.96) добавить
системные зависимости: `cmake`, C-компилятор и (если не используем vendored-сборку)
заголовки. Предпочесть статическую/vendored-сборку librdkafka, чтобы рантайм-образ
не требовал системного пакета. Проверить совместимость с edition 2024 / MSRV 1.96.

## Деградированный режим и health

- **Критичная зависимость**: PostgreSQL. Падение → `/health` отдаёт **503**.
- **Некритичные**: Kafka, ClickHouse, Redis. Падение → `/health` отдаёт **200** со
  `status: "degraded"` и пер-компонентным статусом.
- Эндпоинты статистики при недоступном ClickHouse → **503**
  (`AppError::ServiceUnavailable`).
- Редиректы и создание ссылок работают при любой из некритичных зависимостей в дауне.
- В `AppError` добавить вариант `ServiceUnavailable` (HTTP 503), если его нет.
- `health.rs` переписать: проверки `database` (критично), `kafka`, `clickhouse`,
  `cache` (некритично); итоговый код 503 только при падении критичной. Проверку
  `click_queue` убрать (очереди больше нет).

## Изменения `AppState` / `server.rs`

- `AppState`: убрать `click_sender`; добавить `click_publisher: Arc<dyn ClickPublisher>`.
  `StatsService` пересобрать на `ClickStatsReader` + `LinkRepository`.
- `server.rs`: инициализировать reconnecting Kafka producer и ClickHouse client;
  заспавнить `click_consumer` вместо `run_click_worker`; пробросить shutdown-сигнал в
  консьюмер; на остановке — дождаться слива батча консьюмером.

## Тестирование

- **Unit**:
  - `StatsService` merge-логика с моками `ClickStatsReader` + `LinkRepository`
    (включая ссылки без кликов → 0).
  - Логика батчинга консьюмера (накопление по размеру/таймауту, skip битого
    сообщения) изолированно от реального Kafka/ClickHouse.
  - `ClickPublisher`: при «недоступном» продьюсере `publish` не возвращает ошибку
    наверх и инкрементит метрику.
  - Кодирование/декодирование кэш-значения с `link_id`.
- **Integration**:
  - health: degraded при некритичных в дауне (200), 503 при PG в дауне.
  - статистика: 503 при недоступном ClickHouse.
  - Полный e2e Kafka→ClickHouse — опционально через docker-compose, не в CI по
    умолчанию.

## Риски

- **Сборка `librdkafka` в Docker** — основной риск; смягчается vendored-сборкой и
  проверкой на MSRV 1.96 в начале реализации.
- **Кросс-сторовая согласованность** `get_all_stats` — счётчики из ClickHouse могут
  чуть отставать (лаг батча/Kafka). Приемлемо для аналитики, задокументировать.
- **Дубли кликов** при сбое между инсертом и commit офсета — редки, допустимы; при
  необходимости в будущем — `ReplacingMergeTree` + дедуп-ключ (вне Фазы 1).

## Следующие фазы (вне этого спека)

- **Фаза 2**: массовая деактивация ссылок (по ID и по коду в рамках домена).
- **Фаза 3**: Sentry (ошибки), Elastic APM (трейсинг), Grafana dashboard.

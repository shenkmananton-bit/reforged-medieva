# Reforged Medieval

Архитектурный клон Total War: Medieval II на Rust и Vulkan. Настольное
приложение для Windows и Linux.

- **Как запустить игру:** [docs/how-to-run.md](docs/how-to-run.md)
- **Вся документация:** [docs/README.md](docs/README.md)

## Структура репозитория

| Папка | Что внутри |
|---|---|
| `crates/rm_app` | Приложение: окно, Vulkan, главный цикл |
| `crates/rm_core` | Ядро симуляции (ECS) |
| `crates/rm_data` | Чтение текстовых файлов модов |
| `data/` | Игровые данные в текстовом виде |
| `docs/` | Документация на русском |
| `.github/workflows` | Автоматическая сборка (CI) |

Каждый крейт в `crates/` автоматически входит в общий workspace
(см. корневой `Cargo.toml`).

## Быстрая сборка из исходников

```
cargo run --release
```

Оригинальные ассеты Medieval II в репозиторий не кладутся: используются
собственные или временные модели и текстуры.

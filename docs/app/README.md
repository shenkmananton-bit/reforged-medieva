# Приложение (`crates/rm_app`)

Крейт `rm_app` — это сама программа, которую запускает игрок. Он открывает
окно, подключается к видеокарте через Vulkan, крутит главный цикл, рисует
битву и превращает мышь и клавиатуру в приказы. Правил боя здесь нет: они
живут в `rm_battle` и `rm_battle_ai` ([docs/battle](../battle/README.md)),
а приложение вызывает их с нужной частотой и показывает результат.

## Файлы

| Файл | Что делает | Подробнее |
|---|---|---|
| `src/main.rs` | Точка входа: параметры, журнал, запуск цикла | этот файл |
| `src/options.rs` | Разбор параметров командной строки | [how-to-run.md](../how-to-run.md) |
| `src/app.rs` | Окно, события, главный цикл | [main-loop.md](main-loop.md) |
| `src/timing.rs` | Фиксированный шаг симуляции, счётчик FPS | [main-loop.md](main-loop.md) |
| `src/game.rs` | Битва, ИИ, управление мышью, сборка кадра | [battle-ui.md](battle-ui.md) |
| `src/camera.rs` | Камера поля боя, лучи из курсора | [battle-ui.md](battle-ui.md) |
| `src/hud.rs`, `src/font.rs` | Интерфейс и пиксельный шрифт | [battle-ui.md](battle-ui.md) |
| `src/render/pipelines.rs` | Графические конвейеры и шейдеры | [render.md](render.md) |
| `src/render/mesh.rs` | Процедурные модели солдат, коней, стрел | [render.md](render.md) |
| `src/render/gpu.rs` | Буферы и буфер глубины | [render.md](render.md) |
| `shaders/` | Шейдеры GLSL и готовый SPIR-V | [render.md](render.md) |
| `src/render/context.rs` | Подключение к Vulkan и выбор видеокарты | [vulkan-init.md](vulkan-init.md) |
| `src/render/swapchain.rs` | Цепочка показа картинок в окне | [vulkan-init.md](vulkan-init.md) |
| `src/render/mod.rs` | Рисование одного кадра, ресайз | [frame.md](frame.md), [render.md](render.md) |
| `.github/workflows/ci.yml` | Автосборка для Windows и Linux | [ci.md](ci.md) |

## Как это работает одной картинкой

```
main.rs ──► App (app.rs)
             │
             ├─ окно winit ── события: ресайз, мышь, клавиши, «пора рисовать»
             │      └─ Game.handle_event ── выделение, приказы → Command
             │
             ├─ FixedTimestep (timing.rs) ── сколько тиков симуляции выполнить
             │      └─ Game.tick() × N   (20 раз в секунду × скорость игры)
             │            ├─ BattleAi.think ── приказы противника (rm_battle_ai)
             │            └─ Battle.step   ── шаг боя (rm_battle)
             │
             ├─ Game.build_frame ── экземпляры моделей + интерфейс
             │
             └─ Renderer (render/) ── кадр: земля, модели (instancing), интерфейс
                    ├─ VulkanContext: instance, видеокарта, очередь
                    └─ Swapchain: картинки окна, пересоздание при ресайзе
```

## Библиотеки

| Библиотека | Зачем |
|---|---|
| `winit` 0.30 | Окно и события на Windows и Linux |
| `ash` 0.38 | Прямой доступ к Vulkan из Rust, без прослоек |
| `ash-window` | Связывает окно winit с поверхностью Vulkan |
| `log`, `env_logger` | Журнал в консоль (`RUST_LOG=debug`) |
| `anyhow` | Понятные сообщения об ошибках с цепочкой причин |
| `glam` | Матрицы и векторы камеры (только для отображения, не для симуляции) |

`ash` подгружает системную библиотеку Vulkan при запуске, поэтому для
сборки Vulkan SDK не нужен.

## Что дальше

- Вынести обёртку Vulkan в `rm_render`, а рисование солдат в `rm_crowd`
  (как в [docs/adr/crate-map.md](../adr/crate-map.md)); сейчас для прототипа
  всё лежит в `rm_app`.
- Настоящие модели и анимации из атласов (`rm_assets`) вместо коробок.
- Камера кампании и heightmap глобальной карты.

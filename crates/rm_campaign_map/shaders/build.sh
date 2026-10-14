#!/bin/sh
# Пересобирает SPIR-V из GLSL. Нужен glslangValidator (пакет glslang-tools
# или Vulkan SDK). Готовые .spv лежат в репозитории, поэтому для обычной
# сборки игры этот скрипт не нужен. Подробно: docs/campaign-map/render.md.
set -e
cd "$(dirname "$0")"
for s in terrain.vert terrain.frag border_mask.frag border.vert border.frag; do
    glslangValidator -V --quiet "$s" -o "spv/$s.spv"
done

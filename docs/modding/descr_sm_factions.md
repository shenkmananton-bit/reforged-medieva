# `descr_sm_factions.txt` — фракции

Файл описывает фракции: их культуру, религию, цвета на карте и особые
возможности. Лежит в `data/descr_sm_factions.txt`.

## Пример

```
faction                     northmark
culture                     northern_european
religion                    catholic
symbol                      models_strat/symbol_northmark.cas
rebel_symbol                models_strat/symbol_rebels.cas
primary_colour              red 40, green 70, blue 160
secondary_colour            red 230, green 220, blue 200
loading_logo                loading_screen/symbols/symbol128_northmark.tga
standard_index              0
logon_logo                  menu/symbols/fe_logos/northmark.tga
custom_battle_availability  yes
can_sap                     no
prefers_naval_invasions     yes
can_have_princess           yes
has_family_tree             yes
```

Каждая фракция начинается со строки `faction`.

## Поля

| Поле | Обяз. | По умолчанию | Значение |
|---|---|---|---|
| `faction` | да | — | Внутреннее имя, одно слово. На него ссылается `ownership` у юнитов. Должно быть уникальным. |
| `culture` | да | — | Культура, одно слово (например, `northern_european`). |
| `religion` | да | — | Религия, одно слово (например, `catholic`). |
| `symbol` | нет | — | Модель герба на карте кампании. |
| `rebel_symbol` | нет | — | Модель герба мятежников этой фракции. |
| `primary_colour` | да | — | Основной цвет: `red 0-255, green 0-255, blue 0-255`, именно в таком порядке. |
| `secondary_colour` | да | — | Второй цвет, тот же формат. |
| `loading_logo` | нет | — | Картинка для экрана загрузки. |
| `standard_index` | нет | — | Номер знамени. |
| `logon_logo` | нет | — | Логотип в меню. |
| `custom_battle_availability` | нет | `yes` | Можно ли выбрать фракцию в своей битве. |
| `can_sap` | нет | `no` | Умеет ли делать подкопы при осаде. |
| `prefers_naval_invasions` | нет | `no` | Любит ли ИИ высаживаться с моря. |
| `can_have_princess` | нет | `no` | Есть ли принцессы. |
| `has_family_tree` | нет | `no` | Есть ли семейное древо. |

Да/нет пишутся только как `yes` / `no`.

Имя `slave` зарезервировано за мятежниками, как в оригинале.

Другие поля (например, `periods_unavailable_in_custom_battle` из оригинала)
загрузку не ломают: они сохраняются, а проверка выведет предупреждение.

## Дипломатические кластеры

В Reforged Medieval фракций больше 50, и ИИ оценивает дипломатию внутри
культурно-религиозных кластеров. Кластеры будут задаваться **не в этом файле**,
а в отдельном `descr_rm_faction_clusters.txt` (см.
[контракт моддинга](../adr/modding-contract.md)), чтобы формат
`descr_sm_factions.txt` остался как в оригинале.

## Частые ошибки

| Сообщение | Что значит |
|---|---|
| `цвет `primary_colour` записывается как `red 0, green 0, blue 0`` | Не три части через запятую. |
| `на месте 2 ожидается `green`, а написано `blue`` | Каналы перепутаны местами. |
| `канал red (0–255) должно быть числом, а написано `300`` | Значение больше 255. |
| `должно быть `yes` или `no`` | Написано `true`, `1`, `да` и т. п. |
| `фракция `X` уже объявлена на строке N` | Две фракции с одним именем. |

# Файлы кампании

Кампания собирается из шести файлов. Четыре повторяют формат Medieval II,
два новых (с префиксом `descr_rm_`) добавлены Reforged Medieval.

| Файл | Что в нём | Формат |
|---|---|---|
| `data/world/maps/base/descr_regions.txt` | Регионы: поселение, ресурсы, плодородие, религии | как в оригинале |
| `data/descr_sm_resources.txt` | Ресурсы и их торговая ценность | как в оригинале |
| `data/world/maps/campaign/imperial_campaign/descr_strat.txt` | Стартовая расстановка: кто чем владеет, деньги, армии, даты | подмножество оригинала |
| `data/descr_rm_campaign.txt` | Числа экономики, роста, движения и автобоя; уровни поселений | **новый** |
| `data/world/maps/base/descr_rm_regions.txt` | Координаты поселений и соседство регионов | **новый** |
| `data/export_descr_unit.txt`, `data/descr_sm_factions.txt` | Юниты и фракции | см. [юниты](export_descr_unit.md), [фракции](descr_sm_factions.md) |

Проверка всех файлов разом: `cargo run -p rm_data --bin rm_modcheck -- data`.
Как правила используют эти числа — в [docs/campaign/turn.md](../campaign/turn.md).

## descr_regions.txt

У региона нет имён полей: это **9 строк подряд**, смысл строки задаётся её
номером. Последняя строка `religions { … }` закрывает регион.

```text
Ostmark                          ; 1. имя региона
    Ostburg                      ; 2. имя поселения
    northmark                    ; 3. фракция-основатель (культура построек)
    Northern_Rebels              ; 4. тип мятежников
    40 70 160                    ; 5. цвет на карте регионов, R G B
    iron, furs                   ; 6. ресурсы через запятую или none
    5                            ; 7. очки триумфа
    3                            ; 8. плодородие (доход с ферм и рост)
    religions { catholic 90 pagan 10 }   ; 9. религии и доли в процентах
```

## descr_sm_resources.txt

```text
type            iron            ; имя, на него ссылаются регионы
trade_value     40              ; денариев за ход региону с этим ресурсом
item            models_strat/resource_iron.cas
icon            ui/resources/resource_iron.tga
has_mine                        ; ресурс добывается шахтой (пока не влияет)
```

## descr_strat.txt

Порядок фракций в файле — **порядок их ходов**.

```text
campaign        imperial_campaign
playable                        ; за кого можно играть
    northmark
end
unlockable                      ; откроются позже
    steppe_horde
end
nonplayable                     ; только ИИ
    slave
end
start_date      1080 summer     ; год и время года (summer / winter)
end_date        1230 winter
timescale       2.00            ; лет за ход

faction         northmark, comfortable caution   ; имя, характер ИИ
ai_label        catholic
denari          4000            ; казна на старте
denari_kings_purse  1200        ; доход каждый ход сверху

settlement
{
    level           town        ; уровень из descr_rm_campaign.txt
    region          Ostmark
    year_founded    0
    population      2400
    plan_set        default_set
    faction_creator northmark
    building
    {
        type core_building wooden_pallisade
    }
}

character   Aldric, named character, male, leader, age 41, x 20, y 60
army
unit        Shield Wall Spearmen        exp 1 armour 0 weapon_lvl 0
```

- Персонаж попадает в регион, чьё поселение ближе всего к его клетке `x, y`.
- Отряды (`unit`) идут после `army` и принадлежат последнему персонажу.
  Имя отряда — как в `type` из `export_descr_unit.txt`; оно может состоять
  из нескольких слов.
- Персонаж без отрядов пока игнорируется (шпионы, торговцы — позже).
- Строки, которые игра пока не понимает (`traits`, `ancillaries`,
  `resource`, `core_attitudes`, `faction_relationships`…), можно оставлять:
  они сохраняются и не мешают.
- Мятежники — фракция `slave`, как в оригинале.

## descr_rm_campaign.txt (новый)

Каждое число обязательно; если чего-то не хватает, проверка скажет, чего.

```text
tax_per_100_population          3     ; налог с каждых 100 жителей за ход
farm_income_per_level           30    ; доход с ферм за уровень плодородия
growth_base_permille            5     ; прирост населения за ход, тысячные (5 = 0,5%)
growth_per_farm_level_permille  2     ; добавка за уровень плодородия
army_moves_per_turn             1     ; переходов между регионами за ход
cavalry_army_moves_per_turn     2     ; то же для армии только из конницы
recruit_slots_per_turn          1     ; сколько отрядов поселение учит одновременно
autoresolve_spread_percent      20    ; удача в автобое, ± проценты (не больше 99)
autoresolve_winner_loss_percent 50    ; потери победителя от соотношения сил

settlement_level    village,    2000  ; уровень, предел населения
settlement_level    town,       6000
```

Уровни перечисляются от меньшего к большему.

## descr_rm_regions.txt (новый)

В Medieval II соседство регионов вычислялось из картинки `map_regions.tga`.
Пока её разбора нет, оно задаётся здесь:

```text
region      Ostmark                         ; имя из descr_regions.txt
position    20, 60                          ; клетка поселения на карте
neighbours  Westfold, Hollowmere, Kingsford ; куда армия может перейти
```

Соседство должно быть **взаимным**: если у `Ostmark` в соседях `Westfold`,
то и у `Westfold` в соседях должен быть `Ostmark`. Проверка подскажет,
где забыли.

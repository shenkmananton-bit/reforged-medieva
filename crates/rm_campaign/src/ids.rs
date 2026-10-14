//! Короткие номера фракций, регионов и типов юнитов.
//!
//! Внутри кампании объекты ссылаются друг на друга не по имени, а по номеру:
//! так компоненты маленькие, сравнение мгновенное, а хеш состояния не
//! зависит от длины строк. Номер — это позиция в соответствующей таблице
//! ([`Campaign::factions`](crate::Campaign::factions),
//! [`CampaignMap::regions`](crate::CampaignMap::regions),
//! [`Campaign::unit_types`](crate::Campaign::unit_types)).

/// Номер фракции: позиция в `descr_strat.txt`, она же очередь хода.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FactionId(pub u16);

/// Номер региона: позиция в `descr_regions.txt`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RegionId(pub u16);

/// Номер типа юнита: позиция в `export_descr_unit.txt`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UnitTypeId(pub u16);

impl FactionId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl RegionId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl UnitTypeId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

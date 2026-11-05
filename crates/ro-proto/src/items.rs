//! Inventory, floor items, equipment, stats and skills at PACKETVER 20250402
//! (layouts from rAthena's preprocessed packets.hpp / packets_struct.hpp).

use bytes::{Buf, BufMut, BytesMut};

use crate::{
    codec::Encode,
    packets::{get_fixed_str, put_db_packet},
};

/// An inventory entry from the item list / pickup packets. `index` is the wire index.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemData {
    pub index: u16,
    pub id: u32,
    pub kind: u8,
    pub amount: u16,
    pub location: u32,
    pub equipped: u32,
    pub refine: u8,
    /// Enchant grade: 0 none, 1 D, 2 C, 3 B, 4 A.
    pub grade: u8,
    pub identified: bool,
    pub cards: [u32; 4],
    pub look: u16,
    /// In the "etc" (favourites) tab: the PlaceETCTab flag.
    pub favorite: bool,
}

fn cards(b: &mut &[u8]) -> [u32; 4] {
    [b.get_u32_le(), b.get_u32_le(), b.get_u32_le(), b.get_u32_le()]
}

/// packet_itemlist_normal (0x0b09): NORMALITEM_INFO entries, 34 bytes each.
pub fn parse_itemlist_normal(data: &[u8]) -> Vec<ItemData> {
    data.get(5..)
        .unwrap_or_default()
        .chunks_exact(34)
        .map(|mut b| {
            let index = b.get_u16_le();
            let id = b.get_u32_le();
            let kind = b.get_u8();
            let amount = b.get_i16_le().max(0) as u16;
            let equipped = b.get_u32_le();
            let cards = cards(&mut b);
            b.advance(4); // hire expire date
            let flags = b.get_u8();
            // Cards carry the slots they fit in where other items say what they occupy;
            // a card is never worn.
            let (location, equipped) = if kind == 6 { (0, 0) } else { (0, equipped) };
            ItemData { index, id, kind, amount, location, equipped, refine: 0, grade: 0, identified: flags & 1 != 0, cards, look: 0, favorite: flags & 2 != 0 }
        })
        .collect()
}

/// packet_itemlist_equip (0x0b39): EQUIPITEM_INFO entries, 68 bytes each.
pub fn parse_itemlist_equip(data: &[u8]) -> Vec<ItemData> {
    data.get(5..)
        .unwrap_or_default()
        .chunks_exact(68)
        .map(|mut b| {
            let index = b.get_u16_le();
            let id = b.get_u32_le();
            let kind = b.get_u8();
            let location = b.get_u32_le();
            let equipped = b.get_u32_le();
            let cards = cards(&mut b);
            b.advance(4 + 2); // hire expire date, bind on equip
            let look = b.get_u16_le();
            b.advance(1 + 25); // option count, options
            let refine = b.get_u8();
            let grade = b.get_u8();
            let flags = b.get_u8();
            ItemData { index, id, kind, amount: 1, location, equipped, refine, grade, identified: flags & 1 != 0, cards, look, favorite: flags & 4 != 0 }
        })
        .collect()
}

/// ZC_ITEM_PICKUP_ACK (0x0b41): the item added to the inventory, or None when the pickup
/// failed (overweight, no room...).
pub fn parse_pickup(data: &[u8]) -> Option<ItemData> {
    let mut b = &data[2..];
    let index = b.get_u16_le();
    let amount = b.get_u16_le();
    let id = b.get_u32_le();
    let identified = b.get_u8() != 0;
    b.advance(1); // damaged
    let cards = cards(&mut b);
    let location = b.get_u32_le();
    let kind = b.get_u8();
    let result = b.get_u8();
    b.advance(4 + 2 + 25); // hire date, bind, options
    // Set when the item went into the favourites tab (taken from storage with Shift+right-click).
    let favorite = b.get_u8() != 0;
    let look = b.get_u16_le();
    let refine = b.get_u8();
    let grade = b.get_u8();
    (result == 0).then_some(ItemData { index, id, kind, amount, location, equipped: 0, refine, grade, identified, cards, look, favorite })
}

/// An item lying on the ground.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FloorItem {
    pub id: u32,
    pub item: u32,
    pub identified: bool,
    pub x: u16,
    pub y: u16,
    pub amount: u16,
    /// Sub-cell offset (0..=11, twelfths of a cell).
    pub sub: (u8, u8),
    /// The light pillar it falls with (ZC_ITEM_FALL_ENTRY5): 0 the client's own (pink, for
    /// cards), then blue, yellow, purple, green, red. None: no pillar.
    pub drop_effect: Option<u16>,
}

/// packet_dropflooritem (0x0add, dropped now) / ZC_ITEM_ENTRY (0x009d, entering view).
pub fn parse_floor_item(opcode: u16, data: &[u8]) -> Option<FloorItem> {
    let mut b = &data[2..];
    Some(match opcode {
        0x0add => {
            let id = b.get_u32_le();
            let item = b.get_u32_le();
            b.advance(2); // type
            let identified = b.get_u8() != 0;
            let (x, y) = (b.get_i16_le() as u16, b.get_i16_le() as u16);
            let sub = (b.get_u8(), b.get_u8());
            let amount = b.get_i16_le() as u16;
            let drop_effect = (b.remaining() >= 3 && b.get_u8() != 0).then(|| b.get_u16_le());
            FloorItem { id, item, identified, x, y, amount, sub, drop_effect }
        }
        0x009d => {
            let id = b.get_u32_le();
            let item = b.get_u32_le();
            let identified = b.get_u8() != 0;
            let (x, y) = (b.get_u16_le(), b.get_u16_le());
            let amount = b.get_u16_le();
            let sub = (b.get_u8(), b.get_u8());
            FloorItem { id, item, identified, x, y, amount, sub, drop_effect: None }
        }
        _ => return None,
    })
}

/// ZC_ITEM_DISAPPEAR (0x00a1): floor item id.
pub fn parse_item_disappear(data: &[u8]) -> u32 {
    (&data[2..]).get_u32_le()
}

/// ZC_DELETE_ITEM_FROM_BODY (0x07fa) / ZC_ITEM_THROW_ACK (0x00af): (index, amount removed).
pub fn parse_item_removed(opcode: u16, data: &[u8]) -> Option<(u16, u16)> {
    let mut b = &data[2..];
    match opcode {
        0x07fa => {
            b.advance(2);
            Some((b.get_u16_le(), b.get_i16_le().max(0) as u16))
        }
        0x00af => Some((b.get_u16_le(), b.get_u16_le())),
        _ => None,
    }
}

/// ZC_USE_ITEM_ACK (0x01c8): (index, remaining amount, user id, success).
pub fn parse_use_item_ack(data: &[u8]) -> (u16, u16, u32, bool) {
    let mut b = &data[2..];
    let index = b.get_u16_le();
    b.advance(4); // item id
    let aid = b.get_u32_le();
    let amount = b.get_i16_le().max(0) as u16;
    (index, amount, aid, b.get_u8() != 0)
}

/// ZC_REQ_WEAR_EQUIP_ACK (0x0999): (index, worn slots, view sprite, ok).
pub fn parse_wear_ack(data: &[u8]) -> (u16, u32, u16, bool) {
    let mut b = &data[2..];
    (b.get_u16_le(), b.get_u32_le(), b.get_u16_le(), b.get_u8() == 0)
}

/// ZC_REQ_TAKEOFF_EQUIP_ACK (0x099a): (index, slots it left, ok).
pub fn parse_takeoff_ack(data: &[u8]) -> (u16, u32, bool) {
    let mut b = &data[2..];
    (b.get_u16_le(), b.get_u32_le(), b.get_u8() == 0)
}

/// Item actions the client asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum ItemRequest {
    Use { index: u16, account_id: u32 },
    /// CZ_REQ_ONECLICK_ITEMIDENTIFY (0x0a35): identify this item with a Magnifier we carry.
    Identify { index: u16 },
    Equip { index: u16, location: u32 },
    Unequip { index: u16 },
    /// CZ_REQ_TAKEOFF_EQUIP_ALL: take off everything worn in these slots.
    UnequipAll { location: u32 },
    /// CZ_INVENTORY_TAB: move to (on) or out of the favourites tab.
    Favorite { index: u16, on: bool },
    /// CZ_USE_PACKAGEITEM: open a selectable package, taking its set `set` (the client
    /// table's box index, rAthena's item_packages.yml group).
    UsePackage { index: u16, account_id: u32, item: u32, set: u32 },
    /// Inventory expansion: ask (0x0b14), confirm (0x0b16) or cancel (0x0b19).
    ExpandAsk,
    ExpandConfirm(bool),
    PickUp { floor_id: u32 },
    Drop { index: u16, amount: u16 },
    /// CZ_REQ_ITEMCOMPOSITION_LIST: which equipment can take this card?
    CardTargets { card: u16 },
    /// CZ_REQ_ITEMCOMPOSITION: put the card into the equipment.
    InsertCard { card: u16, equip: u16 },
}

impl Encode for ItemRequest {
    fn encode(&self, d: &mut BytesMut) {
        match *self {
            ItemRequest::Use { index, account_id } => put_db_packet(d, "UseItem", &[&index.to_le_bytes(), &account_id.to_le_bytes()]),
            ItemRequest::Equip { index, location } => {
                d.put_u16_le(0x0998); // CZ_REQ_WEAR_EQUIP
                d.put_u16_le(index);
                d.put_u32_le(location);
            }
            ItemRequest::Unequip { index } => put_db_packet(d, "UnequipItem", &[&index.to_le_bytes()]),
            ItemRequest::UsePackage { index, account_id, item, set } => {
                d.put_u16_le(crate::client_packet("itempackage_select").opcode);
                d.put_u16_le(index);
                d.put_u32_le(account_id);
                d.put_u32_le(item);
                d.put_u32_le(set);
            }
            ItemRequest::ExpandAsk => d.put_u16_le(0x0b14),
            ItemRequest::ExpandConfirm(yes) => d.put_u16_le(if yes { 0x0b16 } else { 0x0b19 }),
            ItemRequest::Favorite { index, on } => {
                d.put_u16_le(0x0907);
                d.put_u16_le(index);
                // The wire flag is inverted: false asks for the favourites tab.
                d.put_u8(!on as u8);
            }
            ItemRequest::UnequipAll { location } => {
                d.put_u16_le(0x0bf5);
                d.put_u32_le(location);
            }
            ItemRequest::PickUp { floor_id } => put_db_packet(d, "TakeItem", &[&floor_id.to_le_bytes()]),
            ItemRequest::Drop { index, amount } => put_db_packet(d, "DropItem", &[&index.to_le_bytes(), &amount.to_le_bytes()]),
            ItemRequest::Identify { index } => put_db_packet(d, "Oneclick_Itemidentify", &[&index.to_le_bytes()]),
            ItemRequest::CardTargets { card } => {
                d.put_u16_le(0x017a);
                d.put_u16_le(card);
            }
            ItemRequest::InsertCard { card, equip } => {
                d.put_u16_le(0x017c);
                d.put_u16_le(card);
                d.put_u16_le(equip);
            }
        }
    }
}

/// ZC_STATUS (0x00bd): base stats, their raise costs, and derived values.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Stats {
    pub points: u32,
    /// str, agi, vit, int, dex, luk
    pub base: [u16; 6],
    /// Status points needed to raise each by one.
    pub cost: [u8; 6],
    /// Bonuses on str..luk from equipment and buffs.
    pub plus: [i16; 6],
    pub atk: (i16, i16),
    /// (min, max)
    pub matk: (i16, i16),
    pub def: (i16, i16),
    pub mdef: (i16, i16),
    pub hit: i16,
    pub flee: (i16, i16),
    pub crit: i16,
    pub aspd: (i16, i16),
    /// 4th-job trait stats pow, sta, wis, spl, con, crt (SP_POW..SP_CRT), their bonuses, the
    /// trait points each raise costs (SP_UPOW..SP_UCRT) and the points left (SP_TRAITPOINT).
    pub traits: [u16; 6],
    pub trait_plus: [i16; 6],
    pub trait_cost: [u8; 6],
    pub trait_points: u32,
    /// P.ATK, S.MATK, RES, MRES, H.PLUS, C.RATE (SP_PATK..SP_CRATE).
    pub advanced: [i16; 6],
}

pub fn parse_status(data: &[u8]) -> Stats {
    let mut b = &data[2..];
    let points = b.get_u16_le() as u32;
    let (mut base, mut cost) = ([0; 6], [0; 6]);
    for i in 0..6 {
        base[i] = b.get_u8() as u16;
        cost[i] = b.get_u8();
    }
    let atk = (b.get_i16_le(), b.get_i16_le());
    let (max_matk, min_matk) = (b.get_i16_le(), b.get_i16_le());
    let def = (b.get_i16_le(), b.get_i16_le());
    let mdef = (b.get_i16_le(), b.get_i16_le());
    let hit = b.get_i16_le();
    let flee = (b.get_i16_le(), b.get_i16_le());
    let crit = b.get_i16_le();
    let aspd = (b.get_i16_le(), b.get_i16_le());
    Stats { points, base, cost, plus: [0; 6], atk, matk: (min_matk, max_matk), def, mdef, hit, flee, crit, aspd, ..Default::default() }
}

/// ZC_STATUS_CHANGE_ACK (0x00bc): (stat id, accepted, new value).
pub fn parse_status_change_ack(data: &[u8]) -> (u16, bool, u8) {
    let mut b = &data[2..];
    (b.get_u16_le(), b.get_u8() != 0, b.get_u8())
}

/// ZC_STATUS_CHANGE (0x00be): (stat id, cost to raise).
pub fn parse_status_cost(data: &[u8]) -> (u16, u8) {
    let mut b = &data[2..];
    (b.get_u16_le(), b.get_u8())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Skill {
    pub id: u16,
    /// Targeting flags: 0 passive, 1 enemy, 2 place, 4 self, 16 friend, 32 trap.
    pub target: i32,
    pub level: u16,
    pub sp: u16,
    pub range: u16,
    pub name: String,
    pub upgradable: bool,
}

/// ZC_SKILLINFO_LIST (0x010f): SKILLDATA entries, 37 bytes each.
pub fn parse_skill_list(data: &[u8]) -> Vec<Skill> {
    data.get(4..)
        .unwrap_or_default()
        .chunks_exact(37)
        .map(|mut b| Skill {
            id: b.get_u16_le(),
            target: b.get_i32_le(),
            level: b.get_u16_le(),
            sp: b.get_u16_le(),
            range: b.get_u16_le(),
            name: get_fixed_str(&mut b, 24),
            upgradable: b.get_u8() != 0,
        })
        .collect()
}

/// ZC_SKILLINFO_UPDATE (0x010e): (skill, level, sp, range, upgradable).
pub fn parse_skill_update(data: &[u8]) -> (u16, u16, u16, u16, bool) {
    let mut b = &data[2..];
    (b.get_u16_le(), b.get_u16_le(), b.get_u16_le(), b.get_u16_le(), b.get_u8() != 0)
}

/// Stat and skill actions the client asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum StatRequest {
    /// Raise a base stat by one; `stat` is the rAthena SP_STR..SP_LUK id (13..=18).
    Raise(u16),
    /// Raise a trait stat by one; `stat` is the rAthena SP_POW..SP_CRT id (219..=224).
    RaiseTrait(u16),
    /// CZ_USE_SKILL_START (0x0b10): keep using a skill while its key is held (Rolling Cutter).
    HoldSkill { skill: u16, level: u16, target: u32 },
    /// CZ_USE_SKILL_END (0x0b11): the key was let go.
    ReleaseSkill(u16),
    SkillUp(u16),
    UseSkill { skill: u16, level: u16, target: u32 },
    UseSkillAt { skill: u16, level: u16, x: u16, y: u16 },
    /// A ground skill that carries a message (Graffiti, Talkie Box).
    UseSkillAtWithText { skill: u16, level: u16, x: u16, y: u16, text: String },
    /// Store a shortcut bar slot (tab 0) on the server.
    SetHotkey { index: u16, key: Hotkey },
    /// Remember which bar row is showing.
    HotkeyRow(u8),
}

impl Encode for StatRequest {
    fn encode(&self, d: &mut BytesMut) {
        match *self {
            StatRequest::Raise(stat) => put_db_packet(d, "StatusUp", &[&stat.to_le_bytes(), &[1]]),
            StatRequest::HoldSkill { skill, level, target } => {
                d.put_u16_le(0x0b10);
                d.put_u16_le(skill);
                d.put_u16_le(level);
                d.put_u32_le(target);
            }
            StatRequest::ReleaseSkill(skill) => {
                d.put_u16_le(0x0b11);
                d.put_u16_le(skill);
            }
            // CZ_ADVANCED_STATUS_CHANGE (0x0b24): <type>.W <amount>.W, one point at a time.
            StatRequest::RaiseTrait(stat) => {
                d.put_u16_le(0x0b24);
                d.put_u16_le(stat);
                d.put_u16_le(1);
            }
            StatRequest::SkillUp(skill) => put_db_packet(d, "SkillUp", &[&skill.to_le_bytes()]),
            StatRequest::UseSkill { skill, level, target } => {
                put_db_packet(d, "UseSkillToId", &[&level.to_le_bytes(), &skill.to_le_bytes(), &target.to_le_bytes()])
            }
            StatRequest::UseSkillAt { skill, level, x, y } => put_db_packet(
                d,
                "UseSkillToPos",
                &[&level.to_le_bytes(), &skill.to_le_bytes(), &x.to_le_bytes(), &y.to_le_bytes()],
            ),
            // CZ_USE_SKILL_TOGROUND_WITHTALKBOX: the message is 21 bytes at this PACKETVER.
            StatRequest::UseSkillAtWithText { skill, level, x, y, ref text } => {
                let mut msg = [0u8; 21];
                let n = text.len().min(20);
                msg[..n].copy_from_slice(&text.as_bytes()[..n]);
                put_db_packet(d, "UseSkillToPosMoreInfo", &[&level.to_le_bytes(), &skill.to_le_bytes(), &x.to_le_bytes(), &y.to_le_bytes(), &msg]);
            }
            // CZ_SHORTCUT_KEY_CHANGE2 / CZ_SHORTCUTKEYBAR_ROTATE2 (registered without field offsets).
            StatRequest::SetHotkey { index, key } => {
                d.put_u16_le(0x0b21);
                d.put_u16_le(0);
                d.put_u16_le(index);
                d.put_u8(key.skill as u8);
                d.put_u32_le(key.id);
                d.put_i16_le(key.count);
            }
            StatRequest::HotkeyRow(row) => {
                d.put_u16_le(0x0b22);
                d.put_u16_le(0);
                d.put_u8(row);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_packet;

    #[test]
    fn pickup_keeps_the_favourites_tab() {
        // ZC_ITEM_PICKUP_ACK, 70 bytes: a Jellopy taken from storage into the favourites tab.
        let mut p = vec![0; 70];
        p[..2].copy_from_slice(&0x0b41u16.to_le_bytes());
        p[2..4].copy_from_slice(&7u16.to_le_bytes()); // index
        p[4..6].copy_from_slice(&3u16.to_le_bytes()); // amount
        p[6..10].copy_from_slice(&909u32.to_le_bytes());
        p[10] = 1; // identified
        p[65] = 1; // favorite, after hire date, bind and the five options
        p[68] = 4; // refine
        let it = parse_pickup(&p).expect("a successful add");
        assert_eq!((it.index, it.amount, it.id, it.refine), (7, 3, 909, 4));
        assert!(it.favorite);
        p[65] = 0;
        assert!(!parse_pickup(&p).unwrap().favorite);
        p[33] = 4; // result: inventory full
        assert!(parse_pickup(&p).is_none());
    }

    #[test]
    fn normal_item_layout() {
        let mut p = vec![0x09, 0x0b, 0, 0, 0];
        p.extend_from_slice(&2u16.to_le_bytes());
        p.extend_from_slice(&501u32.to_le_bytes());
        p.push(0);
        p.extend_from_slice(&5i16.to_le_bytes());
        p.extend_from_slice(&0u32.to_le_bytes());
        p.extend_from_slice(&[0; 16]);
        p.extend_from_slice(&0i32.to_le_bytes());
        p.push(1);
        assert_eq!(p.len(), 5 + 34);
        let items = parse_itemlist_normal(&p);
        assert_eq!((items[0].index, items[0].id, items[0].amount, items[0].identified), (2, 501, 5, true));
    }

    #[test]
    fn request_sizes() {
        let size = |r: &dyn Encode| {
            let mut b = BytesMut::new();
            r.encode(&mut b);
            b.len()
        };
        assert_eq!(size(&ItemRequest::Use { index: 2, account_id: 9 }), client_packet("UseItem").len as usize);
        assert_eq!(size(&ItemRequest::PickUp { floor_id: 3 }), client_packet("TakeItem").len as usize);
        assert_eq!(size(&ItemRequest::UsePackage { index: 2, account_id: 9, item: 101060, set: 1 }), client_packet("itempackage_select").len as usize);
        assert_eq!(size(&ItemRequest::Equip { index: 2, location: 256 }), 8);
        assert_eq!(size(&StatRequest::Raise(13)), client_packet("StatusUp").len as usize);
        assert_eq!(size(&StatRequest::RaiseTrait(219)), client_packet("traitstatus_up").len as usize);
        assert_eq!(size(&StatRequest::HoldSkill { skill: 2036, level: 5, target: 1 }), client_packet("StartUseSkillToId").len as usize);
        assert_eq!(size(&StatRequest::ReleaseSkill(2036)), client_packet("StopUseSkillToId").len as usize);
        assert_eq!(size(&StatRequest::UseSkill { skill: 1, level: 1, target: 1 }), client_packet("UseSkillToId").len as usize);
    }
}

/// A shortcut bar slot: a skill (id, level) or an item (id, amount shown).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Hotkey {
    pub skill: bool,
    pub id: u32,
    pub count: i16,
}

/// ZC_SHORTCUT_KEY_LIST (0x0b20): (tab, visible row, 38 slots).
pub fn parse_hotkeys(data: &[u8]) -> (u16, u8, Vec<Hotkey>) {
    let mut b = &data[2..];
    let rotate = b.get_u8();
    let tab = b.get_u16_le();
    let mut keys = vec![];
    while b.remaining() >= 7 {
        keys.push(Hotkey { skill: b.get_u8() != 0, id: b.get_u32_le(), count: b.get_i16_le() });
    }
    (tab, rotate, keys)
}

/// A skill's cast started (ZC_USESKILL_ACK, 0x0b1a).
#[derive(Debug, Clone, PartialEq)]
pub struct Cast {
    pub src: u32,
    pub target: u32,
    pub cell: (u16, u16),
    pub skill: u16,
    pub ms: u32,
}

pub fn parse_cast(data: &[u8]) -> Cast {
    let mut b = &data[2..];
    let src = b.get_u32_le();
    let target = b.get_u32_le();
    let cell = (b.get_u16_le(), b.get_u16_le());
    let skill = b.get_u16_le();
    b.advance(4); // element
    Cast { src, target, cell, skill, ms: b.get_u32_le() }
}

/// ZC_ACK_TOUSESKILL (0x0110): the skill could not be used. (skill, cause)
pub fn parse_skill_fail(data: &[u8]) -> (u16, u8) {
    (u16::from_le_bytes([data[2], data[3]]), data[13])
}

/// ZC_NOTIFY_SKILL (0x01de): a damaging skill hit, as an attack. (skill, act)
pub fn parse_skill_hit(data: &[u8]) -> (u16, crate::packets::Act) {
    let mut b = &data[2..];
    let skill = b.get_u16_le();
    let src = b.get_u32_le();
    let target = b.get_u32_le();
    b.advance(4); // server tick
    let src_speed = b.get_i32_le();
    let dmg_speed = b.get_i32_le();
    let damage = b.get_i32_le();
    b.advance(2); // level
    let hits = b.get_i16_le();
    let kind = if hits > 1 { 8 } else { 0 };
    (skill, crate::packets::Act { src, target, src_speed, dmg_speed, damage, hits, kind, left_damage: 0 })
}

/// ZC_USE_SKILL (0x09cb): a non-damaging skill went off. (skill, level, target, src)
pub fn parse_skill_used(data: &[u8]) -> (u16, i32, u32, u32) {
    let mut b = &data[2..];
    (b.get_u16_le(), b.get_i32_le(), b.get_u32_le(), b.get_u32_le())
}

/// ZC_NOTIFY_GROUNDSKILL (0x0117): a skill placed on a cell. (skill, src, cell)
pub fn parse_ground_skill(data: &[u8]) -> (u16, u32, (u16, u16)) {
    let mut b = &data[2..];
    let skill = b.get_u16_le();
    let src = b.get_u32_le();
    b.advance(2); // level
    (skill, src, (b.get_i16_le() as u16, b.get_i16_le() as u16))
}

/// ZC_STATE_CHANGE (0x0229): (unit, effect state / option bits: 0x2 hide, 0x4 cloak...).
pub fn parse_state_change(data: &[u8]) -> (u32, u32) {
    let mut b = &data[2..];
    let id = b.get_u32_le();
    b.advance(4); // body and health state
    (id, b.get_i32_le() as u32)
}

/// ZC_NOTIFY_EFFECT (0x019b): (unit, effect: 0 base level up, 1 job level up, ...).
pub fn parse_notify_effect(data: &[u8]) -> (u32, u32) {
    let mut b = &data[2..];
    (b.get_u32_le(), b.get_u32_le())
}

/// ZC_SKILL_ENTRY5 (0x09ca): a lasting ground effect appeared.
/// (unit id, creator, cell, unit type: 0x7e safety wall, 0x7f fire wall, 0x83 sanctuary...)
pub fn parse_skill_entry(data: &[u8]) -> (u32, u32, (u16, u16), u32) {
    let mut b = &data[4..];
    let id = b.get_u32_le();
    let creator = b.get_u32_le();
    let cell = (b.get_u16_le(), b.get_u16_le());
    (id, creator, cell, b.get_u32_le())
}

/// ZC_SKILL_DISAPPEAR (0x0120): the ground effect `id` ended.
pub fn parse_skill_disappear(data: &[u8]) -> u32 {
    u32::from_le_bytes([data[2], data[3], data[4], data[5]])
}

/// ZC_COUPLESTATUS (0x0141): a base stat with its bonus. (SP id 13..=18, base, plus)
pub fn parse_couple_status(data: &[u8]) -> (u32, i32, i32) {
    let mut b = &data[2..];
    (b.get_u32_le(), b.get_i32_le(), b.get_i32_le())
}

/// ZC_EQUIPWIN_MICROSCOPE (0x0b37): another player's worn equipment, (name, job, items).
pub fn parse_view_equip(data: &[u8]) -> Option<(String, i16, Vec<ItemData>)> {
    let mut b = data.get(4..47)?;
    let name = crate::packets::get_fixed_str(&mut b, 24);
    let job = b.get_i16_le();
    // The entries are the inventory's EQUIPITEM_INFO; reuse that parser (it skips 5 bytes).
    let mut list = vec![0u8; 5];
    list.extend_from_slice(&data[47..]);
    Some((name, job, parse_itemlist_equip(&list)))
}

/// CZ_EQUIPWIN_MICROSCOPE: ask to see a player's equipment.
pub struct CzViewEquip(pub u32);

impl Encode for CzViewEquip {
    fn encode(&self, d: &mut BytesMut) {
        d.put_u16_le(0x02d6);
        d.put_u32_le(self.0);
    }
}

/// CZ_CONFIG (0x02d8): let others see our equipment (type 0).
pub struct CzShowEquip(pub bool);

impl Encode for CzShowEquip {
    fn encode(&self, d: &mut BytesMut) {
        d.put_u16_le(0x02d8);
        d.put_u32_le(0);
        d.put_u32_le(self.0 as u32);
    }
}

/// Equipment switch set: add (index, slots), remove, swap all.
#[derive(Debug, Clone, PartialEq)]
pub enum SwitchRequest {
    Add { index: u16, location: u32 },
    Remove(u16),
    All,
}

impl crate::codec::Encode for SwitchRequest {
    fn encode(&self, d: &mut bytes::BytesMut) {
        use bytes::BufMut;
        match self {
            SwitchRequest::Add { index, location } => {
                d.put_u16_le(0x0a97);
                d.put_u16_le(*index);
                d.put_u32_le(*location);
            }
            SwitchRequest::Remove(index) => {
                d.put_u16_le(0x0a99);
                d.put_u16_le(*index);
            }
            SwitchRequest::All => d.put_u16_le(0x0a9c),
        }
    }
}

/// CZ_REQ_MERGE_ITEM: merge these inventory stacks (same item) into one.
pub struct MergeItems(pub Vec<u16>);

impl crate::codec::Encode for MergeItems {
    fn encode(&self, d: &mut bytes::BytesMut) {
        use bytes::BufMut;
        d.put_u16_le(0x096e);
        d.put_u16_le((4 + 2 * self.0.len()) as u16);
        for i in &self.0 {
            d.put_u16_le(*i);
        }
    }
}

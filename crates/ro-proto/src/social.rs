//! NPC shops, Kafra storage, player trading, party, friends and guild chat at PACKETVER
//! 20250402 (layouts from rAthena's preprocessed packets.hpp / packets_struct.hpp).

use bytes::{Buf, BufMut, BytesMut};
use encoding_rs::EUC_KR;

use crate::{
    codec::Encode,
    items::ItemData,
    packets::{get_fixed_str, put_db_packet, put_fixed_str},
};

fn text_from(data: &[u8], from: usize) -> String {
    let raw = data.get(from..).unwrap_or_default();
    let end = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
    EUC_KR.decode(&raw[..end]).0.into_owned()
}

/// A variable packet: opcode, u16 length, body.
fn put_var(d: &mut BytesMut, opcode: u16, body: &[u8]) {
    d.put_u16_le(opcode);
    d.put_u16_le((4 + body.len()) as u16);
    d.put_slice(body);
}

fn euc(s: &str) -> Vec<u8> {
    let mut v = EUC_KR.encode(s).0.into_owned();
    v.push(0);
    v
}

// ---------------------------------------------------------------- NPC shops

#[derive(Debug, Clone, PartialEq)]
pub struct ShopItem {
    pub id: u32,
    pub price: u32,
    /// Price after the buyer's Discount skill.
    pub discount_price: u32,
    pub kind: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SellOffer {
    /// Inventory wire index.
    pub index: u16,
    pub price: u32,
    /// Price after the seller's Overcharge skill.
    pub overcharge_price: u32,
}

/// ZC_SELECT_DEALTYPE (0x00c4): the NPC asks buy or sell.
pub fn parse_deal_type(data: &[u8]) -> u32 {
    (&data[2..]).get_u32_le()
}

/// ZC_PC_PURCHASE_ITEMLIST (0x0b77): 19-byte entries.
pub fn parse_buy_list(data: &[u8]) -> Vec<ShopItem> {
    data.get(4..)
        .unwrap_or_default()
        .chunks_exact(19)
        .map(|mut b| {
            let id = b.get_u32_le();
            let price = b.get_u32_le();
            let discount_price = b.get_u32_le();
            let kind = b.get_u8();
            ShopItem { id, price, discount_price, kind }
        })
        .collect()
}

/// ZC_PC_SELL_ITEMLIST (0x00c7): 10-byte entries.
pub fn parse_sell_list(data: &[u8]) -> Vec<SellOffer> {
    data.get(4..)
        .unwrap_or_default()
        .chunks_exact(10)
        .map(|mut b| SellOffer { index: b.get_u16_le(), price: b.get_u32_le(), overcharge_price: b.get_u32_le() })
        .collect()
}

/// ZC_PC_CASH_POINT_ITEMLIST (0x0287): a cashshop / itemshop / pointshop NPC's goods, with
/// what we hold of its currency (and Kafra points): (currency, kafra points, items).
pub fn parse_cash_shop(data: &[u8]) -> Option<(u32, u32, Vec<ShopItem>)> {
    let mut b = data.get(4..12)?;
    let (cash, kafra) = (b.get_u32_le(), b.get_u32_le());
    let items = data[12..]
        .chunks_exact(13)
        .map(|mut b| {
            let price = b.get_u32_le();
            let discount_price = b.get_u32_le();
            let kind = b.get_u8();
            ShopItem { id: b.get_u32_le(), price, discount_price, kind }
        })
        .collect();
    Some((cash, kafra, items))
}

/// ZC_PC_CASH_POINT_UPDATE (0x0289): (currency, kafra points, error; 0 = bought).
pub fn parse_cash_shop_result(data: &[u8]) -> Option<(u32, u32, u16)> {
    let mut b = data.get(2..12)?;
    Some((b.get_u32_le(), b.get_u32_le(), b.get_u16_le()))
}

#[derive(Debug, Clone, PartialEq)]
pub enum ShopRequest {
    /// Answer ZC_SELECT_DEALTYPE: buy (true) or sell.
    Deal { npc: u32, buy: bool },
    /// (item id, amount) pairs.
    Buy(Vec<(u32, u16)>),
    /// (inventory index, amount) pairs.
    Sell(Vec<(u16, u16)>),
    /// CZ_PC_BUY_CASH_POINT_ITEM: (item id, amount) pairs from a cash-point shop, paying
    /// `kafra` of the price in Kafra points.
    CashBuy { kafra: u32, items: Vec<(u32, u16)> },
}

impl Encode for ShopRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            ShopRequest::Deal { npc, buy } => {
                d.put_u16_le(0x00c5);
                d.put_u32_le(*npc);
                d.put_u8(if *buy { 0 } else { 1 });
            }
            ShopRequest::Buy(list) => {
                let mut body = vec![];
                for (id, amount) in list {
                    body.extend_from_slice(&amount.to_le_bytes());
                    body.extend_from_slice(&id.to_le_bytes());
                }
                put_var(d, 0x00c8, &body);
            }
            ShopRequest::Sell(list) => {
                let mut body = vec![];
                for (index, amount) in list {
                    body.extend_from_slice(&index.to_le_bytes());
                    body.extend_from_slice(&amount.to_le_bytes());
                }
                put_var(d, 0x00c9, &body);
            }
            ShopRequest::CashBuy { kafra, items } => {
                let mut body = kafra.to_le_bytes().to_vec();
                body.extend_from_slice(&(items.len() as u16).to_le_bytes());
                for (id, amount) in items {
                    body.extend_from_slice(&amount.to_le_bytes());
                    body.extend_from_slice(&id.to_le_bytes());
                }
                put_var(d, 0x0288, &body);
            }
        }
    }
}

#[cfg(test)]
mod cash_shop_tests {
    use super::*;

    #[test]
    fn cash_shop_round_trip() {
        let mut p = vec![0x87, 0x02, 25, 0];
        p.extend(500u32.to_le_bytes());
        p.extend(30u32.to_le_bytes());
        p.extend(100u32.to_le_bytes());
        p.extend(100u32.to_le_bytes());
        p.push(0);
        p.extend(12210u32.to_le_bytes());
        let (cash, kafra, items) = parse_cash_shop(&p).unwrap();
        assert_eq!((cash, kafra, items.len(), items[0].id, items[0].price), (500, 30, 1, 12210, 100));
        let mut d = BytesMut::new();
        ShopRequest::CashBuy { kafra: 5, items: vec![(12210, 2)] }.encode(&mut d);
        assert_eq!(&d[..], &[0x88, 0x02, 16, 0, 5, 0, 0, 0, 1, 0, 2, 0, 0xb2, 0x2f, 0, 0]);
    }
}

// ---------------------------------------------------------------- Kafra storage

/// ZC_ADD_ITEM_TO_STORE (0x0b44).
pub fn parse_store_add(data: &[u8]) -> ItemData {
    let mut b = &data[2..];
    let index = b.get_i16_le() as u16;
    let amount = b.get_i32_le().max(0) as u16;
    let id = b.get_u32_le();
    let kind = b.get_u8();
    let identified = b.get_u8() != 0;
    b.advance(1); // damaged
    let cards = [b.get_u32_le(), b.get_u32_le(), b.get_u32_le(), b.get_u32_le()];
    b.advance(25); // options
    let refine = b.get_u8();
    ItemData { index, id, kind, amount, location: 0, equipped: 0, refine, grade: 0, identified, cards, look: 0, favorite: false }
}

/// ZC_DELETE_ITEM_FROM_STORE (0x00f6): (index, amount).
pub fn parse_store_delete(data: &[u8]) -> (u16, u16) {
    let mut b = &data[2..];
    (b.get_u16_le(), b.get_u32_le() as u16)
}

/// ZC_NOTIFY_STOREITEM_COUNTINFO (0x00f2): (used, capacity).
pub fn parse_store_count(data: &[u8]) -> (u16, u16) {
    let mut b = &data[2..];
    (b.get_u16_le(), b.get_u16_le())
}

#[derive(Debug, Clone, PartialEq)]
pub enum StorageRequest {
    /// Inventory index -> storage.
    Put { index: u16, amount: u32 },
    /// Storage index -> inventory.
    Take { index: u16, amount: u32 },
    /// CZ_MOVE_ITEM_TO_PERSONAL: storage index -> the inventory's favourites tab (the
    /// original's Shift+right-click in storage).
    TakeFavorite { index: u16, amount: u32 },
    Close,
}

impl Encode for StorageRequest {
    fn encode(&self, d: &mut BytesMut) {
        match *self {
            StorageRequest::Put { index, amount } => put_db_packet(d, "MoveToKafra", &[&index.to_le_bytes(), &amount.to_le_bytes()]),
            StorageRequest::Take { index, amount } => put_db_packet(d, "MoveFromKafra", &[&index.to_le_bytes(), &amount.to_le_bytes()]),
            StorageRequest::TakeFavorite { index, amount } => {
                d.put_u16_le(crate::client_packet("MoveFromKafraFav").opcode);
                d.put_u32_le(0);
                d.put_u16_le(index);
                d.put_u32_le(amount);
            }
            StorageRequest::Close => d.put_u16_le(0x0193),
        }
    }
}

// ---------------------------------------------------------------- trading

/// ZC_REQ_EXCHANGE_ITEM (0x01f4): (requester name, their id, their level).
pub fn parse_trade_request(data: &[u8]) -> (String, u32, u16) {
    let mut b = &data[2..];
    let name = get_fixed_str(&mut b, 24);
    (name, b.get_u32_le(), b.get_u16_le())
}

/// ZC_ACK_EXCHANGE_ITEM (0x01f5): result (0 too far, 1 not found, 2 already trading,
/// 3 accepted, 4 declined, 5 busy).
pub fn parse_trade_ack(data: &[u8]) -> u8 {
    data[2]
}

/// ZC_ADD_EXCHANGE_ITEM (0x0b42): what the partner put up. `id` 0 means zeny.
pub fn parse_trade_item(data: &[u8]) -> (u32, i32, u8) {
    let mut b = &data[2..];
    let id = b.get_u32_le();
    b.advance(1);
    let amount = b.get_i32_le();
    let identified = b.get_u8();
    (id, amount, identified)
}

#[derive(Debug, Clone, PartialEq)]
pub enum TradeRequest {
    Ask { target: u32 },
    Answer { accept: bool },
    /// Inventory index (0 = zeny) and amount.
    Add { index: u16, amount: i32 },
    /// Lock in our side.
    Ok,
    /// Final confirmation once both sides are locked.
    Commit,
    Cancel,
}

impl Encode for TradeRequest {
    fn encode(&self, d: &mut BytesMut) {
        match *self {
            TradeRequest::Ask { target } => put_db_packet(d, "TradeRequest", &[&target.to_le_bytes()]),
            TradeRequest::Answer { accept } => put_db_packet(d, "TradeAck", &[&[if accept { 3 } else { 4 }]]),
            TradeRequest::Add { index, amount } => {
                d.put_u16_le(0x00e8);
                d.put_u16_le(index);
                d.put_i32_le(amount);
            }
            TradeRequest::Ok => d.put_u16_le(0x00eb),
            TradeRequest::Commit => d.put_u16_le(0x00ef),
            TradeRequest::Cancel => d.put_u16_le(0x00ed),
        }
    }
}

// ---------------------------------------------------------------- party

#[derive(Debug, Clone, PartialEq)]
pub struct PartyMember {
    pub account_id: u32,
    pub char_id: u32,
    pub name: String,
    pub map: String,
    pub leader: bool,
    pub online: bool,
    pub job: i16,
    pub level: i16,
}

/// ZC_GROUP_LIST (0x0ae5): (party name, members); 54-byte entries.
pub fn parse_party_list(data: &[u8]) -> (String, Vec<PartyMember>) {
    let mut b = &data[4..];
    let name = get_fixed_str(&mut b, 24);
    let members = b
        .chunks_exact(54)
        .map(|mut m| {
            let account_id = m.get_u32_le();
            let char_id = m.get_u32_le();
            let name = get_fixed_str(&mut m, 24);
            let map = get_fixed_str(&mut m, 16);
            let leader = m.get_u8() == 0;
            let online = m.get_u8() == 0;
            PartyMember { account_id, char_id, name, map: map.trim_end_matches(".gat").into(), leader, online, job: m.get_i16_le(), level: m.get_i16_le() }
        })
        .collect();
    (name, members)
}

/// ZC_ADD_MEMBER_TO_GROUP (0x0ae4): (party name, member).
pub fn parse_party_add(data: &[u8]) -> (String, PartyMember) {
    let mut b = &data[2..];
    let account_id = b.get_u32_le();
    let char_id = b.get_u32_le();
    let leader = b.get_u32_le() == 0;
    let job = b.get_i16_le();
    let level = b.get_i16_le();
    b.advance(4); // x, y
    let online = b.get_u8() == 0;
    let party = get_fixed_str(&mut b, 24);
    let name = get_fixed_str(&mut b, 24);
    let map = get_fixed_str(&mut b, 16);
    (party, PartyMember { account_id, char_id, name, map: map.trim_end_matches(".gat").into(), leader, online, job, level })
}

/// ZC_DELETE_MEMBER_FROM_GROUP (0x0105): (account id, name).
pub fn parse_party_remove(data: &[u8]) -> (u32, String) {
    let mut b = &data[2..];
    let aid = b.get_u32_le();
    (aid, get_fixed_str(&mut b, 24))
}

/// ZC_NOTIFY_HP_TO_GROUPM (0x080e): (account id, hp, max hp).
pub fn parse_party_hp(data: &[u8]) -> (u32, i32, i32) {
    let mut b = &data[2..];
    (b.get_u32_le(), b.get_i32_le(), b.get_i32_le())
}

/// ZC_PARTY_JOIN_REQ (0x02c6): (party id, party name).
pub fn parse_party_invite(data: &[u8]) -> (u32, String) {
    let mut b = &data[2..];
    let id = b.get_u32_le();
    (id, get_fixed_str(&mut b, 24))
}

/// An Adventurer's Agency join request, as the ad's leader sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct AgencyAsk {
    pub char_id: u32,
    pub account_id: u32,
    pub name: String,
    pub level: u16,
    pub job: u16,
}

/// ZC_PARTY_REQ_MASTER_TO_JOIN (0x0ae7). rAthena sends the requester's base level and job
/// in the fields named x and y.
pub fn parse_agency_ask(data: &[u8]) -> Option<AgencyAsk> {
    let mut b = data.get(2..38)?;
    let char_id = b.get_u32_le();
    let account_id = b.get_u32_le();
    let name = get_fixed_str(&mut b, 24);
    Some(AgencyAsk { char_id, account_id, name, level: b.get_u16_le(), job: b.get_u16_le() })
}

/// ZC_PARTY_JOIN_REQ_ACK_FROM_MASTER (0x0afa): (leader name, party name, refused).
pub fn parse_agency_result(data: &[u8]) -> Option<(String, String, bool)> {
    let mut b = data.get(2..58)?;
    let leader = get_fixed_str(&mut b, 24);
    let party = get_fixed_str(&mut b, 24);
    b.advance(4);
    Some((leader, party, b.get_u32_le() != 0))
}

/// ZC_NOTIFY_CHAT_PARTY (0x0109) / ZC_GUILD_CHAT (0x017f): message text ("Name : text").
pub fn parse_party_chat(data: &[u8]) -> String {
    text_from(data, 8)
}

pub fn parse_guild_chat(data: &[u8]) -> String {
    text_from(data, 4)
}

/// ZC_UPDATE_GDID (0x02f7): guild name ("" when not in a guild).
pub fn parse_guild_name(data: &[u8]) -> (u32, String) {
    let mut b = &data[2..];
    let id = b.get_u32_le();
    b.advance(4 + 4 + 1 + 4);
    (id, get_fixed_str(&mut b, 24))
}

#[derive(Debug, Clone, PartialEq)]
pub enum PartyRequest {
    Create { name: String },
    Invite { name: String },
    Answer { party: u32, accept: bool },
    Leave,
    /// CZ_REQ_EXPEL_GROUP_MEMBER: the leader removes a member.
    Expel { account_id: u32, name: String },
    /// CZ_CHANGE_GROUP_MASTER (0x07da): the leader hands the party to a member.
    ChangeLeader { account_id: u32 },
    /// Party chat; `speaker` is our name ("Name : text").
    Chat { speaker: String, text: String },
    GuildChat { speaker: String, text: String },
    ClanChat { speaker: String, text: String },
    /// CZ_PARTY_REQ_MASTER_TO_JOIN: ask an Adventurer's Agency ad's leader to let us in.
    AgencyJoin { char_id: u32, account_id: u32 },
    /// CZ_PARTY_REQ_ACK_MASTER_TO_JOIN: the leader's answer to a join request.
    AgencyAnswer { char_id: u32, account_id: u32, accept: bool },
}

impl Encode for PartyRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            PartyRequest::Create { name } => {
                d.put_u16_le(0x01e8);
                put_fixed_str(d, name, 24);
                d.put_u8(0);
                d.put_u8(0);
            }
            PartyRequest::Invite { name } => {
                d.put_u16_le(0x02c4);
                put_fixed_str(d, name, 24);
            }
            PartyRequest::Answer { party, accept } => {
                d.put_u16_le(0x02c7);
                d.put_u32_le(*party);
                d.put_u8(*accept as u8);
            }
            PartyRequest::Leave => d.put_u16_le(0x0100),
            PartyRequest::ChangeLeader { account_id } => put_db_packet(d, "PartyChangeLeader", &[&account_id.to_le_bytes()]),
            PartyRequest::Expel { account_id, name } => {
                d.put_u16_le(0x0103);
                d.put_u32_le(*account_id);
                put_fixed_str(d, name, 24);
            }
            PartyRequest::Chat { speaker, text } => put_var(d, 0x0108, &euc(&format!("{speaker} : {text}"))),
            PartyRequest::GuildChat { speaker, text } => put_var(d, 0x017e, &euc(&format!("{speaker} : {text}"))),
            PartyRequest::ClanChat { speaker, text } => put_var(d, 0x098d, &euc(&format!("{speaker} : {text}"))),
            PartyRequest::AgencyJoin { char_id, account_id } => {
                d.put_u16_le(0x0ae6);
                d.put_u32_le(*char_id);
                d.put_u32_le(*account_id);
            }
            PartyRequest::AgencyAnswer { char_id, account_id, accept } => {
                d.put_u16_le(0x0af8);
                d.put_u32_le(*char_id);
                d.put_u32_le(*account_id);
                d.put_u8(*accept as u8);
            }
        }
    }
}

// ---------------------------------------------------------------- friends

#[derive(Debug, Clone, PartialEq)]
pub struct Friend {
    pub account_id: u32,
    pub char_id: u32,
    pub name: String,
    pub online: bool,
}

/// ZC_FRIENDS_LIST (0x0201): 32-byte entries (online state follows in ZC_FRIENDS_STATE).
pub fn parse_friends(data: &[u8]) -> Vec<Friend> {
    data.get(4..)
        .unwrap_or_default()
        .chunks_exact(32)
        .map(|mut b| {
            let account_id = b.get_u32_le();
            let char_id = b.get_u32_le();
            Friend { account_id, char_id, name: get_fixed_str(&mut b, 24), online: false }
        })
        .collect()
}

/// ZC_FRIENDS_STATE (0x0206): (account id, char id, online).
pub fn parse_friend_state(data: &[u8]) -> (u32, u32, bool) {
    let mut b = &data[2..];
    (b.get_u32_le(), b.get_u32_le(), b.get_u8() == 0)
}

/// ZC_REQ_ADD_FRIENDS (0x0207): (account id, char id, name) of whoever asks.
pub fn parse_friend_request(data: &[u8]) -> (u32, u32, String) {
    let mut b = &data[2..];
    let aid = b.get_u32_le();
    let cid = b.get_u32_le();
    (aid, cid, get_fixed_str(&mut b, 24))
}

/// ZC_ADD_FRIENDS_LIST (0x0209): (result, friend); result 0 = added.
pub fn parse_friend_added(data: &[u8]) -> (u16, Friend) {
    let mut b = &data[2..];
    let result = b.get_u16_le();
    let account_id = b.get_u32_le();
    let char_id = b.get_u32_le();
    (result, Friend { account_id, char_id, name: get_fixed_str(&mut b, 24), online: true })
}

/// ZC_DELETE_FRIENDS (0x020a): (account id, char id).
pub fn parse_friend_removed(data: &[u8]) -> (u32, u32) {
    let mut b = &data[2..];
    (b.get_u32_le(), b.get_u32_le())
}

#[derive(Debug, Clone, PartialEq)]
pub enum FriendRequest {
    Add { name: String },
    Answer { account_id: u32, char_id: u32, accept: bool },
    Remove { account_id: u32, char_id: u32 },
}

impl Encode for FriendRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            FriendRequest::Add { name } => {
                d.put_u16_le(0x0202);
                put_fixed_str(d, name, 24);
            }
            FriendRequest::Answer { account_id, char_id, accept } => {
                d.put_u16_le(0x0208);
                d.put_u32_le(*account_id);
                d.put_u32_le(*char_id);
                d.put_u32_le(*accept as u32);
            }
            FriendRequest::Remove { account_id, char_id } => {
                d.put_u16_le(0x0203);
                d.put_u32_le(*account_id);
                d.put_u32_le(*char_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_packet;

    fn encoded(r: &dyn Encode) -> BytesMut {
        let mut b = BytesMut::new();
        r.encode(&mut b);
        b
    }

    #[test]
    fn delegating_leadership_names_the_new_leader() {
        let b = encoded(&PartyRequest::ChangeLeader { account_id: 2000001 });
        assert_eq!(b.len(), client_packet("PartyChangeLeader").len as usize);
        assert_eq!(&b[..], &[0xda, 0x07, 0x81, 0x84, 0x1e, 0x00]);
    }

    #[test]
    fn fixed_request_sizes_match_server_db() {
        assert_eq!(encoded(&ShopRequest::Deal { npc: 1, buy: true }).len(), client_packet("NpcBuySellSelected").len as usize);
        assert_eq!(encoded(&StorageRequest::Put { index: 2, amount: 1 }).len(), client_packet("MoveToKafra").len as usize);
        assert_eq!(encoded(&StorageRequest::Take { index: 2, amount: 1 }).len(), client_packet("MoveFromKafra").len as usize);
        assert_eq!(encoded(&StorageRequest::Close).len(), client_packet("CloseKafra").len as usize);
        assert_eq!(encoded(&TradeRequest::Ask { target: 1 }).len(), client_packet("TradeRequest").len as usize);
        assert_eq!(encoded(&TradeRequest::Answer { accept: true }).len(), client_packet("TradeAck").len as usize);
        assert_eq!(encoded(&TradeRequest::Add { index: 2, amount: 1 }).len(), client_packet("TradeAddItem").len as usize);
        assert_eq!(encoded(&PartyRequest::Create { name: "p".into() }).len(), client_packet("CreateParty2").len as usize);
        assert_eq!(encoded(&PartyRequest::Invite { name: "x".into() }).len(), client_packet("PartyInvite2").len as usize);
        assert_eq!(encoded(&PartyRequest::Answer { party: 1, accept: true }).len(), client_packet("ReplyPartyInvite2").len as usize);
        assert_eq!(encoded(&FriendRequest::Add { name: "x".into() }).len(), client_packet("FriendsListAdd").len as usize);
        assert_eq!(encoded(&FriendRequest::Answer { account_id: 1, char_id: 2, accept: true }).len(), client_packet("FriendsListReply").len as usize);
        assert_eq!(encoded(&FriendRequest::Remove { account_id: 1, char_id: 2 }).len(), client_packet("FriendsListRemove").len as usize);
    }

    #[test]
    fn variable_packets_have_lengths() {
        let b = encoded(&ShopRequest::Buy(vec![(501, 3)]));
        assert_eq!(u16::from_le_bytes([b[2], b[3]]) as usize, b.len());
        assert_eq!(b.len(), 4 + 6);
        let b = encoded(&PartyRequest::Chat { speaker: "A".into(), text: "hi".into() });
        assert_eq!(&b[4..], b"A : hi\0");
    }
}

// ---------------------------------------------------------------- clans

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClanInfo {
    pub id: u32,
    pub name: String,
    pub master: String,
    pub map: String,
    pub allies: Vec<String>,
    pub antagonists: Vec<String>,
}

/// ZC_CLANINFO (0x098a).
pub fn parse_clan_info(data: &[u8]) -> Option<ClanInfo> {
    let mut b = data.get(4..)?;
    if b.len() < 4 + 24 + 24 + 16 + 2 {
        return None;
    }
    let id = b.get_u32_le();
    let name = get_fixed_str(&mut b, 24);
    let master = get_fixed_str(&mut b, 24);
    let map = get_fixed_str(&mut b, 16);
    let (allies, antagonists) = (b.get_u8() as usize, b.get_u8() as usize);
    let mut names = |n: usize| (0..n).take_while(|_| b.len() >= 24).map(|_| get_fixed_str(&mut b, 24)).collect::<Vec<_>>();
    let allies = names(allies);
    let antagonists = names(antagonists);
    Some(ClanInfo { id, name, master, map, allies, antagonists })
}

/// ZC_NOTIFY_CLAN_CHAT (0x098e): "Name : text".
pub fn parse_clan_chat(data: &[u8]) -> String {
    text_from(data, 28)
}

/// CZ_REQNAME_BYGID: ask for a unit's full name (party, guild, rank).
pub struct ReqName(pub u32);

impl Encode for ReqName {
    fn encode(&self, d: &mut BytesMut) {
        put_db_packet(d, "SolveCharName", &[&self.0.to_le_bytes()]);
    }
}

/// ZC_ACK_REQNAMEALL (0x0a30): (id, name, party, guild, rank).
pub fn parse_name_all(data: &[u8]) -> Option<(u32, String, String, String, String)> {
    let mut b = data.get(2..102)?;
    let id = b.get_u32_le();
    Some((id, get_fixed_str(&mut b, 24), get_fixed_str(&mut b, 24), get_fixed_str(&mut b, 24), get_fixed_str(&mut b, 24)))
}

/// CZ_REQ_STYLE_CHANGE2 (indices; 0 unchanged) / CZ_REQ_STYLE_CLOSE.
#[derive(Debug, Clone, PartialEq)]
pub enum StyleRequest {
    Buy { hair_color: i16, hair: i16, clothes_color: i16 },
    Close,
}

impl Encode for StyleRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            StyleRequest::Buy { hair_color, hair, clothes_color } => {
                d.put_u16_le(0x0afc);
                for v in [*hair_color, *hair, *clothes_color, 0, 0, 0, 0] {
                    d.put_i16_le(v);
                }
            }
            StyleRequest::Close => d.put_u16_le(0x0a48),
        }
    }
}

/// CZ_REQ_RANKING: 0 blacksmith, 1 alchemist, 2 taekwon, 3 PK.
pub struct ReqRanking(pub u16);

impl Encode for ReqRanking {
    fn encode(&self, d: &mut BytesMut) {
        d.put_u16_le(0x097c);
        d.put_u16_le(self.0);
    }
}

/// ZC_ACK_RANKING2 (0x0af6): (type, (char id, points) x10, our points).
pub fn parse_ranking(data: &[u8]) -> Option<(u16, Vec<(u32, u32)>, u32)> {
    let mut b = data.get(2..88)?;
    let kind = b.get_u16_le();
    let cids: Vec<u32> = (0..10).map(|_| b.get_u32_le()).collect();
    let pts: Vec<u32> = (0..10).map(|_| b.get_u32_le()).collect();
    let mine = b.get_u32_le();
    Some((kind, cids.into_iter().zip(pts).filter(|(c, _)| *c != 0).collect(), mine))
}

/// Server-side options: CZ_CONFIG (type, on), CZ_PARTY_CONFIG (deny invites), and
/// CZ_PARTY_CHANGE_OPTION (exp share, pickup share, item share; leader only).
#[derive(Debug, Clone, PartialEq)]
pub enum OptionRequest {
    Config { kind: u32, on: bool },
    DenyPartyInvites(bool),
    PartyOptions { exp_share: bool, pickup_share: bool, item_share: bool },
}

impl Encode for OptionRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            OptionRequest::Config { kind, on } => {
                d.put_u16_le(0x02d8);
                d.put_u32_le(*kind);
                d.put_u32_le(*on as u32);
            }
            OptionRequest::DenyPartyInvites(deny) => {
                d.put_u16_le(0x02c8);
                d.put_u8(*deny as u8);
            }
            OptionRequest::PartyOptions { exp_share, pickup_share, item_share } => {
                d.put_u16_le(0x07d7);
                d.put_u32_le(*exp_share as u32);
                d.put_u8(*pickup_share as u8);
                d.put_u8(*item_share as u8);
            }
        }
    }
}

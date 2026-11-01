//! Packets needed for login -> char select -> map entry at PACKETVER 20250402.

use bytes::{Buf, BufMut, BytesMut};
use encoding_rs::EUC_KR;

use crate::{codec::Encode, table::client_packet};

pub fn put_fixed_str(dst: &mut BytesMut, s: &str, n: usize) {
    let (b, _, _) = EUC_KR.encode(s);
    let k = b.len().min(n - 1);
    dst.put_slice(&b[..k]);
    dst.put_bytes(0, n - k);
}

pub fn get_fixed_str(src: &mut &[u8], n: usize) -> String {
    let raw = &src[..n];
    let end = raw.iter().position(|&c| c == 0).unwrap_or(n);
    let (s, _, _) = EUC_KR.decode(&raw[..end]);
    let s = s.into_owned();
    src.advance(n);
    s
}

fn ipv4(ip: u32) -> std::net::Ipv4Addr {
    // rAthena stores the address in network order inside a little-endian u32.
    std::net::Ipv4Addr::from(ip.to_le_bytes())
}

// ---------------------------------------------------------------- client -> server

pub struct CaLogin<'a> {
    pub version: u32,
    pub username: &'a str,
    pub password: &'a str,
    pub clienttype: u8,
}

impl Encode for CaLogin<'_> {
    fn encode(&self, d: &mut BytesMut) {
        d.put_u16_le(0x0064);
        d.put_u32_le(self.version);
        put_fixed_str(d, self.username, 24);
        put_fixed_str(d, self.password, 24);
        d.put_u8(self.clienttype);
    }
}

pub struct ChEnter {
    pub account_id: u32,
    pub login_id1: u32,
    pub login_id2: u32,
    pub sex: u8,
}

impl Encode for ChEnter {
    fn encode(&self, d: &mut BytesMut) {
        d.put_u16_le(0x0065);
        d.put_u32_le(self.account_id);
        d.put_u32_le(self.login_id1);
        d.put_u32_le(self.login_id2);
        d.put_u16_le(0);
        d.put_u8(self.sex);
    }
}

pub struct ChCharlistReq;

impl Encode for ChCharlistReq {
    fn encode(&self, d: &mut BytesMut) {
        d.put_u16_le(0x09a1);
    }
}

/// A PIN code answer to HC_SECOND_PASSWD_LOGIN's prompt.
#[derive(Debug, Clone, PartialEq)]
pub enum PinRequest {
    /// CH_SECOND_PASSWD_ACK (0x08b8): the PIN, when asked for it.
    Check(String),
    /// CH_MAKE_SECOND_PASSWD (0x08ba): the first PIN of the account.
    Make(String),
    /// CH_EDIT_SECOND_PASSWD (0x08be): replace the PIN.
    Change { old: String, new: String },
}

/// A [`PinRequest`] scrambled with the seed of the latest prompt.
pub struct PinPacket<'a> {
    pub account_id: u32,
    pub seed: u32,
    pub req: &'a PinRequest,
}

/// The digits the server turns back into `pin`: it maps each digit d it gets to tab[d], with
/// tab a permutation of 0-9 shuffled from the seed (char_pincode_decrypt).
pub fn pin_scramble(seed: u32, pin: &str) -> [u8; 4] {
    let mut tab: [u8; 10] = std::array::from_fn(|i| i as u8);
    let mut s = seed;
    for i in 1..10u32 {
        s = 0x881234u32.wrapping_add(s.wrapping_mul(0x3498));
        tab.swap(i as usize, (s % (i + 1)) as usize);
    }
    let mut out = [b'0'; 4];
    for (o, c) in out.iter_mut().zip(pin.bytes()) {
        let want = c.wrapping_sub(b'0');
        *o = b'0' + tab.iter().position(|&t| t == want).unwrap_or(0) as u8;
    }
    out
}

impl Encode for PinPacket<'_> {
    fn encode(&self, d: &mut BytesMut) {
        let (op, pins): (u16, Vec<&str>) = match self.req {
            PinRequest::Check(p) => (0x08b8, vec![p]),
            PinRequest::Make(p) => (0x08ba, vec![p]),
            PinRequest::Change { old, new } => (0x08be, vec![old, new]),
        };
        d.put_u16_le(op);
        d.put_u32_le(self.account_id);
        for p in pins {
            d.put_slice(&pin_scramble(self.seed, p));
        }
    }
}

pub struct ChSelectChar {
    pub slot: u8,
}

impl Encode for ChSelectChar {
    fn encode(&self, d: &mut BytesMut) {
        d.put_u16_le(0x0066);
        d.put_u8(self.slot);
    }
}

/// Char-server keepalive.
pub struct ChPing {
    pub account_id: u32,
}

impl Encode for ChPing {
    fn encode(&self, d: &mut BytesMut) {
        d.put_u16_le(0x0187);
        d.put_u32_le(self.account_id);
    }
}

/// A fixed-length map-server packet laid out from the generated packet db: opcode and
/// field offsets come from the handler's current registration, so shuffled
/// opcodes and padding are handled by the table rather than by hand.
pub(crate) fn put_db_packet(d: &mut BytesMut, handler: &str, fields: &[&[u8]]) {
    let p = client_packet(handler);
    assert!(p.len > 0, "{handler} is variable-length");
    assert_eq!(p.pos.len(), fields.len(), "{handler}: field count mismatch");
    let start = d.len();
    d.put_bytes(0, p.len as usize);
    let buf = &mut d[start..];
    buf[..2].copy_from_slice(&p.opcode.to_le_bytes());
    for (&off, f) in p.pos.iter().zip(fields) {
        buf[off as usize..off as usize + f.len()].copy_from_slice(f);
    }
}

/// As `Bare`, but by rAthena handler, from the packet db (zero-padded to its length):
/// CZ_CANCEL_LOCKON ("StopAttack"), CZ_DORIDORI ("NoviceDoriDori"), CZ_CHOPOKGI
/// ("NoviceExplosionSpirits"), /changedress ("changedress"), /resetskill ("reset_skill").
pub struct BareDb(pub &'static str);

impl Encode for BareDb {
    fn encode(&self, d: &mut BytesMut) {
        put_db_packet(d, self.0, &[&[]]);
    }
}

/// CZ_CHANGE_DIRECTION ("ChangeDir"): turn in place. `head` 0 straight, 1 / 2 turned
/// right / left; `dir` the body's direction as the server counts it (see `server_dir`).
pub struct CzChangeDir {
    pub head: u8,
    pub dir: u8,
}

impl Encode for CzChangeDir {
    fn encode(&self, d: &mut BytesMut) {
        put_db_packet(d, "ChangeDir", &[&[self.head], &[self.dir]]);
    }
}

/// CZ_ENTER
pub struct CzEnter {
    pub account_id: u32,
    pub char_id: u32,
    pub login_id1: u32,
    pub client_tick: u32,
    pub sex: u8,
}

impl Encode for CzEnter {
    fn encode(&self, d: &mut BytesMut) {
        put_db_packet(
            d,
            "WantToConnection",
            &[
                &self.account_id.to_le_bytes(),
                &self.char_id.to_le_bytes(),
                &self.login_id1.to_le_bytes(),
                &self.client_tick.to_le_bytes(),
                &[self.sex],
            ],
        );
    }
}

/// A packet that is only its opcode: CZ_PROGRESS (0x02f1, a script progress bar finished),
/// CZ_PING_LIVE (0x0b1c, the answer to ZC_PING_LIVE), CZ_NOTIFY_ACTORINIT (0x0447).
pub struct Bare(pub u16);

impl Encode for Bare {
    fn encode(&self, d: &mut BytesMut) {
        d.put_u16_le(self.0);
    }
}

/// CZ_VIEW_CAMERAINFO: /viewpointvalue (show 1) or /setcamera <range> <rotation> <latitude>;
/// the server answers through @camerainfo.
pub struct CameraInfo {
    pub show: bool,
    pub range: f32,
    pub rotation: f32,
    pub latitude: f32,
}

impl Encode for CameraInfo {
    fn encode(&self, d: &mut BytesMut) {
        d.put_u16_le(0x0a77);
        d.put_u8(if self.show { 1 } else { 0 });
        d.put_f32_le(self.range);
        d.put_f32_le(self.rotation);
        d.put_f32_le(self.latitude);
    }
}

/// Adoption: a married couple asks a player to become their child (CZ_REQ_JOIN_BABY), who
/// answers (CZ_JOIN_BABY) naming both parents.
#[derive(Debug, Clone, PartialEq)]
pub enum AdoptRequest {
    Ask(u32),
    Answer { father: u32, mother: u32, accept: bool },
}

impl Encode for AdoptRequest {
    fn encode(&self, d: &mut BytesMut) {
        match *self {
            AdoptRequest::Ask(target) => {
                d.put_u16_le(0x01f9);
                d.put_u32_le(target);
            }
            AdoptRequest::Answer { father, mother, accept } => {
                d.put_u16_le(0x01f7);
                d.put_u32_le(father);
                d.put_u32_le(mother);
                d.put_u32_le(accept as u32);
            }
        }
    }
}

/// A GM's macro reporter: list the players around a cell (CZ_REQ_PLAYER_AID_IN_RANGE), then
/// put one under the macro detector (CZ_REQ_APPLY_MACRO_DETECTOR).
#[derive(Debug, Clone, PartialEq)]
pub enum MacroReport {
    Scan { x: u16, y: u16, radius: u8 },
    Apply(u32),
}

impl Encode for MacroReport {
    fn encode(&self, d: &mut BytesMut) {
        match *self {
            MacroReport::Scan { x, y, radius } => {
                d.put_u16_le(0x0a6c);
                d.put_u16_le(x);
                d.put_u16_le(y);
                d.put_u8(radius);
            }
            MacroReport::Apply(aid) => {
                d.put_u16_le(0x0a56);
                d.put_u32_le(aid);
            }
        }
    }
}

/// CZ_AGREE_STARPLACE: yes to Feeling's "save this map" (0 sun, 1 moon, 2 stars).
pub struct StarPlace(pub u8);

impl Encode for StarPlace {
    fn encode(&self, d: &mut BytesMut) {
        d.put_u16_le(0x0254);
        d.put_u8(self.0);
    }
}

pub struct CzLoadEndAck;

impl Encode for CzLoadEndAck {
    fn encode(&self, d: &mut BytesMut) {
        d.put_u16_le(client_packet("LoadEndAck").opcode);
    }
}

/// CZ_REQUEST_TIME: map-server keepalive.
pub struct CzRequestTime {
    pub client_tick: u32,
}

impl Encode for CzRequestTime {
    fn encode(&self, d: &mut BytesMut) {
        put_db_packet(d, "TickSend", &[&self.client_tick.to_le_bytes()]);
    }
}

/// CZ_REQUEST_MOVE
pub struct CzRequestMove {
    pub x: u16,
    pub y: u16,
    pub dir: u8,
}

impl Encode for CzRequestMove {
    fn encode(&self, d: &mut BytesMut) {
        put_db_packet(d, "WalkToXY", &[&pack_pos(self.x, self.y, self.dir)]);
    }
}

/// Packs x:10, y:10, dir:4 into 3 bytes (rAthena WBUFPOS).
pub fn pack_pos(x: u16, y: u16, dir: u8) -> [u8; 3] {
    [(x >> 2) as u8, (((x & 3) << 6) | (y >> 4)) as u8, (((y & 0xf) << 4) as u8) | (dir & 0xf)]
}

// ---------------------------------------------------------------- server -> client

#[derive(Debug, Clone)]
pub struct CharServerInfo {
    pub addr: std::net::SocketAddrV4,
    pub name: String,
    pub users: u16,
}

#[derive(Debug, Clone)]
pub struct AcceptLogin {
    pub login_id1: u32,
    pub account_id: u32,
    pub login_id2: u32,
    pub sex: u8,
    /// The web server's session token (AuthToken), refreshed each login.
    pub web_token: String,
    pub servers: Vec<CharServerInfo>,
}

/// 0x0ac4
pub fn parse_accept_login(data: &[u8]) -> AcceptLogin {
    let mut b = &data[4..];
    let login_id1 = b.get_u32_le();
    let account_id = b.get_u32_le();
    let login_id2 = b.get_u32_le();
    let _last_ip = b.get_u32_le();
    b.advance(26);
    let sex = b.get_u8();
    let web_token = get_fixed_str(&mut b, 17);
    let mut servers = Vec::new();
    while b.len() >= 160 {
        let ip = b.get_u32_le();
        let port = b.get_u16_le();
        let name = get_fixed_str(&mut b, 20);
        let users = b.get_u16_le();
        b.advance(2 + 2 + 128);
        servers.push(CharServerInfo { addr: std::net::SocketAddrV4::new(ipv4(ip), port), name, users });
    }
    AcceptLogin { login_id1, account_id, login_id2, sex, web_token, servers }
}

/// 0x083e error code.
pub fn parse_refuse_login(data: &[u8]) -> u32 {
    (&data[2..]).get_u32_le()
}

/// CHARACTER_INFO, 175 bytes at this PACKETVER.
pub const CHARACTER_INFO_LEN: usize = 175;

#[derive(Debug, Clone)]
pub struct CharInfo {
    pub char_id: u32,
    pub base_exp: i64,
    pub zeny: i32,
    pub job_level: i32,
    pub hp: i64,
    pub max_hp: i64,
    pub sp: i64,
    pub max_sp: i64,
    pub job: i16,
    pub head: i16,
    pub base_level: i16,
    /// Milliseconds per cell.
    pub speed: i16,
    pub look: Look,
    pub name: String,
    pub slot: u8,
    pub hair_color: u8,
    /// Body dye (bodypalette): the clothes colour, 0 for none.
    pub clothes_color: i16,
    pub map: String,
    pub sex: u8,
    /// Seconds until a pending deletion can be confirmed; 0 when none is pending.
    pub delete_date: u32,
    /// The server lets this character be renamed (chr_name_changeCnt).
    pub can_rename: bool,
    /// Slot moves left (chr_slot_changeCnt; 0: moving is off).
    pub moves: u32,
}

pub fn parse_char_info(mut b: &[u8]) -> CharInfo {
    let char_id = b.get_u32_le();
    let base_exp = b.get_i64_le();
    let zeny = b.get_i32_le();
    let _job_exp = b.get_i64_le();
    let job_level = b.get_i32_le();
    b.advance(4 * 5 + 2); // body/health/effect state, virtue, honor, jobpoint
    let hp = b.get_i64_le();
    let max_hp = b.get_i64_le();
    let sp = b.get_i64_le();
    let max_sp = b.get_i64_le();
    let speed = b.get_i16_le();
    let job = b.get_i16_le();
    let head = b.get_i16_le();
    let _body = b.get_i16_le();
    let weapon = b.get_i16_le();
    let base_level = b.get_i16_le();
    let _sppoint = b.get_i16_le();
    let head_bottom = b.get_i16_le() as u16;
    let shield = b.get_i16_le() as u16 as u32;
    let head_top = b.get_i16_le() as u16;
    let head_mid = b.get_i16_le() as u16;
    b.advance(2); // head palette (hair_color, again)
    let clothes_color = b.get_i16_le();
    let look = Look { weapon: weapon as u16 as u32, shield, head_top, head_mid, head_bottom };
    let name = get_fixed_str(&mut b, 24);
    b.advance(6); // stats
    let slot = b.get_u8();
    let hair_color = b.get_u8();
    b.advance(2);
    let map = get_fixed_str(&mut b, 16);
    let delete_date = b.get_u32_le();
    b.advance(4); // robe palette
    let moves = b.get_u32_le();
    let can_rename = b.get_u32_le() != 0;
    let sex = b.get_u8();
    CharInfo { char_id, base_exp, zeny, job_level, hp, max_hp, sp, max_sp, job, head, base_level, speed, look, name, slot, hair_color, clothes_color, map, sex, delete_date, can_rename, moves }
}

/// 0x006b: 3 + 20 bytes of header data, then characters.
pub fn parse_hc_accept_enter(data: &[u8]) -> Vec<CharInfo> {
    data[4 + 23..].chunks_exact(CHARACTER_INFO_LEN).map(parse_char_info).collect()
}

/// HC_ACK_CHARINFO_PER_PAGE
pub fn parse_charinfo_page(data: &[u8]) -> Vec<CharInfo> {
    data[4..].chunks_exact(CHARACTER_INFO_LEN).map(parse_char_info).collect()
}

#[derive(Debug, Clone)]
pub struct ZoneServer {
    pub char_id: u32,
    pub map: String,
    pub addr: std::net::SocketAddrV4,
}

/// 0x0ac5
pub fn parse_notify_zonesvr(data: &[u8]) -> ZoneServer {
    let mut b = &data[2..];
    let char_id = b.get_u32_le();
    let map = get_fixed_str(&mut b, 16);
    let ip = b.get_u32_le();
    let port = b.get_u16_le();
    ZoneServer { char_id, map, addr: std::net::SocketAddrV4::new(ipv4(ip), port) }
}

#[derive(Debug, Clone, Copy)]
pub struct AcceptEnter {
    pub start_time: u32,
    pub x: u16,
    pub y: u16,
    pub dir: u8,
    /// Our name font (ZC_ACCEPT_ENTER2/3), 0 when not sent.
    pub font: u16,
}

/// 0x02eb / 0x0a18. posDir packs x:10, y:10, dir:4; the font follows the two size bytes.
pub fn parse_accept_enter(data: &[u8]) -> AcceptEnter {
    let mut b = &data[2..];
    let start_time = b.get_u32_le();
    let p = [b[0] as u16, b[1] as u16, b[2] as u16];
    AcceptEnter {
        start_time,
        x: (p[0] << 2) | (p[1] >> 6),
        y: ((p[1] & 0x3f) << 4) | (p[2] >> 4),
        dir: server_dir((p[2] & 0x0f) as u8),
        font: if data.len() >= 13 { u16::from_le_bytes([data[11], data[12]]) } else { 0 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_requests_are_their_opcode() {
        let enc = |h| {
            let mut b = BytesMut::new();
            BareDb(h).encode(&mut b);
            b.to_vec()
        };
        assert_eq!(enc("StopAttack"), [0x18, 0x01]);
        assert_eq!(enc("reset_skill"), [0xb1, 0x0b, 0]);
        let mut b = BytesMut::new();
        CzChangeDir { head: 2, dir: 6 }.encode(&mut b);
        assert_eq!(b.to_vec(), [0x61, 0x03, 2, 0, 6]);
    }

    #[test]
    fn sizes() {
        let mut b = BytesMut::new();
        CaLogin { version: 0, username: "a", password: "b", clienttype: 0 }.encode(&mut b);
        assert_eq!(b.len(), 55);
        b.clear();
        ChEnter { account_id: 0, login_id1: 0, login_id2: 0, sex: 0 }.encode(&mut b);
        assert_eq!(b.len(), 17);
        b.clear();
        CzEnter { account_id: 1, char_id: 2, login_id1: 3, client_tick: 4, sex: 1 }.encode(&mut b);
        assert_eq!(b.len(), 23);
        assert_eq!(&b[..6], &[0x36, 0x04, 1, 0, 0, 0]);
        assert_eq!(b[22], 1);
        b.clear();
        CzRequestTime { client_tick: 7 }.encode(&mut b);
        assert_eq!(&b[..], &[0x60, 0x03, 7, 0, 0, 0]);
    }

    #[test]
    fn pos_dir() {
        let p = pack_pos(150, 200, 3);
        let mut data = vec![0xeb, 0x02, 0, 0, 0, 0];
        data.extend_from_slice(&p);
        data.extend_from_slice(&[5, 5, 0, 0]);
        let a = parse_accept_enter(&data);
        // Server direction 3 (south-west) is sprite direction 1.
        assert_eq!((a.x, a.y, a.dir), (150, 200, 1));
    }

    #[test]
    fn unit_level() {
        // packet_idle_unit, packet_spawn_unit (no state byte) and packet_unit_walking.
        for (opcode, len, clevel_at, name_at) in [(0x09ffu16, 108, 69, 84), (0x09fe, 107, 68, 83), (0x09fd, 114, 75, 90)] {
            let mut d = vec![0u8; len];
            d[..2].copy_from_slice(&opcode.to_le_bytes());
            d[clevel_at..clevel_at + 2].copy_from_slice(&175u16.to_le_bytes());
            d[name_at..name_at + 6].copy_from_slice(b"Hayate");
            let u = parse_unit(opcode, &d).unwrap();
            assert_eq!((u.level, u.name.as_str()), (175, "Hayate"), "{opcode:#06x}");
        }
    }
}

// ---------------------------------------------------------------- map: movement

/// Unpacks rAthena's 6-byte MoveData (WBUFPOS2): source and destination cells.
pub fn unpack_move(p: &[u8]) -> ((u16, u16), (u16, u16)) {
    let p: [u16; 5] = [p[0] as u16, p[1] as u16, p[2] as u16, p[3] as u16, p[4] as u16];
    let x0 = (p[0] << 2) | (p[1] >> 6);
    let y0 = ((p[1] & 0x3f) << 4) | (p[2] >> 4);
    let x1 = ((p[2] & 0x0f) << 6) | (p[3] >> 2);
    let y1 = ((p[3] & 0x03) << 8) | p[4];
    ((x0, y0), (x1, y1))
}

/// ZC_NOTIFY_PLAYERMOVE: our own walk was accepted. Returns (start tick, from, to).
pub fn parse_player_move(data: &[u8]) -> (u32, (u16, u16), (u16, u16)) {
    let tick = (&data[2..]).get_u32_le();
    let (from, to) = unpack_move(&data[6..12]);
    (tick, from, to)
}

/// ZC_STOPMOVE: (account/unit id, x, y).
pub fn parse_stop_move(data: &[u8]) -> (u32, u16, u16) {
    let mut b = &data[2..];
    (b.get_u32_le(), b.get_u16_le(), b.get_u16_le())
}

/// ZC_NPCACK_MAPMOVE: warp to (map, x, y) on the same map server.
pub fn parse_map_move(data: &[u8]) -> (String, u16, u16) {
    let mut b = &data[2..];
    let map = get_fixed_str(&mut b, 16);
    (map, b.get_u16_le(), b.get_u16_le())
}

#[cfg(test)]
mod move_tests {
    use super::*;

    #[test]
    fn unpacks_wbufpos2() {
        // Mirror of rAthena WBUFPOS2 for (156,191) -> (160,185).
        let (x0, y0, x1, y1) = (156u16, 191u16, 160u16, 185u16);
        let p = [
            (x0 >> 2) as u8,
            ((x0 << 6) | ((y0 >> 4) & 0x3f)) as u8,
            ((y0 << 4) | ((x1 >> 6) & 0x0f)) as u8,
            ((x1 << 2) | ((y1 >> 8) & 0x03)) as u8,
            y1 as u8,
            0x88,
        ];
        assert_eq!(unpack_move(&p), ((156, 191), (160, 185)));
    }
}

// ---------------------------------------------------------------- map: other units

/// Where a unit is: standing at a cell, or walking between two.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnitPos {
    At { x: u16, y: u16, dir: u8 },
    Walking { from: (u16, u16), to: (u16, u16) },
}

/// A player, NPC, monster, pet, ... entering view.
#[derive(Debug, Clone)]
pub struct UnitInfo {
    /// 0 player, 1 NPC, 5 monster, 6 event NPC, 7 pet, 8 homunculus, ...
    pub object_type: u8,
    pub id: u32,
    pub speed: i16,
    /// Effect state (OPTION_* flags: 0x2 hiding, 0x4 cloaking, 0x40 invisible, ...).
    pub option: u32,
    /// Class: job id for players, sprite class for everything else.
    pub job: i16,
    pub hair: u16,
    pub hair_color: i16,
    /// Body dye: the clothes colour, 0 for none.
    pub clothes_color: i16,
    pub look: Look,
    pub sex: u8,
    pub name: String,
    pub pos: UnitPos,
    /// Guild id (0 none) and its emblem version.
    pub guild: u32,
    pub emblem: u16,
    /// Base level as the server reports it (clif_setlevel); players at 99 or more show
    /// the level aura. NPCs and pets report 0.
    pub level: u16,
    /// Name font (players; 0 for others). Tenseiga reads its GM look from it (see gm_look).
    pub font: u16,
    /// Head turned from the body: 0 straight, 1 right, 2 left (players).
    pub head_dir: u8,
}

/// ZC packets packet_idle_unit (0x09ff), packet_spawn_unit (0x09fe) and
/// packet_unit_walking (0x09fd) at PACKETVER 20250402 (see packets_struct.hpp).
pub fn parse_unit(opcode: u16, data: &[u8]) -> Option<UnitInfo> {
    let (pos_at, name_at, sex_at, walking) = match opcode {
        0x09ff => (63, 84, 62, false),
        0x09fe => (63, 83, 62, false),
        0x09fd => (67, 90, 66, true),
        _ => return None,
    };
    if data.len() < name_at {
        return None;
    }
    let u16_at = |o: usize| u16::from_le_bytes([data[o], data[o + 1]]);
    let i16_at = |o: usize| i16::from_le_bytes([data[o], data[o + 1]]);
    let u32_at = |o: usize| u32::from_le_bytes(data[o..o + 4].try_into().unwrap());
    // headpalette moves 4 bytes later in the walking packet (moveStartTime precedes it).
    let palette_at = if walking { 45 } else { 41 };
    let pos = if walking {
        let (from, to) = unpack_move(&data[pos_at..pos_at + 6]);
        UnitPos::Walking { from, to }
    } else {
        let a = parse_accept_enter(&[&[0u8; 6][..], &data[pos_at..pos_at + 3]].concat());
        UnitPos::At { x: a.x, y: a.y, dir: a.dir }
    };
    let mut name = &data[name_at.min(data.len())..];
    let name = if name.len() >= 24 { get_fixed_str(&mut name, 24) } else { String::new() };
    Some(UnitInfo {
        object_type: data[4],
        // AID: the unit id used by vanish/move packets (GID is a char id, 0 for NPCs).
        id: u32_at(5),
        speed: i16_at(13),
        option: u32_at(19),
        job: i16_at(23),
        hair: u16_at(25),
        hair_color: i16_at(palette_at),
        clothes_color: i16_at(palette_at + 2),
        look: Look {
            weapon: u32_at(27),
            shield: u32_at(31),
            head_bottom: u16_at(35),
            head_top: u16_at(if walking { 41 } else { 37 }),
            head_mid: u16_at(if walking { 43 } else { 39 }),
        },
        sex: data[sex_at],
        name,
        pos,
        guild: if data.len() >= palette_at + 14 { u32_at(palette_at + 8) } else { 0 },
        emblem: if data.len() >= palette_at + 14 { u16_at(palette_at + 12) } else { 0 },
        // clevel sits 15 bytes before the name (font, maxHP, HP, isBoss and body follow it).
        level: u16_at(name_at - 15),
        font: u16_at(name_at - 13),
        // headDir follows the two palettes.
        head_dir: data[palette_at + 4],
    })
}

/// ZC_NOTIFY_VANISH: (unit id, reason: 0 out of sight, 1 died, 2 logged out, 3 teleported).
pub fn parse_vanish(data: &[u8]) -> (u32, u8) {
    ((&data[2..]).get_u32_le(), data[6])
}

/// ZC_NPCACK_SERVERMOVE: warp to (map, x, y) on another map server.
pub fn parse_server_move(data: &[u8]) -> (ZoneServer, u16, u16) {
    let mut b = &data[2..];
    let map = get_fixed_str(&mut b, 16);
    let (x, y) = (b.get_u16_le(), b.get_u16_le());
    let ip = b.get_u32_le();
    let port = b.get_u16_le();
    (ZoneServer { char_id: 0, map, addr: std::net::SocketAddrV4::new(ipv4(ip), port) }, x, y)
}

// ---------------------------------------------------------------- map: status and chat

/// rAthena status ids (`SP_*` in map.hpp) the HUD shows.
pub mod sp {
    pub const BASEEXP: u16 = 1;
    pub const JOBEXP: u16 = 2;
    pub const HP: u16 = 5;
    pub const MAXHP: u16 = 6;
    pub const SP: u16 = 7;
    pub const MAXSP: u16 = 8;
    pub const BASELEVEL: u16 = 11;
    pub const ZENY: u16 = 20;
    pub const NEXTBASEEXP: u16 = 22;
    pub const NEXTJOBEXP: u16 = 23;
    pub const WEIGHT: u16 = 24;
    pub const MAXWEIGHT: u16 = 25;
    pub const JOBLEVEL: u16 = 55;
}

/// ZC_PAR_CHANGE (0x00b0) / ZC_LONGPAR_CHANGE (0x00b1) / ZC_LONGLONGPAR_CHANGE (0x0acb):
/// (status id, value).
pub fn parse_par_change(opcode: u16, data: &[u8]) -> Option<(u16, i64)> {
    let mut b = &data[2..];
    let var = b.get_u16_le();
    let value = match opcode {
        0x00b0 => b.get_i32_le() as i64,
        0x00b1 => b.get_u32_le() as i64,
        0x0acb => b.get_i64_le(),
        _ => return None,
    };
    Some((var, value))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatKind {
    /// Someone nearby (or us) talking: "Name : text".
    Public,
    Npc,
    Whisper,
    Broadcast,
    System,
    Party,
    Guild,
    Clan,
    Battle,
}

/// Text after the fixed header of a variable packet, without trailing NULs.
fn tail_text(data: &[u8], from: usize) -> String {
    let raw = data.get(from..).unwrap_or_default();
    let end = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
    EUC_KR.decode(&raw[..end]).0.into_owned()
}

/// Chat-like packets: (kind, speaker unit id if any, text).
pub fn parse_chat(opcode: u16, data: &[u8]) -> Option<(ChatKind, Option<u32>, String)> {
    let id_at = |o: usize| u32::from_le_bytes(data[o..o + 4].try_into().unwrap());
    Some(match opcode {
        0x008d => (ChatKind::Public, Some(id_at(4)), tail_text(data, 8)), // ZC_NOTIFY_CHAT
        0x008e => (ChatKind::Public, None, tail_text(data, 4)),           // ZC_NOTIFY_PLAYERCHAT (ours)
        0x02c1 => (ChatKind::Npc, Some(id_at(4)), tail_text(data, 12)),   // ZC_NPC_CHAT
        0x09de => {
            // ZC_WHISPER: senderGID, sender[24], isAdmin, message
            let mut b = &data[8..];
            let sender = get_fixed_str(&mut b, 24);
            (ChatKind::Whisper, None, format!("(From {sender}) : {}", tail_text(data, 33)))
        }
        0x009a => {
            // ZC_BROADCAST: an optional 4-char colour tag ("blue", "ssss", "micc").
            let text = tail_text(data, 4);
            let text = match text.get(..4) {
                Some("blue" | "ssss" | "micc") => text[4..].to_string(),
                _ => text,
            };
            (ChatKind::Broadcast, None, text)
        }
        0x01c3 => (ChatKind::Broadcast, None, tail_text(data, 16)), // ZC_BROADCAST2
        _ => return None,
    })
}

/// The RGB colour a chat-like packet asks for, if it carries one. ZC_NPC_CHAT's colour is
/// stored blue-green-red in a u32 (the server swaps it), so its bytes read red, green, blue;
/// ZC_BROADCAST2 sends the script's 0xRRGGBB as is, so its bytes read blue, green, red.
pub fn chat_colour(opcode: u16, data: &[u8]) -> Option<[u8; 3]> {
    match opcode {
        0x02c1 => data.get(8..11).map(|c| [c[0], c[1], c[2]]),
        0x01c3 => data.get(4..7).map(|c| [c[2], c[1], c[0]]),
        // ZC_BROADCAST's "blue" tag.
        0x009a if data.get(4..8) == Some(b"blue") => Some([0, 255, 255]),
        _ => None,
    }
}

/// CZ_REQUEST_CHAT (GlobalMessage): the client sends "Name : text".
pub struct CzChat<'a> {
    pub name: &'a str,
    pub text: &'a str,
}

impl Encode for CzChat<'_> {
    fn encode(&self, d: &mut BytesMut) {
        let line = format!("{} : {}", self.name, self.text);
        let (msg, _, _) = EUC_KR.encode(&line);
        d.put_u16_le(client_packet("GlobalMessage").opcode);
        d.put_u16_le((4 + msg.len() + 1) as u16);
        d.put_slice(&msg);
        d.put_u8(0);
    }
}

/// CZ_WHISPER (WisMessage): a private message to a character by name.
pub struct CzWhisper<'a> {
    pub to: &'a str,
    pub text: &'a str,
}

impl Encode for CzWhisper<'_> {
    fn encode(&self, d: &mut BytesMut) {
        let (to, _, _) = EUC_KR.encode(self.to);
        let (msg, _, _) = EUC_KR.encode(self.text);
        d.put_u16_le(client_packet("WisMessage").opcode);
        d.put_u16_le((28 + msg.len() + 1) as u16);
        let mut name = [0u8; 24];
        let n = to.len().min(23);
        name[..n].copy_from_slice(&to[..n]);
        d.put_slice(&name);
        d.put_slice(&msg);
        d.put_u8(0);
    }
}

/// CZ_REQ_EMOTION: show an emote over our head.
pub struct CzEmotion(pub u8);

impl Encode for CzEmotion {
    fn encode(&self, d: &mut BytesMut) {
        d.put_u16_le(0x00bf);
        d.put_u8(self.0);
    }
}

#[cfg(test)]
mod chat_tests {
    use super::*;

    #[test]
    fn chat_roundtrip_shapes() {
        let mut b = BytesMut::new();
        CzChat { name: "Tester", text: "hi" }.encode(&mut b);
        assert_eq!(u16::from_le_bytes([b[2], b[3]]) as usize, b.len());
        assert_eq!(&b[4..b.len() - 1], b"Tester : hi");

        let mut p = vec![0x8e, 0x00, 0, 0];
        p.extend_from_slice(b"Tester : hi\0");
        let n = p.len() as u16;
        p[2..4].copy_from_slice(&n.to_le_bytes());
        assert_eq!(parse_chat(0x008e, &p), Some((ChatKind::Public, None, "Tester : hi".into())));
    }

    #[test]
    fn script_colours_read_as_rgb() {
        // npctalk "...", ..., 0xFFAA66: the server swaps it to blue-green-red, 0x0066AAFF.
        let mut npc = vec![0xc1, 0x02, 0, 0, 1, 0, 0, 0];
        npc.extend_from_slice(&0x0066AAFFu32.to_le_bytes());
        assert_eq!(chat_colour(0x02c1, &npc), Some([0xFF, 0xAA, 0x66]));
        // mapannounce ..., "0xFFAA66": sent as is.
        let mut bc = vec![0xc3, 0x01, 0, 0];
        bc.extend_from_slice(&0x00FFAA66u32.to_le_bytes());
        assert_eq!(chat_colour(0x01c3, &bc), Some([0xFF, 0xAA, 0x66]));
        assert_eq!(chat_colour(0x008d, &bc), None);
    }
}

// ---------------------------------------------------------------- map: NPC dialogs

/// What an NPC script asks the client to show.
#[derive(Debug, Clone, PartialEq)]
pub enum NpcDialog {
    /// Append a line of text (may contain ^RRGGBB colour codes).
    Say { npc: u32, text: String },
    /// Show a Next button.
    Next { npc: u32 },
    /// Show a Close button.
    Close { npc: u32 },
    /// Choose one option; reply with its 1-based index (255 cancels).
    Menu { npc: u32, options: Vec<String> },
    InputNumber { npc: u32 },
    InputText { npc: u32 },
    /// Clear the text so far (`clear` in scripts).
    Clear { npc: u32 },
    /// Show (or with `kind` 255, hide) an illustration from texture\유저인터페이스\illust.
    Cutin { image: String, kind: u8 },
    /// Text alignment (`setdialogalign`): 0 left, 1 right, 2 centre, 3 top, 4 middle, 5 bottom.
    Align(u8),
    /// Window size in pixels (`setdialogsize`).
    Size { width: i32, height: i32 },
    /// Window top-left in pixels (`setdialogpos`).
    Pos { x: i32, y: i32 },
    /// Window centre as a percentage of the screen (`setdialogpospercent`).
    PosPercent { x: i32, y: i32 },
}

pub fn parse_npc_dialog(opcode: u16, data: &[u8]) -> Option<NpcDialog> {
    let npc_at = |o: usize| u32::from_le_bytes(data[o..o + 4].try_into().unwrap());
    Some(match opcode {
        0x00b4 => NpcDialog::Say { npc: npc_at(4), text: tail_text(data, 8) },
        0x00b5 => NpcDialog::Next { npc: npc_at(2) },
        0x00b6 => NpcDialog::Close { npc: npc_at(2) },
        0x00b7 => NpcDialog::Menu { npc: npc_at(4), options: tail_text(data, 8).split(':').map(str::to_string).collect() },
        0x0142 => NpcDialog::InputNumber { npc: npc_at(2) },
        0x01d4 => NpcDialog::InputText { npc: npc_at(2) },
        0x08d6 => NpcDialog::Clear { npc: npc_at(2) },
        0x01b3 => {
            let mut b = &data[2..];
            let image = get_fixed_str(&mut b, 64);
            NpcDialog::Cutin { image, kind: data[66] }
        }
        0x0ba1 => NpcDialog::Align(data[2]),
        0x0ba2 => NpcDialog::Size { height: npc_at(2) as i32, width: npc_at(6) as i32 },
        0x0ba3 => NpcDialog::Pos { x: npc_at(2) as i32, y: npc_at(6) as i32 },
        0x0bb5 => NpcDialog::PosPercent { x: npc_at(2) as i32, y: npc_at(6) as i32 },
        _ => return None,
    })
}

/// Replies to an NPC dialog.
#[derive(Debug, Clone, PartialEq)]
pub enum NpcReply {
    /// Talk to (click) an NPC.
    Contact,
    Next,
    Close,
    /// 1-based option, or 255 to cancel.
    Menu(u8),
    Number(i32),
    Text(String),
}

pub struct CzNpc<'a> {
    pub npc: u32,
    pub reply: &'a NpcReply,
}

impl Encode for CzNpc<'_> {
    fn encode(&self, d: &mut BytesMut) {
        let id = self.npc.to_le_bytes();
        match self.reply {
            NpcReply::Contact => {
                d.put_u16_le(0x0090); // CZ_CONTACTNPC
                d.put_u32_le(self.npc);
                d.put_u8(1);
            }
            NpcReply::Next => put_db_packet(d, "NpcNextClicked", &[&id]),
            NpcReply::Menu(choice) => put_db_packet(d, "NpcSelectMenu", &[&id, &[*choice]]),
            NpcReply::Close => {
                d.put_u16_le(0x0146); // CZ_CLOSE_DIALOG
                d.put_u32_le(self.npc);
            }
            NpcReply::Number(value) => {
                d.put_u16_le(0x0143); // CZ_INPUT_EDITDLG
                d.put_u32_le(self.npc);
                d.put_i32_le(*value);
            }
            NpcReply::Text(text) => {
                let (bytes, _, _) = EUC_KR.encode(text);
                d.put_u16_le(0x01d5); // CZ_INPUT_EDITDLGSTR
                d.put_u16_le((8 + bytes.len() + 1) as u16);
                d.put_u32_le(self.npc);
                d.put_slice(&bytes);
                d.put_u8(0);
            }
        }
    }
}

#[cfg(test)]
mod npc_tests {
    use super::*;

    #[test]
    fn menu_and_replies() {
        let mut p = vec![0xb7, 0x00, 0, 0, 7, 0, 0, 0];
        p.extend_from_slice(b"Save:Storage:Cancel\0");
        let n = p.len() as u16;
        p[2..4].copy_from_slice(&n.to_le_bytes());
        assert_eq!(
            parse_npc_dialog(0x00b7, &p),
            Some(NpcDialog::Menu { npc: 7, options: vec!["Save".into(), "Storage".into(), "Cancel".into()] })
        );
        let mut b = BytesMut::new();
        CzNpc { npc: 7, reply: &NpcReply::Contact }.encode(&mut b);
        assert_eq!(&b[..], &[0x90, 0x00, 7, 0, 0, 0, 1]);
        b.clear();
        CzNpc { npc: 7, reply: &NpcReply::Menu(2) }.encode(&mut b);
        assert_eq!(b.len(), client_packet("NpcSelectMenu").len as usize);
    }
}

// ---------------------------------------------------------------- map: combat

/// ZC_NOTIFY_ACT (0x08c8): someone acted on someone (attack, sit, stand, pick up...).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Act {
    pub src: u32,
    pub target: u32,
    /// Attacker's attack motion time and the target's flinch time, in ms.
    pub src_speed: i32,
    pub dmg_speed: i32,
    pub damage: i32,
    pub hits: i16,
    /// 0 hit, 1 pick up, 2 sit, 3 stand, 4 endure, 8 multi-hit, 9 multi-hit endure,
    /// 10 critical, 11 lucky dodge.
    pub kind: u8,
    pub left_damage: i32,
}

pub fn parse_act(data: &[u8]) -> Act {
    let mut b = &data[2..];
    Act {
        src: b.get_u32_le(),
        target: b.get_u32_le(),
        src_speed: {
            b.advance(4); // server tick
            b.get_i32_le()
        },
        dmg_speed: b.get_i32_le(),
        damage: b.get_i32_le(),
        hits: {
            b.advance(1); // isSPDamage
            b.get_i16_le()
        },
        kind: b.get_u8(),
        left_damage: b.get_i32_le(),
    }
}

/// ZC_ATTACK_FAILURE_FOR_DISTANCE (0x0139): (target, target cell, our cell, attack range).
pub fn parse_attack_out_of_range(data: &[u8]) -> (u32, (u16, u16), (u16, u16), i16) {
    let mut b = &data[2..];
    let target = b.get_u32_le();
    let t = (b.get_i16_le() as u16, b.get_i16_le() as u16);
    let me = (b.get_i16_le() as u16, b.get_i16_le() as u16);
    (target, t, me, b.get_i16_le())
}

/// ZC_NOTIFY_MONSTER_HP (0x0977): (monster id, hp, max hp).
pub fn parse_monster_hp(data: &[u8]) -> (u32, i32, i32) {
    let mut b = &data[2..];
    (b.get_u32_le(), b.get_i32_le(), b.get_i32_le())
}

/// CZ_REQUEST_ACT (ActionRequest): 0 attack once, 7 keep attacking, 2 sit, 3 stand.
pub struct CzRequestAct {
    pub target: u32,
    pub action: u8,
}

impl Encode for CzRequestAct {
    fn encode(&self, d: &mut BytesMut) {
        put_db_packet(d, "ActionRequest", &[&self.target.to_le_bytes(), &[self.action]]);
    }
}

#[cfg(test)]
mod combat_tests {
    use super::*;

    #[test]
    fn notify_act_layout() {
        let mut p = vec![0xc8, 0x08];
        p.extend_from_slice(&5u32.to_le_bytes()); // src
        p.extend_from_slice(&9u32.to_le_bytes()); // target
        p.extend_from_slice(&0u32.to_le_bytes()); // tick
        p.extend_from_slice(&400i32.to_le_bytes());
        p.extend_from_slice(&300i32.to_le_bytes());
        p.extend_from_slice(&17i32.to_le_bytes()); // damage
        p.push(0);
        p.extend_from_slice(&1i16.to_le_bytes());
        p.push(10); // critical
        p.extend_from_slice(&0i32.to_le_bytes());
        assert_eq!(p.len(), 34);
        let a = parse_act(&p);
        assert_eq!((a.src, a.target, a.src_speed, a.dmg_speed, a.damage, a.hits, a.kind), (5, 9, 400, 300, 17, 1, 10));
        let mut b = BytesMut::new();
        CzRequestAct { target: 9, action: 7 }.encode(&mut b);
        assert_eq!(b.len(), client_packet("ActionRequest").len as usize);
    }
}

/// Worn equipment views: weapon/shield sprite classes and headgear view ids.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Look {
    pub weapon: u32,
    pub shield: u32,
    pub head_top: u16,
    pub head_mid: u16,
    pub head_bottom: u16,
}

/// ZC_SPRITE_CHANGE (0x01d7): (unit, kind, value, value2). Kinds: 1 hair, 2 weapon
/// (value2 = shield), 3 lower headgear, 4 upper, 5 middle, 6 hair colour.
pub fn parse_sprite_change(data: &[u8]) -> (u32, u8, u32, u32) {
    let mut b = &data[2..];
    (b.get_u32_le(), b.get_u8(), b.get_u32_le(), b.get_u32_le())
}

/// CZ_RESTART (0x00b2): 0 return to the save point after dying, 1 back to character select.
pub struct CzRestart {
    pub kind: u8,
}

impl Encode for CzRestart {
    fn encode(&self, d: &mut BytesMut) {
        put_db_packet(d, "Restart", &[&[self.kind]]);
    }
}

/// CZ_STANDING_RESURRECTION (0x0292): revive in place with a Token of Siegfried.
pub struct CzStandingResurrection;

impl Encode for CzStandingResurrection {
    fn encode(&self, d: &mut BytesMut) {
        d.put_u16_le(0x0292);
    }
}

/// CZ_REQ_DISCONNECT (0x018a): ask to leave the game. The server answers ZC_ACK_REQ_DISCONNECT:
/// 0 go ahead, 1 not yet (within prevent_logout of combat).
pub struct CzReqDisconnect;

impl Encode for CzReqDisconnect {
    fn encode(&self, d: &mut BytesMut) {
        put_db_packet(d, "QuitGame", &[&0u16.to_le_bytes()]);
    }
}

/// Character-select screen requests at PACKETVER 20250402.
#[derive(Debug, Clone, PartialEq)]
pub enum CharRequest {
    /// CH_MAKE_CHAR (0x0a39). `sex`: 0 female, 1 male; `job` 0 Novice, 4218 Summoner.
    Make { name: String, slot: u8, hair_color: u16, hair_style: u16, job: u32, sex: u8 },
    /// CH_DELETE_CHAR3_RESERVED (0x0827): start the deletion timer.
    ReserveDelete(u32),
    /// CH_DELETE_CHAR3 (0x0829): delete after the timer, confirming with the birthdate (YYMMDD).
    Delete { char_id: u32, birthdate: String },
    /// CH_DELETE_CHAR3_CANCEL (0x082b).
    CancelDelete(u32),
    /// CH_REQ_IS_VALID_CHARNAME (0x028d): may this character take the name?
    CheckName { account_id: u32, char_id: u32, name: String },
    /// CH_REQ_CHANGE_CHARNAME (0x08fc): rename it (after the check).
    Rename { char_id: u32, name: String },
    /// CH_REQ_CHANGE_CHARACTER_SLOT (0x08d4).
    MoveSlot { from: u16, to: u16 },
}

/// HC_BLOCK_CHARACTER (0x020d): characters that cannot be played yet, with when they free up.
pub fn parse_blocked_chars(data: &[u8]) -> Vec<(u32, String)> {
    data.get(4..)
        .unwrap_or_default()
        .chunks_exact(24)
        .map(|mut c| {
            let id = c.get_u32_le();
            (id, get_fixed_str(&mut c, 20))
        })
        .collect()
}

impl Encode for CharRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            CharRequest::Make { name, slot, hair_color, hair_style, job, sex } => {
                d.put_u16_le(0x0a39);
                put_fixed_str(d, name, 24);
                d.put_u8(*slot);
                d.put_u16_le(*hair_color);
                d.put_u16_le(*hair_style);
                d.put_u32_le(*job);
                d.put_u8(*sex);
            }
            CharRequest::ReserveDelete(id) => {
                d.put_u16_le(0x0827);
                d.put_u32_le(*id);
            }
            CharRequest::Delete { char_id, birthdate } => {
                d.put_u16_le(0x0829);
                d.put_u32_le(*char_id);
                let mut b = [b'0'; 6];
                for (dst, src) in b.iter_mut().zip(birthdate.bytes().filter(u8::is_ascii_digit)) {
                    *dst = src;
                }
                d.put_slice(&b);
            }
            CharRequest::CancelDelete(id) => {
                d.put_u16_le(0x082b);
                d.put_u32_le(*id);
            }
            CharRequest::CheckName { account_id, char_id, name } => {
                d.put_u16_le(0x028d);
                d.put_u32_le(*account_id);
                d.put_u32_le(*char_id);
                put_fixed_str(d, name, 24);
            }
            CharRequest::Rename { char_id, name } => {
                d.put_u16_le(0x08fc);
                d.put_u32_le(*char_id);
                put_fixed_str(d, name, 24);
            }
            CharRequest::MoveSlot { from, to } => {
                d.put_u16_le(0x08d4);
                d.put_u16_le(*from);
                d.put_u16_le(*to);
                d.put_u16_le(0);
            }
        }
    }
}

/// HC_DELETE_CHAR3_RESERVED / HC_DELETE_CHAR3 / HC_DELETE_CHAR3_CANCEL: (char id, result
/// (1 ok), seconds until the deletion can be confirmed, 0 if not sent).
pub fn parse_delete_reply(data: &[u8]) -> (u32, i32, u32) {
    let mut b = &data[2..];
    let id = b.get_u32_le();
    let result = b.get_i32_le();
    (id, result, if b.remaining() >= 4 { b.get_u32_le() } else { 0 })
}

/// rAthena directions (0 north, 1 north-west, ... 4 south) as sprite directions (0 south,
/// 1 south-west, ... 4 north).
pub fn server_dir(d: u8) -> u8 {
    (12 - (d & 7)) % 8
}

/// CZ_MEMORIALDUNGEON_COMMAND (0x02cf): 3 destroys our memorial dungeon.
pub struct MemorialCommand(pub i32);

impl Encode for MemorialCommand {
    fn encode(&self, d: &mut BytesMut) {
        d.put_u16_le(0x02cf);
        d.put_i32_le(self.0);
    }
}

/// Roulette requests: open (0x0a19), prize table (0x0a1b), close (0x0a1d), spin (0x0a1f),
/// claim the prize (0x0a21).
#[derive(Debug, Clone, Copy)]
pub enum RouletteRequest {
    Open,
    Info,
    Close,
    Spin,
    Claim,
}

impl Encode for RouletteRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            RouletteRequest::Open => d.put_u16_le(0x0a19),
            RouletteRequest::Info => d.put_u16_le(0x0a1b),
            RouletteRequest::Close => d.put_u16_le(0x0a1d),
            RouletteRequest::Spin => d.put_u16_le(0x0a1f),
            RouletteRequest::Claim => {
                d.put_u16_le(0x0a21);
                d.put_u8(0);
            }
        }
    }
}

/// Macro detector (anti-bot captcha) requests.
#[derive(Debug, Clone)]
pub enum CaptchaRequest {
    /// CZ_COMPLETE_APPLY_MACRO_DETECTOR_CAPTCHA (0x0a5a): the image arrived.
    Downloaded,
    /// CZ_ACK_ANSWER_MACRO_DETECTOR (0x0a5c): our answer (up to 15 characters).
    Answer(String),
    /// CZ_REQ_APPLY_MACRO_DETECTOR (0x0a56): a GM puts a player (account id) to the test.
    Apply(u32),
}

impl Encode for CaptchaRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            CaptchaRequest::Downloaded => d.put_u16_le(0x0a5a),
            CaptchaRequest::Answer(a) => {
                d.put_u16_le(0x0a5c);
                put_fixed_str(d, a, 16);
            }
            CaptchaRequest::Apply(aid) => {
                d.put_u16_le(0x0a56);
                d.put_u32_le(*aid);
            }
        }
    }
}

#[cfg(test)]
mod pin_tests {
    use super::*;

    /// rAthena's char_pincode_decrypt.
    fn decrypt(mut seed: u32, sent: [u8; 4]) -> String {
        let mut tab: [u8; 10] = std::array::from_fn(|i| i as u8);
        for i in 1..10u32 {
            seed = 0x881234u32.wrapping_add(seed.wrapping_mul(0x3498));
            let pos = (seed % (i + 1)) as usize;
            if i as usize != pos {
                tab.swap(i as usize, pos);
            }
        }
        sent.iter().map(|&c| char::from(b'0' + tab[(c - b'0') as usize])).collect()
    }

    #[test]
    fn pin_survives_the_servers_decrypt() {
        for seed in [0, 1, 12345, 0xfffe, 40000] {
            for pin in ["2580", "0000", "9137"] {
                assert_eq!(decrypt(seed, pin_scramble(seed, pin)), pin);
            }
        }
        let mut d = BytesMut::new();
        PinPacket { account_id: 7, seed: 0, req: &PinRequest::Change { old: "1111".into(), new: "2580".into() } }.encode(&mut d);
        assert_eq!((d.len(), &d[..2]), (14, &[0xbe, 0x08][..]));
    }
}

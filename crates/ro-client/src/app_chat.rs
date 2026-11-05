//! The chat line: client commands (/commands) and what is said. Part of `App` (app.rs).

use super::*;

impl App {
    /// The original client's slash commands for parties, options and help (msgstringtable
    /// wording). Returns false for anything else.
    pub(crate) fn client_command(&mut self, text: &str) -> bool {
        let t = text.trim();
        let (head, arg) = t.split_once(' ').map_or((t, ""), |(h, a)| (h, a.trim().trim_matches('"')));
        let say = |id: usize, en: &str| msgstr::get(id, &[]).unwrap_or_else(|| en.to_string());
        let mut notes = vec![];
        match head {
            "/h" | "/help" => notes.extend((33..=46).chain(132..=142).chain([157, 211]).filter_map(|id| msgstr::get(id, &[]))),
            "/who" | "/w" | "/player" if arg.is_empty() => self.send(ro_net::Command::Who),
            "/organize" if !arg.is_empty() => self.send(ro_net::Command::Party(ro_net::PartyRequest::Create { name: arg.to_string() })),
            "/invite" if !arg.is_empty() => self.send(ro_net::Command::Party(ro_net::PartyRequest::Invite { name: arg.to_string() })),
            "/leave" => self.send(ro_net::Command::Party(ro_net::PartyRequest::Leave)),
            "/expel" if !arg.is_empty() => {
                let member = self.social.party.as_ref().and_then(|p| p.members.iter().find(|m| m.name.eq_ignore_ascii_case(arg)));
                match member.map(|m| (m.account_id, m.name.clone())) {
                    Some((account_id, name)) => self.send(ro_net::Command::Party(ro_net::PartyRequest::Expel { account_id, name })),
                    None => notes.push(say(148, "There is no such character name or the user is offline.")),
                }
            }
            "/accept" | "/refuse" => {
                self.game_opts.deny_invites = head == "/refuse";
                self.send(ro_net::Command::Option(ro_net::OptionRequest::DenyPartyInvites(head == "/refuse")));
            }
            "/call" => {
                let on = !self.game_opts.call;
                self.game_opts.call = on;
                self.send(ro_net::Command::Option(ro_net::OptionRequest::Config { kind: 1, on }));
                notes.push(if on { say(2978, "You can be called with various skills (Emergency Call,...). (/call)") } else { say(2979, "You cannot be called with various skills (Emergency Call,...). (/call)") });
            }
            "/memo" => self.send(ro_net::Command::Memo),
            // GM commands, as the original client sends them; the server checks permissions.
            "/b" | "/nb" | "/lb" | "/nlb" if !arg.is_empty() => {
                // /b and /lb sign the message with our name; /nb and /nlb don't.
                let rest = t.split_once(' ').map_or("", |(_, r)| r.trim());
                let text = if head.starts_with("/n") { rest.to_string() } else { format!("{}: {rest}", self.hud.status.name) };
                self.send(ro_net::Command::Gm(if head.ends_with("lb") { ro_net::GmRequest::LocalBroadcast(text) } else { ro_net::GmRequest::Broadcast(text) }));
            }
            "/hide" => self.send(ro_net::Command::Gm(ro_net::GmRequest::Hide)),
            "/mm" | "/mapmove" if !arg.is_empty() => {
                let mut parts = t.split_whitespace().skip(1);
                let map = parts.next().unwrap_or_default().to_string();
                let (x, y) = (parts.next().and_then(|v| v.parse().ok()).unwrap_or(0), parts.next().and_then(|v| v.parse().ok()).unwrap_or(0));
                self.send(ro_net::Command::Gm(ro_net::GmRequest::MapMove { map, x, y }));
            }
            "/recall" if !arg.is_empty() => self.send(ro_net::Command::Gm(ro_net::GmRequest::Recall(arg.to_string()))),
            "/shift" | "/remove" if !arg.is_empty() => self.send(ro_net::Command::Gm(ro_net::GmRequest::Shift(arg.to_string()))),
            "/item" | "/monster" if !arg.is_empty() => self.send(ro_net::Command::Gm(ro_net::GmRequest::Create(arg.to_string()))),
            "/rc" if !arg.is_empty() => self.send(ro_net::Command::Gm(ro_net::GmRequest::Mute(arg.to_string()))),
            "/check" if !arg.is_empty() => self.send(ro_net::Command::Gm(ro_net::GmRequest::Check(arg.to_string()))),
            "/killall" => self.send(ro_net::Command::Gm(ro_net::GmRequest::KickAll)),
            "/resetstate" => self.send(ro_net::Command::Gm(ro_net::GmRequest::ResetStats)),
            "/resetcooltime" => self.send(ro_net::Command::Gm(ro_net::GmRequest::ResetCooltime)),
            "/changemaptype" => {
                let v: Vec<u16> = arg.split_whitespace().filter_map(|v| v.parse().ok()).collect();
                match v[..] {
                    [x, y, kind] => self.send(ro_net::Command::Gm(ro_net::GmRequest::ChangeMapType { x, y, kind })),
                    _ => notes.push("Usage: /changemaptype <x> <y> <type>".into()),
                }
            }
            "/deal" if !arg.is_empty() => match self.units.iter().find(|(_, u)| u.name.eq_ignore_ascii_case(arg)).map(|(id, _)| *id) {
                Some(target) => self.send(ro_net::Command::Trade(ro_net::TradeRequest::Ask { target })),
                None => notes.push(say(148, "There is no such character name or the user is offline.")),
            },
            "/chat" => {
                let me = self.hud.status.name.clone();
                self.rooms.open_create(&me);
            }
            "/q" => self.send(ro_net::Command::ChatRoom(ro_net::ChatRoomRequest::Leave)),
            "/bgm" | "/music" => {
                self.settings.bgm_off = !self.settings.bgm_off;
                notes.push(if self.settings.bgm_off { say(32, "BGM Off") } else { say(31, "BGM On") });
            }
            "/sound" => {
                self.settings.sfx_off = !self.settings.sfx_off;
                notes.push(if self.settings.sfx_off { say(28, "Sound Effects Off") } else { say(27, "Sound Effects On") });
            }
            "/effect" => {
                self.settings.effects_off = !self.settings.effects_off;
                notes.push(if self.settings.effects_off { say(24, "Effects Off") } else { say(23, "Effects On") });
            }
            "/aura" if self.settings.aura_off => notes.push(say(2996, "Aura effect is OFF. Please turn it ON and try again")),
            "/aura" => {
                self.settings.aura_simple = !self.settings.aura_simple;
                notes.push(if self.settings.aura_simple { say(712, "Simplified Aura [ON]") } else { say(711, "Simplified Aura [OFF]") });
            }
            "/bm" | "/battlemode" => {
                self.settings.battle_mode = !self.settings.battle_mode;
                notes.push(if self.settings.battle_mode { say(785, "Skills assigned to shortcut windows 1, 2, 3 are Enabled. [/bm ON]") } else { say(786, "Skills assigned to shortcut windows 1, 2, 3 are Disabled. [/bm OFF]") });
                if self.settings.battle_mode {
                    notes.push(say(796, "Press the space bar to Chat when in Battle mode [/battlemode | /bm]."));
                }
            }
            "/emo" | "/emotion" => self.emote_win.open = !self.emote_win.open,
            // CZ_DORIDORI: the head shake's SP regen bonus (Super Novice, or a resting Taekwon
            // line); the head itself doesn't turn here.
            "/doridori" => {
                self.doridori = Some(0.0);
                self.send(ro_net::Command::Bare("NoviceDoriDori"));
            }
            // Out of a costume status (wedding, Xmas, summer, dress-up, hanbok, Oktoberfest).
            "/changedress" | "/nocosplay" => self.send(ro_net::Command::Bare("changedress")),
            // CZ_RESET_SKILL runs rAthena's @resetskill, so it needs that GM permission.
            "/resetskill" => self.send(ro_net::Command::Bare("reset_skill")),
            "/aura2" => {
                self.settings.aura_off = !self.settings.aura_off;
                notes.push(if self.settings.aura_off { say(2995, "Aura effect is OFF") } else { say(2994, "Aura effect is ON") });
            }
            "/v" | "/bv" if !arg.is_empty() => {
                let Ok(v) = arg.parse::<u8>() else { return false };
                let v = v.min(127) as f32 / 127.0;
                if head == "/v" {
                    self.settings.sfx_volume = v;
                } else {
                    self.settings.bgm_volume = v;
                }
            }
            _ => return false,
        }
        if matches!(head, "/bgm" | "/music" | "/sound" | "/effect" | "/aura" | "/aura2" | "/v" | "/bv" | "/bm" | "/battlemode") {
            self.apply_sound_and_effects();
            self.settings.save();
        }
        for n in notes {
            self.hud.push_chat(ro_net::ChatKind::System, n);
        }
        true
    }

    /// Volumes and the effect switch, as the settings and /bgm, /sound, /effect leave them.
    pub(crate) fn apply_sound_and_effects(&mut self) {
        let s = &self.settings;
        if let Some(a) = self.audio.as_mut() {
            a.set_bgm_volume(if s.bgm_off { 0.0 } else { s.bgm_volume });
            a.sfx_volume = if s.sfx_off { 0.0 } else { s.sfx_volume };
        }
        if let Some(r) = self.renderer.as_mut() {
            r.show_effects = !s.effects_off;
        }
    }

    /// Sends a chat line, handling the client's own commands (whispers, emotes, /where...).
    /// A Super Novice's prayer to the guardian angel (msgstringtable 790-793), said line by line
    /// in public chat: true once its last line is said, when the client asks for Critical
    /// Explosion (CZ_CHOPOKGI; rAthena grants it at each tenth of the base level's experience).
    pub(crate) fn prayer(&mut self, line: &str) -> bool {
        const SUPER_NOVICES: [i16; 4] = [23, 4045, 4190, 4191];
        if !SUPER_NOVICES.contains(&self.hud.status.job_id) || line.starts_with('/') {
            self.prayer_at = 0;
            return false;
        }
        let say = |id: usize, en: &str| msgstr::get(id, &[]).unwrap_or_else(|| en.to_string());
        let lines = [say(790, "Dear angel, can you hear my voice?"), format!("{} {} {}", say(791, "I am"), self.hud.status.name, say(792, "Super Novice~")), say(793, "Help me out~ Please~ T_T")];
        let said = |i: usize| line.trim().eq_ignore_ascii_case(lines[i].trim());
        let next = if said(self.prayer_at) { self.prayer_at + 1 } else { said(0) as usize };
        self.prayer_at = next % lines.len();
        next == lines.len()
    }

    pub(crate) fn submit_chat(&mut self, text: String) {
        if self.client_command(&text) {
            return;
        }
        let prayed = self.prayer(&text);
        let Some((tx, _)) = self.net.as_ref() else { return };
            // "/p text" goes to the party, "/g text" to the guild.
            let speaker = self.hud.status.name.clone();
            let mut note = None;
            // Whisper blocking, as the original: /ex name, /in name, /exall, /inall, /ex (list).
            let ignore = match text.trim() {
                "/exall" => Some(ro_net::IgnoreRequest::All { block: true }),
                "/inall" => Some(ro_net::IgnoreRequest::All { block: false }),
                "/ex" => Some(ro_net::IgnoreRequest::List),
                t => match t.split_once(' ') {
                    Some(("/ex", n)) => Some(ro_net::IgnoreRequest::Player { name: n.trim().trim_matches('"').to_string(), block: true }),
                    Some(("/in", n)) => Some(ro_net::IgnoreRequest::Player { name: n.trim().trim_matches('"').to_string(), block: false }),
                    _ => None,
                },
            };
            // /viewpointvalue shows the camera values; /setcamera range rotation latitude sets them.
            if text.trim() == "/viewpointvalue" {
                let _ = tx.send(ro_net::Command::Camera { show: true, range: 0.0, rotation: 0.0, latitude: 0.0 });
                return;
            }
            if let Some(args) = text.trim().strip_prefix("/setcamera ") {
                let v: Vec<f32> = args.split_whitespace().filter_map(|s| s.parse().ok()).collect();
                if let [range, rotation, latitude] = v[..] {
                    let _ = tx.send(ro_net::Command::Camera { show: false, range, rotation, latitude });
                }
                return;
            }
            if text.trim() == "/macroreport" {
                self.reporter.toggle();
                return;
            }
            if text.trim() == "/agency" {
                self.agency.toggle();
                return;
            }
            if text.trim() == "/limitedsale" {
                let _ = tx.send(ro_net::Command::Sale(self.account_id, ro_net::SaleRequest::Open));
                return;
            }
            if text.trim() == "/roulette" {
                let _ = tx.send(ro_net::Command::Roulette(ro_net::RouletteRequest::Open));
                let _ = tx.send(ro_net::Command::Roulette(ro_net::RouletteRequest::Info));
                return;
            }
            if let Some(k) = ["/blacksmith", "/alchemist", "/taekwon", "/pk"].iter().position(|c| text.trim() == *c) {
                let _ = tx.send(ro_net::Command::Ranking(k as u16));
                return;
            }
            // /navi map [x/y]: guide there, as the original client; /navi alone opens the window.
            if let Some(rest) = text.trim().strip_prefix("/navi") {
                self.navi.ensure_loaded(self.vfs.as_deref());
                let mut parts = rest.split(|c: char| c == ' ' || c == '/' || c == ',').filter(|s| !s.is_empty());
                match parts.next() {
                    Some(map) => {
                        let cell = match (parts.next().and_then(|x| x.parse().ok()), parts.next().and_then(|y| y.parse().ok())) {
                            (Some(x), Some(y)) => Some((x, y)),
                            _ => None,
                        };
                        let label = match cell { Some((x, y)) => format!("{map} ({x}, {y})"), None => map.to_string() };
                        self.navi.go(map, cell, label);
                    }
                    None => self.navi.open = true,
                }
                return;
            }
            if let Some(r) = ignore {
                if let ro_net::IgnoreRequest::Player { name, block } = &r {
                    self.ignore_pending = Some((name.clone(), *block));
                }
                let _ = tx.send(ro_net::Command::Ignore(r));
                return;
            }
            let cmd = match text.split_once(' ') {
                Some(("/p", rest)) => Some(ro_net::Command::Party(ro_net::PartyRequest::Chat { speaker, text: rest.to_string() })),
                Some(("/g", rest)) => Some(ro_net::Command::Party(ro_net::PartyRequest::GuildChat { speaker, text: rest.to_string() })),
                Some(("/cl", rest)) => Some(ro_net::Command::Party(ro_net::PartyRequest::ClanChat { speaker, text: rest.to_string() })),
                Some(("/bc", rest)) => Some(ro_net::Command::Bg(ro_net::BgRequest::Chat(format!("{speaker} : {rest}")))),
                // /guild "Name": found a guild, as in the original client.
                Some(("/guild", rest)) => Some(ro_net::Command::Guild(ro_net::GuildRequest::Create { char_id: self.char_id, name: rest.trim().trim_matches('"').to_string() })),
                // /r text: answer whoever whispered last.
                Some(("/r", rest)) => match self.last_whisper_from.clone() {
                    Some(to) => {
                        note = Some((ro_net::ChatKind::Whisper, format!("(To {to}) : {rest}")));
                        Some(ro_net::Command::Whisper { to, text: rest.to_string() })
                    }
                    None => {
                        note = Some((ro_net::ChatKind::System, "Nobody has whispered you yet.".into()));
                        None
                    }
                },
                _ if text.starts_with("/w ") || text.starts_with("/whisper ") => match emotes::whisper(&text) {
                    Some((to, msg)) => {
                        note = Some((ro_net::ChatKind::Whisper, format!("(To {to}) : {msg}")));
                        Some(ro_net::Command::Whisper { to, text: msg })
                    }
                    None => {
                        note = Some((ro_net::ChatKind::System, "Usage: /w Name message  (quote names with spaces)".into()));
                        None
                    }
                },
                _ => match emotes::command(&text) {
                    Some(e) => Some(ro_net::Command::Emotion(e)),
                    // Commands the client handles itself.
                    None if text == "/sit" || text == "/stand" => Some(ro_net::Command::Sit(text == "/sit")),
                    None if text == "/where" => {
                        let map = self.world.as_ref().map(|w| w.name.clone()).unwrap_or_default();
                        if let Some((x, y)) = self.player.as_ref().map(|p| p.walker.cell) {
                            note = Some((ro_net::ChatKind::System, format!("{map} {x}, {y}")));
                        }
                        None
                    }
                    // An unknown /command is not said aloud.
                    None if text.starts_with('/') => {
                        let name = text.split_whitespace().next().unwrap_or(&text);
                        note = Some((ro_net::ChatKind::System, format!("{name}: unknown command.")));
                        None
                    }
                    None => Some(ro_net::Command::Chat(text)),
                },
            };
            if let Some(cmd) = cmd {
                let _ = tx.send(cmd);
            }
            if prayed {
                let _ = tx.send(ro_net::Command::Bare("NoviceExplosionSpirits"));
            }
            if let Some((kind, line)) = note {
                self.hud.push_chat(kind, line);
            }
    }
}

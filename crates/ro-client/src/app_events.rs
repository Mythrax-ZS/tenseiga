//! What the server says: one ro_net::Event at a time. Part of `App` (app.rs).

use super::*;

impl App {
    pub(crate) fn handle(&mut self, event: ro_net::Event) {
        use ro_net::Event::*;
        // Login and character-select replies belong to those screens.
        if let Some(front) = self.front.as_mut() {
            if front.event(&event) {
                if let (Characters { chars, .. }, true) = (&event, std::env::args().any(|a| a == "--front-start")) {
                    if let Some(c) = chars.first() {
                        self.front_start = Some(c.slot);
                    }
                }
                // --front-pin 1234 answers PIN prompts (entering, making or renewing it).
                let pin_arg = std::env::args().skip_while(|a| a != "--front-pin").nth(1);
                if let (Pin(state @ 1..=4), Some(pin)) = (&event, pin_arg) {
                    let req = match state {
                        1 => ro_net::PinRequest::Check(pin),
                        3 => ro_net::PinRequest::Change { old: pin.clone(), new: pin },
                        _ => ro_net::PinRequest::Make(pin),
                    };
                    self.queued_commands.push(ro_net::Command::Pin(req));
                }
                // --front-start waits for the PIN code to be accepted.
                if !front.pin_pending() {
                    if let Some(slot) = self.front_start.take() {
                        self.queued_commands.push(ro_net::Command::SelectChar(slot));
                    }
                }
                if let (Characters { chars, .. }, true) = (&event, std::env::args().any(|a| a == "--front-make")) {
                    if !chars.iter().any(|c| c.name == "Preview") {
                        let slot = (0..9u8).find(|s| !chars.iter().any(|c| c.slot == *s)).unwrap_or(1);
                        self.queued_commands.push(ro_net::Command::Char(ro_net::CharRequest::Make { name: "Preview".into(), slot, hair_color: 4, hair_style: 9, job: 0, sex: 0 }));
                    }
                }
                if matches!(event, Characters { .. }) && std::env::args().any(|a| a == "--front-create") {
                    front.open_create();
                }
                // --front-rename NAME / --front-move SLOT: rename or move the last character once.
                let args: Vec<String> = std::env::args().collect();
                let after = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned());
                match &event {
                    Characters { chars, .. } if self.front_hook.is_none() => {
                        if let Some(c) = chars.iter().max_by_key(|c| c.slot) {
                            self.front_hook = Some(c.char_id);
                            if let Some(name) = after("--front-rename") {
                                self.queued_commands.push(ro_net::Command::Char(ro_net::CharRequest::CheckName { account_id: 0, char_id: c.char_id, name }));
                            }
                            if let Some(to) = after("--front-move").and_then(|s| s.parse().ok()) {
                                self.queued_commands.push(ro_net::Command::Char(ro_net::CharRequest::MoveSlot { from: c.slot as u16, to }));
                            }
                        }
                    }
                    CharNameOk(true) => {
                        if let (Some(char_id), Some(name)) = (self.front_hook, after("--front-rename")) {
                            self.queued_commands.push(ro_net::Command::Char(ro_net::CharRequest::Rename { char_id, name }));
                        }
                    }
                    _ => {}
                }
                return;
            }
            match &event {
                Entered { .. } => {
                    // Remember the ID for next time (Remember ID).
                    self.settings.last_id = if front.remember { front.user().to_string() } else { String::new() };
                    self.settings.save();
                    self.front = None;
                }
                Disconnected(reason) => {
                    // Back to a fresh login screen with the reason.
                    let user = front.user().to_string();
                    let mut f = screens::Front::new(user, front.remember);
                    f.event(&LoginFailed(format!("Disconnected: {reason}")));
                    self.front = Some(f);
                    if let (Some(a), Some(v)) = (self.audio.as_mut(), self.vfs.as_deref()) {
                        a.title(v);
                    }
                    self.net = Some(ro_net::spawn_session(self.session.clone()));
                    return;
                }
                _ => {}
            }
        }
        // A run of the server's weather effects lasts until it sends anything else.
        if !matches!(event, ScriptEffect { effect, .. } if weather::from_effect(effect).is_some()) && !matches!(event, Chat { from: None, .. }) {
            self.weather.server_other();
        }
        match event {
            Entered { map, x, y, dir, character, account_id, font } => {
                tracing::info!(%map, x, y, name = %character.name, job = character.job, "entered");
                self.account_id = account_id;
                self.char_id = character.char_id;
                self.units.clear();
                self.devotions.clear();
                self.ground_units.clear();
                self.traps.clear();
                self.graffiti.clear();
                self.floor.clear();
                self.warps.clear();
                self.enter_map(&map);
                let look = Look {
                    job: if self.gm_sprite(account_id, font) { jobs::GM_SPRITE.to_string() } else { job_sprite(character.job).to_string() },
                    male: character.sex == 1,
                    hair: character.head.max(1) as u16,
                    hair_color: character.hair_color as u16,
                    clothes_color: character.clothes_color.max(0) as u16,
                    gear: character.look,
                };
                let s = &mut self.hud.status;
                s.name = character.name.clone();
                s.job = job_title(character.job).to_string();
                s.job_id = character.job;
                s.base_level = character.base_level as i64;
                s.job_level = character.job_level as i64;
                (s.hp, s.max_hp, s.sp, s.max_sp) = (character.hp, character.max_hp, character.sp, character.max_sp);
                s.base_exp = character.base_exp;
                s.zeny = character.zeny as i64;
                self.spawn_player(&look, (x, y), dir as usize, character.speed);
                let yellow = self.yellow_name(account_id, font);
                if let Some(p) = self.player.as_mut() {
                    (p.font, p.yellow) = (font, yellow);
                }
                if let Some(text) = self.say_on_enter.take() {
                    // Several lines separated by "|" (e.g. a batch of GM commands).
                    for line in text.split('|') {
                        self.queued_chat.push(line.trim().to_string());
                    }
                }
                if let Some(cell) = self.walk_on_enter.take() {
                    self.request_walk(cell);
                }
            }
            Walk { from, to } => {
                let gat = self.world.as_ref().and_then(|w| w.gat.as_ref());
                if let Some(p) = self.player.as_mut() {
                    tracing::debug!(?from, ?to, cell = ?p.walker.cell, next = ?p.walker.next(), "walk");
                    if let Some(g) = self.walk_goal.as_mut() {
                        g.tries = 0; // the server accepted this hop
                    }
                    p.walk(gat, from, to);
                }
            }
            Stop { id, x, y } => {
                let mob = if id == self.account_id { self.player.as_mut() } else { self.units.get_mut(&id) };
                if let Some(m) = mob {
                    m.walker.stop_at((x as i32, y as i32));
                }
            }
            Unit(info) => self.unit(info),
            Stat { id, value } => {
                self.hud.status.set(id, value);
                // SP_SPEED: our walk speed (ms per cell) as the server moves us. Walking at any
                // other pace runs ahead of (or behind) the server, and every re-aimed walk
                // then pulls us back to where it has us.
                if id == 0 && value > 0 {
                    tracing::debug!(speed = value, "walk speed");
                    self.walk_speed = Some(value as f32);
                    if let Some(p) = self.player.as_mut() {
                        p.walker.speed = value as f32;
                    }
                }
                // Keep the Status window live: points and the derived values (SP_ATK1..SP_ASPD).
                if let Some(s) = self.stats.as_mut() {
                    let v = value.clamp(i16::MIN as i64, i16::MAX as i64) as i16;
                    match id {
                        9 => s.points = value.max(0) as u32,
                        41 => s.atk.0 = v,
                        42 => s.atk.1 = v,
                        43 => s.matk.1 = v,
                        44 => s.matk.0 = v,
                        45 => s.def.0 = v,
                        46 => s.def.1 = v,
                        47 => s.mdef.0 = v,
                        48 => s.mdef.1 = v,
                        49 => s.hit = v,
                        50 => s.flee.0 = v,
                        51 => s.flee.1 = v,
                        52 => s.crit = v,
                        53 => s.aspd = (v, 0),
                        225..=230 => s.advanced[(id - 225) as usize] = v,
                        231 => s.trait_points = value.max(0) as u32,
                        _ => {}
                    }
                }
            }
            Chat { kind, from, text, color } => {
                if let (ro_net::ChatKind::Whisper, Some(name)) = (kind, text.strip_prefix("(From ").and_then(|t| t.split_once(") : ")).map(|(n, _)| n.to_string())) {
                    self.last_whisper_from = Some(name);
                    self.play_sound("_chat_whisper.wav");
                }
                // What a unit says aloud also shows over its head: chat (ours comes back
                // without a speaker), npctalk and unittalk.
                match (kind, from) {
                    (ro_net::ChatKind::Public | ro_net::ChatKind::Npc, Some(id)) => _ = self.shouts.insert(id, (text.clone(), 0.0, true)),
                    (ro_net::ChatKind::Public, None) => _ = self.shouts.insert(self.account_id, (text.clone(), 0.0, true)),
                    _ => {}
                }
                // The server's word that weather stopped (@snow again, @clearweather).
                if from.is_none() {
                    self.weather.server_said(&text);
                }
                self.hud.push_chat_coloured(kind, text, color);
            }
            Menu(menu) => {
                // Icons for items that may not be in the inventory (poisons, arrows to make).
                let ids: Vec<u32> = match &menu {
                    ro_net::Menu::Items(ids) | ro_net::Menu::Make { items: ids, .. } => ids.clone(),
                    ro_net::Menu::Produce(list) => list.iter().flat_map(|(id, m)| std::iter::once(*id).chain(m.iter().copied())).collect(),
                    _ => vec![],
                };
                self.load_icons_for(&ids);
                self.windows.menu = Some(menu);
            }
            MakeResult { ok, item } => {
                let name = self.item_db.as_ref().map_or_else(|| format!("item {item}"), |d| d.name(item, true));
                let msg = if ok { format!("You made {name}.") } else { format!("Making {name} failed.") };
                self.hud.push_chat(ro_net::ChatKind::System, msg);
            }
            Quests(list) => {
                tracing::debug!(quests = list.len(), "quest list");
                // The character's saved pins join any made since (the server sends the list
                // again after a quest is taken), then those of quests it no longer has go.
                let saved = self.settings.tracked_quests.get(&self.hud.status.name).cloned().unwrap_or_default();
                for id in saved {
                    if !self.quests.tracked.contains(&id) {
                        self.quests.tracked.push(id);
                    }
                }
                self.quests.listed(list);
            }
            QuestAdded(q) => {
                tracing::debug!(quest = q.id, objectives = q.objectives.len(), "quest added");
                self.hud.push_chat(ro_net::ChatKind::System, format!("Quest accepted: {}", self.quests.title(q.id)));
                self.quests.accepted(q);
            }
            QuestRemoved(id) => {
                self.quests.quests.remove(&id);
                self.quests.set_tracked(id, false);
            }
            QuestProgress(updates) => self.quests.progress(&updates),
            QuestActiveChanged { quest, active } => {
                if let Some(q) = self.quests.quests.get_mut(&quest) {
                    q.active = active;
                }
            }
            QuestMark { npc, mark, color } => {
                if mark == 9999 {
                    self.quests.marks.remove(&npc);
                } else {
                    self.quests.marks.insert(npc, (mark, color));
                }
            }
            InventoryExtra(n) => self.windows.inv_extra = n,
            ItemFavorite { index, favorite } => {
                if let Some(it) = self.inventory.items.get_mut(&index) {
                    it.favorite = favorite;
                }
            }
            ExpandAsk { result, item } => match result {
                0 => self.windows.expand_ask = Some(item),
                _ => self.hud.push_chat(ro_net::ChatKind::System, match result {
                    2 => "You can't expand your inventory right now.",
                    3 => "You need an inventory expansion item.",
                    4 => "Your inventory can't be expanded any further.",
                    _ => "The inventory can't be expanded.",
                }.into()),
            },
            CartOff => {
                self.merchant.cart.clear();
                self.merchant.cart_count = Default::default();
                self.merchant.cart_open = false;
            }
            PartyPickup { aid, item, refine } => {
                let who = self.units.get(&aid).map(|m| m.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| "A party member".into());
                let name = self.item_db.as_ref().map_or_else(|| format!("Item #{item}"), |d| d.name(item, true));
                let refine = if refine > 0 { format!("+{refine} ") } else { String::new() };
                self.hud.push_chat(ro_net::ChatKind::Party, format!("{who} got {refine}{name}."));
            }
            CartStart => {
                tracing::debug!("cart list");
                self.merchant.cart.clear();
            }
            CartItems(items) => {
                for d in items {
                    let it = inv_item(d);
                    self.merchant.cart.insert(it.index, it);
                }
                let ids = self.merchant.item_ids();
                self.load_icons_for(&ids);
            }
            CartCount { items, max_items, weight, max_weight } => {
                tracing::debug!(items, max_items, weight, max_weight, "cart");
                self.merchant.cart_count = (items, max_items, weight, max_weight);
            }
            CartAdded(d) => {
                let it = inv_item(d);
                self.load_icons_for(&[it.id]);
                match self.merchant.cart.get_mut(&it.index) {
                    Some(e) if e.id == it.id => e.amount += it.amount,
                    _ => {
                        self.merchant.cart.insert(it.index, it);
                    }
                }
            }
            CartRemoved { index, amount } => {
                if let Some(e) = self.merchant.cart.get_mut(&index) {
                    e.amount = e.amount.saturating_sub(amount as u16);
                    if e.amount == 0 {
                        self.merchant.cart.remove(&index);
                    }
                }
            }
            ShopSign { aid, title } => {
                if title.is_empty() {
                    self.merchant.signs.remove(&aid);
                } else {
                    self.merchant.signs.insert(aid, title);
                }
            }
            Shop { seller, shop, items } => {
                let ids: Vec<u32> = items.iter().map(|i| i.id).collect();
                self.load_icons_for(&ids);
                let amounts = vec![0; items.len()];
                self.merchant.browsing = Some((seller, shop, items, amounts));
            }
            ShopSetup(slots) => {
                self.merchant.setup = Some((slots, format!("{}'s shop", self.hud.status.name), Default::default()));
            }
            MyShop(stock) => self.merchant.my_shop = Some(stock),
            Sold { index, amount, zeny } => {
                let what = self.merchant.my_shop.as_ref().and_then(|s| s.iter().find(|i| i.index == index)).map(|i| i.id);
                let name = what.and_then(|id| self.item_db.as_ref().map(|d| d.name(id, true))).unwrap_or_else(|| "an item".into());
                self.hud.push_chat(ro_net::ChatKind::System, format!("Sold {amount} x {name} for {zeny} z."));
                if let Some(stock) = self.merchant.my_shop.as_mut() {
                    if let Some(i) = stock.iter_mut().find(|i| i.index == index) {
                        i.amount = i.amount.saturating_sub(amount);
                    }
                    stock.retain(|i| i.amount > 0);
                }
            }
            BuySign { aid, title } => {
                if title.is_empty() {
                    self.stores.signs.remove(&aid);
                } else {
                    self.stores.signs.insert(aid, title);
                }
            }
            BuyStoreSetup(slots) => {
                let limit = self.hud.status.zeny.max(0).to_string();
                self.stores.setup = Some((slots, format!("{} is buying", self.hud.status.name), limit, Default::default()));
            }
            MyBuyStore { limit, items } => {
                self.stores.mine = Some((limit, items));
                let ids = self.stores.item_ids();
                self.load_icons_for(&ids);
            }
            BuyStore { buyer, store, limit, items } => {
                let amounts = vec![0; items.len()];
                self.stores.browsing = Some((buyer, store, limit, items, amounts));
                let ids = self.stores.item_ids();
                self.load_icons_for(&ids);
            }
            BoughtThroughStore { id, amount, zeny, limit } => {
                let name = self.item_db.as_ref().map_or_else(|| format!("Item #{id}"), |d| d.name(id, true));
                self.hud.push_chat(ro_net::ChatKind::System, format!("Your buying store bought {amount} x {name} for {zeny} z."));
                if let Some((l, items)) = self.stores.mine.as_mut() {
                    *l = limit;
                    if let Some(i) = items.iter_mut().find(|i| i.id == id) {
                        i.amount = i.amount.saturating_sub(amount);
                    }
                }
            }
            SoldToStore { index, amount, price } => {
                let name = self.inventory.items.get(&index).and_then(|i| self.item_db.as_ref().map(|d| d.name(i.id, true))).unwrap_or_else(|| "an item".into());
                self.hud.push_chat(ro_net::ChatKind::System, format!("Sold {amount} x {name} for {} z.", amount as u64 * price as u64));
                self.inventory.remove(index, amount);
            }
            BuyStoreNotice(msg) => {
                if msg.contains("closed") {
                    self.stores.mine = None;
                }
                self.hud.push_chat(ro_net::ChatKind::System, msg);
            }
            Inbox(mut list) => {
                for h in &mut list {
                    h.title = msgstr::resolve(&h.title, "(no title)");
                    h.sender = msgstr::resolve(&h.sender, "System");
                }
                self.mailbox.inbox = list;
            }
            Mail { id, text, zeny, items, .. } => {
                let text = msgstr::resolve(&text, "");
                if let Some(h) = self.mailbox.inbox.iter_mut().find(|h| h.id == id) {
                    h.read = true;
                    self.mailbox.reading = Some((h.clone(), text, zeny, items));
                }
                let ids = self.mailbox.item_ids();
                self.load_icons_for(&ids);
            }
            MailTaken { items, id, result } => {
                if result == 0 {
                    if let Some(r) = self.mailbox.reading.as_mut().filter(|r| r.0.id == id) {
                        if items {
                            r.3.clear();
                        } else {
                            r.2 = 0;
                        }
                    }
                    self.mailbox.status = if items { "Items taken.".into() } else { "Zeny taken.".into() };
                } else {
                    self.mailbox.status = "Couldn't take that (weight or inventory full?).".into();
                }
            }
            MailDeleted(id) => {
                self.mailbox.inbox.retain(|h| h.id != id);
                if self.mailbox.reading.as_ref().is_some_and(|r| r.0.id == id) {
                    self.mailbox.reading = None;
                }
            }
            MailUnread(new) => {
                if new && !self.mailbox.open {
                    self.mailbox.unread = true;
                    self.hud.push_chat(ro_net::ChatKind::System, "You have new mail (Alt+X).".into());
                }
            }
            MailReceiver(id, _) => {
                let me = self.hud.status.name.clone();
                if let Some(req) = self.mailbox.receiver_found(id, &me) {
                    self.send(ro_net::Command::Mail(req));
                }
            }
            ClanInfo(info) => self.clan.info = Some(info),
            ClanOnline(on, max) => self.clan.online = Some((on, max)),
            Navigate { map, x, y } => {
                self.navi.ensure_loaded(self.vfs.as_deref());
                let cell = (x > 0 && y > 0).then_some((x, y));
                let label = match cell { Some((x, y)) => format!("{map} ({x}, {y})"), None => map.clone() };
                self.navi.go(&map, cell, label);
            }
            Bg(e) => {
                let map = self.world.as_ref().map(|w| w.name.clone()).unwrap_or_default();
                if let Some(text) = self.battle.event(e, &map) {
                    self.hud.push_chat(ro_net::ChatKind::Battle, text);
                }
                if self.battle.auto_accept {
                    if let Some((name, lobby)) = self.battle.ready.take() {
                        self.send(ro_net::Command::Bg(ro_net::BgRequest::Reply { accept: true, name, lobby }));
                    }
                }
            }
            Castles(list) => self.battle.castles = list,
            CastleInfo { id, economy, defense } => self.battle.castle_info(id, economy, defense),
            CastleTeleport(r) => {
                let why = if r == 1 { "You don't have enough zeny to teleport to the castle." } else { "You can't teleport to the castle right now." };
                self.hud.push_chat(ro_net::ChatKind::System, why.into());
            }
            Market(items) => {
                self.market.open(market::Shop::Market(items));
                self.market_test_buy();
            }
            MarketResult { ok, bought } => {
                self.market.status = if ok { "Thank you for your purchase.".into() } else { "The purchase failed.".into() };
                if let Some(market::Shop::Market(items)) = self.market.shop.as_mut() {
                    for (item, n, _) in bought {
                        if let Some(it) = items.iter_mut().find(|i| i.item == item) {
                            it.stock = it.stock.saturating_sub(n as u32);
                        }
                    }
                }
            }
            Barter { items, expanded } => {
                self.market.open(market::Shop::Barter { items, expanded });
                self.market_test_buy();
            }
            ClanLeft => {
                self.clan.left();
                self.hud.push_chat(ro_net::ChatKind::System, "You left your clan.".into());
            }
            MailItemAdded { result, index, amount, id } => self.mailbox.item_added(result, index, amount, id),
            MailItemRemoved { ok, index, amount } => self.mailbox.item_removed(ok, index, amount),
            MailSent(result) => {
                self.mailbox.sent(result);
                if result == 0 {
                    self.send(ro_net::Command::Mail(ro_net::MailRequest::Open { refresh: true }));
                }
            }
            IgnoreResult { all, block, result } => {
                let who = if all { "everyone".to_string() } else { self.ignore_pending.take().map_or_else(|| "that player".into(), |(n, _)| n) };
                let msg = match (result, block) {
                    (0, true) => format!("Whispers from {who} are now blocked."),
                    (0, false) => format!("Whispers from {who} are allowed again."),
                    (2, _) => "Your block list is full.".into(),
                    (_, true) => format!("Whispers from {who} were already blocked."),
                    (_, false) => format!("{who} wasn't blocked."),
                };
                self.hud.push_chat(ro_net::ChatKind::System, msg);
            }
            IgnoreList(names) => {
                let msg = if names.is_empty() { "You aren't blocking anyone's whispers.".into() } else { format!("Blocked: {}", names.join(", ")) };
                self.hud.push_chat(ro_net::ChatKind::System, msg);
            }
            BookingResult { delete, result } => {
                self.booking.status = match (delete, result) {
                    (false, 0) => {
                        self.booking.posted = true;
                        "Your ad is posted.".into()
                    }
                    (false, 2) => {
                        self.booking.posted = true;
                        "You already have an ad posted.".into()
                    }
                    (false, _) => "Couldn't post the ad.".into(),
                    (true, 0) => {
                        self.booking.posted = false;
                        "Your ad was removed.".into()
                    }
                    (true, _) => "Couldn't remove the ad.".into(),
                };
                let level = self.hud.status.base_level.clamp(1, 999) as u16;
                let _ = level;
                    self.send(ro_net::Command::Booking(ro_net::BookingRequest::Search { level: 0, map: 0, job: 0xffff }));
            }
            BookingList(ads) => {
                let me = self.hud.status.name.clone();
                self.booking.posted = ads.iter().any(|a| a.leader == me) || self.booking.posted;
                self.booking.ads = ads;
            }
            RoomSign(room) => {
                if room.owner == self.account_id {
                    if let Some(inside) = self.rooms.inside.as_mut().filter(|r| r.0 == 0) {
                        inside.0 = room.id;
                    }
                }
                self.rooms.signs.insert(room.owner, room);
            }
            RoomGone(id) => self.rooms.signs.retain(|_, r| r.id != id),
            RoomCreated(code) => {
                if code == 0 {
                    // The creator gets no member list: the room is just us, as owner.
                    let me = self.hud.status.name.clone();
                    self.rooms.inside = Some((0, vec![(true, me)]));
                }
                let msg = match code {
                    0 => None,
                    1 => Some("Too many chat rooms here."),
                    _ => Some("You already have a chat room."),
                };
                if let Some(m) = msg {
                    self.hud.push_chat(ro_net::ChatKind::System, m.into());
                }
            }
            RoomRefused(code) => {
                let msg = match code {
                    0 => "The room is full.",
                    1 => "Wrong password.",
                    2 => "You were kicked from that room.",
                    4 => "You don't have enough zeny to join.",
                    5 => "Your level is too low for that room.",
                    6 => "Your level is too high for that room.",
                    7 => "Your job can't join that room.",
                    _ => "",
                };
                if !msg.is_empty() {
                    self.hud.push_chat(ro_net::ChatKind::System, msg.into());
                }
            }
            RoomEntered { id, members } => self.rooms.inside = Some((id, members)),
            RoomMember { joined, name, kicked } => {
                let me = self.hud.status.name.clone();
                if !joined && name == me {
                    self.rooms.inside = None;
                    if kicked {
                        self.hud.push_chat(ro_net::ChatKind::System, "You were kicked from the chat room.".into());
                    }
                } else if let Some((_, members)) = self.rooms.inside.as_mut() {
                    if joined {
                        members.push((false, name.clone()));
                        self.hud.push_chat(ro_net::ChatKind::System, format!("{name} joined the chat room."));
                    } else {
                        members.retain(|(_, n)| *n != name);
                        self.hud.push_chat(ro_net::ChatKind::System, format!("{name} left the chat room."));
                    }
                }
            }
            RoomOwner(name) => {
                if let Some((_, members)) = self.rooms.inside.as_mut() {
                    for m in members.iter_mut() {
                        m.0 = m.1 == name;
                    }
                }
            }
            Pet(info) => self.companions.pet = Some(info),
            CompanionState { homun, state, id, value } => {
                if homun {
                    if state == 0 {
                        self.companions.homun_id = id;
                    } else if let Some(h) = self.companions.homun.as_mut() {
                        match state {
                            1 => h.intimacy = value as u16,
                            2 => h.hunger = value as u16,
                            _ => {}
                        }
                    }
                } else if state == 0 {
                    self.companions.pet_id = id;
                    // The server sends the pet's details only when asked.
                    self.send(ro_net::Command::Companion(ro_net::CompanionRequest::PetMenu(0)));
                } else if let Some(p) = self.companions.pet.as_mut() {
                    match state {
                        1 => p.intimacy = value as u16,
                        2 => p.hunger = value as u16,
                        3 => p.accessory = value as u16,
                        _ => {}
                    }
                }
            }
            Homun(info) => self.companions.homun = Some(info),
            CompanionSkills { merc, skills } => {
                if merc {
                    self.companions.merc_skills = skills;
                } else {
                    self.companions.homun_skills = skills;
                }
            }
            Merc(info) => self.companions.merc = Some(info),
            MsgId(id @ (1266 | 1267)) => {
                // The mercenary is gone: killed, or its contract ran out.
                self.companions.merc = None;
                self.companions.merc_skills.clear();
                self.merc_ai = None;
                tracing::info!(id, "mercenary gone");
                let msg = if id == 1267 { "Your mercenary was killed." } else { "Your mercenary's contract has ended." };
                self.hud.push_chat(ro_net::ChatKind::System, msg.into());
            }
            MsgId(id) => match msgstr::get(id as usize, &[]) {
                Some(text) => self.hud.push_chat(ro_net::ChatKind::System, text),
                None => tracing::debug!(id, "server message"),
            },
            Elemental { id, hp, max_hp, sp, max_sp } => {
                let name = self.units.get(&id).map(|m| m.name.clone()).unwrap_or_default();
                self.companions.elem = Some(companions::Elemental { id, name, hp, max_hp, sp, max_sp });
            }
            ElemParam(k, v) => {
                if let Some(e) = self.companions.elem.as_mut() {
                    let u = v.max(0) as u32;
                    match k {
                        5 => e.hp = u,
                        6 => e.max_hp = u,
                        7 => e.sp = u,
                        8 => e.max_sp = u,
                        _ => {}
                    }
                    let id = e.id;
                    let (hp, max) = (e.hp as i32, e.max_hp as i32);
                    if let Some(m) = self.units.get_mut(&id) {
                        m.hp = Some((hp, max));
                    }
                }
            }
            HomunParam(k, v) => {
                if let Some(h) = self.companions.homun.as_mut() {
                    let u = v.clamp(0, u32::MAX as i64) as u32;
                    match k {
                        1 => h.exp = v,
                        5 => h.hp = u,
                        6 => h.max_hp = u,
                        7 => h.sp = u,
                        8 => h.max_sp = u,
                        _ => {}
                    }
                }
            }
            HomunSkill { id, level, sp, range, upgradable } => {
                if let Some(s) = self.companions.homun_skills.iter_mut().find(|s| s.id == id) {
                    s.level = level;
                    s.sp = sp;
                    s.range = range;
                    s.upgradable = upgradable;
                }
            }
            SummonHp { id, hp, max } => {
                if let Some(m) = self.units.get_mut(&id) {
                    let max = max.map(|x| x as i32).or(m.hp.map(|h| h.1)).unwrap_or(hp as i32);
                    m.hp = Some((hp as i32, max));
                }
            }
            MercParam(k, v) => {
                if let Some(m) = self.companions.merc.as_mut() {
                    match k {
                        5 => m.hp = v.max(0) as u32,
                        6 => m.max_hp = v.max(0) as u32,
                        7 => m.sp = v.max(0) as u32,
                        8 => m.max_sp = v.max(0) as u32,
                        _ => {}
                    }
                }
            }
            Achievements { rank, list } => {
                self.progress.set_all(rank, list);
                let ids = self.progress.item_ids();
                self.load_icons_for(&ids);
            }
            AchievementUpdate { rank, ach } => {
                if let Some(name) = self.progress.update(rank, ach) {
                    self.hud.push_chat(ro_net::ChatKind::System, format!("Achievement complete: {name}"));
                    self.play_sound("ui_achievement_complete.wav");
                }
            }
            AchievementReward { failed, id } => {
                if failed {
                    self.hud.push_chat(ro_net::ChatKind::System, "The achievement reward couldn't be given.".into());
                } else {
                    self.progress.rewarded(id);
                    self.hud.push_chat(ro_net::ChatKind::System, format!("Reward collected: {}", self.progress.name(id)));
                }
            }
            TitleChanged { failed, id } => {
                if !failed {
                    self.progress.title = id;
                    let msg = if id == 0 { "Title removed.".into() } else { format!("Title: {}", self.progress.title_name(id)) };
                    self.hud.push_chat(ro_net::ChatKind::System, msg);
                }
            }
            UiOpen { kind: 7, data } => {
                if let Some(vfs) = self.vfs.as_deref() {
                    self.progress.load_art(&self.egui_ctx, vfs);
                }
                self.progress.attendance = Some(data);
            }
            UiOpen { kind: 1, .. } => {
                let (hair, color) = self.player.as_ref().and_then(|p| p.look.as_ref()).map_or((1, 0), |l| (l.hair, l.hair_color));
                self.stylist.open(hair, color, 0);
            }
            UiOpen { kind: 8, .. } => {
                self.grade.open = true;
            }
            GradeOptions(o) => self.grade.options = Some(o),
            GradeResult { index, grade, result } => {
                if let Some(it) = self.inventory.items.get_mut(&index) {
                    it.grade = grade as u8;
                    // A grade raised starts again from +0.
                    if result == 0 {
                        it.refine = 0;
                    }
                }
                if let Some(r) = self.grade.result(grade, result) {
                    self.send(ro_net::Command::Grade(r));
                }
            }
            GradeBroadcast { name, item, grade, ok } => {
                let item = self.item_db.as_ref().map_or_else(|| format!("Item #{item}"), |d| d.name(item, true));
                let letter = grade::LETTERS.get(grade as usize).copied().unwrap_or("?");
                let text = if ok { format!("{name} raised {item} to grade {letter}!") } else { format!("{name} failed to raise the grade of {item}.") };
                self.hud.push_chat(ro_net::ChatKind::Broadcast, text);
            }
            UiOpen { .. } => {}
            Attendance(data) => {
                self.progress.attendance = Some(data);
                self.hud.push_chat(ro_net::ChatKind::System, format!("Attendance checked: day {}. Your reward was sent by mail.", data / 10));
            }
            CashShop { cash, kafra } => {
                self.stores.cash = Some((cash, kafra));
                if let Some(vfs) = self.vfs.as_deref() {
                    self.stores.load_cash_art(&self.egui_ctx, vfs);
                }
                // The server sends the item list once per login.
                if self.stores.cash_tabs.is_empty() {
                    self.send(ro_net::Command::CashShop(ro_net::CashShopRequest::List));
                }
            }
            CashTab { tab, items } => {
                self.stores.cash_tabs.insert(tab, items);
                let ids = self.stores.item_ids();
                self.load_icons_for(&ids);
            }
            CashResult { result, cash, kafra } => {
                if let Some(c) = self.stores.cash.as_mut() {
                    *c = (cash, kafra);
                }
                let msg = match result {
                    0 => "Purchase complete.",
                    2 => "You don't have enough points.",
                    3 => "That item isn't for sale.",
                    4 => "You are carrying too much weight.",
                    5 => "You can't carry that many items.",
                    6 => "You can't buy anything right now.",
                    _ => "The purchase failed.",
                };
                self.hud.push_chat(ro_net::ChatKind::System, msg.into());
            }
            ShopBuyResult(code) => {
                let msg = match code {
                    0 => "Purchase complete.",
                    1 => "You don't have enough zeny.",
                    2 => "You are carrying too much weight.",
                    4 => "The shop is out of that item.",
                    5 => "The shop has changed; look again.",
                    6 => "The seller cannot take more zeny.",
                    7 => "You cannot carry that many items.",
                    _ => "The purchase failed.",
                };
                self.hud.push_chat(ro_net::ChatKind::System, msg.into());
            }
            Bank { balance, reply } => {
                if let Some((b, _)) = self.merchant.bank.as_mut() {
                    *b = Some(balance);
                }
                if let Some((deposit, reason)) = reply.filter(|(_, r)| *r != 0) {
                    let why = match (deposit, reason) {
                        (true, 1) | (false, 1) => "The bank is busy; try again.",
                        (true, 2) => "You don't have that much zeny.",
                        (false, 2) => "The bank doesn't hold that much.",
                        (_, 3) => "That would exceed the limit.",
                        _ => "The bank couldn't do that.",
                    };
                    self.hud.push_chat(ro_net::ChatKind::System, why.into());
                }
            }
            ViewEquip { name, job, items } => {
                let items: Vec<ro_game::items::InvItem> = items.into_iter().map(inv_item).collect();
                let ids: Vec<u32> = items.iter().map(|i| i.id).collect();
                self.load_icons_for(&ids);
                self.item_info.viewing = Some((name, job_title(job).to_string(), items));
            }
            ShowEquip(on) => self.show_equip = Some(on),
            RefineOpen => {
                self.refine.open = true;
                self.refine.selected = None;
                self.windows.inventory = true;
            }
            RefineInfo { index, blessing, options } => {
                let ids: Vec<u32> = options.iter().map(|o| o.material).collect();
                self.load_icons_for(&ids);
                self.refine.selected = Some((index, blessing, options));
            }
            RefineResult { result, index, refine } => {
                let name = self.inventory.items.get(&index).and_then(|i| self.item_db.as_ref().map(|d| d.name(i.id, true))).unwrap_or_else(|| "The item".into());
                let (msg, sound) = match result {
                    0 => (format!("Refine succeeded: +{refine} {name}!"), "bs_refinesuccess.wav"),
                    1 => (format!("Refine failed: {name} broke."), "bs_refinefailed.wav"),
                    2 => (format!("Refine failed: {name} went down to +{refine}."), "bs_refinefailed.wav"),
                    _ => (format!("Refine failed: {name} stays +{refine}."), "bs_refinefailed.wav"),
                };
                if let Some(it) = self.inventory.items.get_mut(&index) {
                    it.refine = refine as u8;
                }
                self.play_sound(sound);
                self.hud.push_chat(ro_net::ChatKind::System, msg);
            }
            SkillCooldown { skill, ms } => {
                self.cooldowns.insert(skill, std::time::Instant::now() + std::time::Duration::from_millis(ms as u64));
            }
            Emotion { id, kind } => {
                self.emotes.insert(id, (kind, 0.0));
            }
            WhisperResult(code) => {
                let why = match code {
                    0 => return,
                    1 => "That character is not online.",
                    2 => "Your whisper was ignored.",
                    _ => "That character is ignoring all whispers.",
                };
                self.hud.push_chat(ro_net::ChatKind::System, why.into());
            }
            Npc(d) => self.npc_dialog(d),
            Vanish { id, reason } => {
                if self.companions.elem.as_ref().is_some_and(|e| e.id == id) && reason != 0 {
                    self.companions.elem = None;
                }
                self.warps.remove(&id);
                if self.attack.is_some_and(|(t, _)| t == id) {
                    self.attack = None;
                }
                if id == self.account_id {
                    if reason == 1 {
                        if let Some(p) = self.player.as_mut() {
                            p.set_pose(Pose::Dead, f32::MAX);
                        }
                        self.hud.push_chat(ro_net::ChatKind::System, "You have died.".into());
                        self.feel(haptics::Cue::Death);
                        self.dead = true;
                        self.attack = None;
                        self.walk_goal = None;
                    }
                } else if reason == 1 {
                    // A boss with a timed spawn here: its MVP timer starts.
                    if let (Some(m), Some(w)) = (self.units.get(&id).filter(|m| m.kind == 5), self.world.as_ref()) {
                        if mvptimers::MvpTimers::died(&mut self.settings.mvp_deaths, &w.name, m.class as u16, &m.name, mvptimers::now_unix()) {
                            self.settings.save();
                        }
                    }
                    // Died: play the death animation, then remove.
                    let death_3d = self.units.get(&id).and_then(|m| self.models3d.get(&m.class)).and_then(|l| l.as_ref()).map(|l| l.death_ms());
                    if let Some(m) = self.units.get_mut(&id) {
                        m.walker.stop_at(m.walker.cell);
                        m.set_pose(Pose::Dead, f32::MAX);
                        m.dying = Some(death_3d.map_or(1500.0, |ms| ms.max(500.0)));
                    }
                } else {
                    self.units.remove(&id);
                }
            }
            Act(act) => {
                if act.target != 0 && act.src != act.target {
                    self.aggro.insert(act.src, act.target);
                }
                self.feel_act(&act);
                self.log_combat(0, &act);
                self.act(act)
            }
            OutOfRange { target, target_cell, range } => {
                // Walk toward the target; the attack is re-sent once in reach.
                self.chasing = true;
                self.attack = Some((target, range.max(1)));
                self.request_walk(target_cell);
            }
            LookChange { id, kind, value, value2 } => self.look_change(id, kind, value, value2),
            StorageOpen => {
                self.social.storage.items.clear();
                self.social.storage.open = true;
                // Talking to the Kafra ends the dialog the storage was opened from.
                self.dialog = None;
                self.cutin = None;
            }
            StorageItems(list) => {
                for it in list {
                    self.social.storage.items.insert(it.index, inv_item(it));
                }
                self.load_storage_icons();
            }
            StorageAdded(it) => {
                let it = inv_item(it);
                match self.social.storage.items.get_mut(&it.index) {
                    Some(s) if s.id == it.id => s.amount += it.amount,
                    _ => {
                        self.social.storage.items.insert(it.index, it);
                    }
                }
                self.load_storage_icons();
            }
            StorageRemoved { index, amount } => {
                if let Some(s) = self.social.storage.items.get_mut(&index) {
                    s.amount = s.amount.saturating_sub(amount);
                    if s.amount == 0 {
                        self.social.storage.items.remove(&index);
                    }
                }
            }
            StorageCount { used, capacity } => self.social.storage.count = (used, capacity),
            StorageClosed => self.social.storage.open = false,
            ShopDeal(npc) => {
                self.dialog = None;
                self.social.shop = Some(social::Shop::Choose { npc });
                if let Some(buy) = self.auto_deal.take() {
                    self.send(ro_net::Command::Shop(ro_net::ShopRequest::Deal { npc, buy }));
                }
            }
            ShopBuyList(items) => {
                if let Some(id) = self.auto_buy.take() {
                    tracing::info!(id, "buying");
                    self.send(ro_net::Command::Shop(ro_net::ShopRequest::Buy(vec![(id, 1)])));
                }
                self.social.shop = Some(social::Shop::Buy { items, cart: Default::default() });
                self.load_shop_icons();
            }
            RankPoints { kind, gained, total } => {
                let (id, en) = match kind {
                    0 => (901, "Blacksmith rank"),
                    1 => (902, "Alchemist rank"),
                    2 => (926, "Tae-Kwon Mission rank"),
                    3 => (953, "Slayer rank"),
                    _ => (1209, "Ranking"),
                };
                let (g, t) = (gained.to_string(), total.to_string());
                let text = msgstr::get(id, &[&g, &t]).unwrap_or_else(|| format!("[Point] You have been rewarded with {g} {en} points. Your point total is {t}."));
                self.hud.push_chat(ro_net::ChatKind::System, text);
            }
            Ping(ms) => self.ping = Some(ms),
            MannerResult(result) => {
                let text = match result {
                    0 => Some(msgstr::get(854, &[]).unwrap_or_else(|| "A manner point has been successfully aligned.".into())),
                    3 => Some("Chat Block has been applied by GM due to your ill-mannerous action.".into()),
                    4 => Some("Automated Chat Block has been applied due to Anti-Spam System.".into()),
                    _ => None,
                };
                if let Some(text) = text {
                    self.hud.push_chat(ro_net::ChatKind::System, text);
                }
            }
            Muted { muted, by } => {
                let (id, en) = if muted { (388, "negative") } else { (387, "positive") };
                let text = msgstr::get(id, &[&by]).unwrap_or_else(|| format!("Received {en} manner point from {by}."));
                self.hud.push_chat(ro_net::ChatKind::System, text);
            }
            PointShop { cash, kafra, items } => {
                self.dialog = None;
                if let Some(id) = self.auto_buy.take() {
                    tracing::info!(id, "buying with points");
                    self.send(ro_net::Command::Shop(ro_net::ShopRequest::CashBuy { kafra: 0, items: vec![(id, 1)] }));
                }
                self.social.shop = Some(social::Shop::Cash { items, cart: Default::default(), cash, kafra, use_kafra: 0 });
                self.load_shop_icons();
            }
            PointShopResult { cash: c, kafra: k, error } => {
                if let Some(social::Shop::Cash { cash, kafra, cart, use_kafra, .. }) = self.social.shop.as_mut() {
                    (*cash, *kafra) = (c, k);
                    if error == 0 {
                        cart.clear();
                        *use_kafra = 0;
                    }
                }
                let text = match error {
                    0 => "The deal has successfully completed.",
                    1 => "The purchase has failed because the NPC does not exist.",
                    3 => "You are over your Weight Limit.",
                    4 => "You cannot purchase items while you are in a trade.",
                    5 => "The purchase has failed because the item information was incorrect.",
                    6 => "You do not have enough points.",
                    7 => "You can purchase up to 10 items.",
                    8 => "Some items could not be purchased.",
                    _ => "The purchase has failed.",
                };
                self.hud.push_chat(ro_net::ChatKind::System, text.into());
            }
            ShopSellList(offers) => {
                if let Some(id) = self.auto_sell.take() {
                    let list: Vec<(u16, u16)> = offers
                        .iter()
                        .filter_map(|o| self.inventory.items.get(&o.index).filter(|i| i.id == id).map(|i| (i.index, i.amount)))
                        .collect();
                    tracing::info!(?list, "selling");
                    self.send(ro_net::Command::Shop(ro_net::ShopRequest::Sell(list)));
                }
                self.social.shop = Some(social::Shop::Sell { offers, cart: Default::default() });
            }
            ShopResult { buy, ok } => {
                let msg = match (buy, ok) {
                    (true, true) => "Purchase complete.",
                    (true, false) => "You could not buy that (not enough zeny, weight or space).",
                    (false, true) => "Sale complete.",
                    (false, false) => "You could not sell that.",
                };
                self.hud.push_chat(ro_net::ChatKind::System, msg.into());
            }
            TradeRequested { name, .. } => self.social.prompts.push(social::Prompt::Trade { name }),
            TradeAnswer(result) => match result {
                3 => {
                    let partner = self.trade_partner.take().unwrap_or_default();
                    self.social.trade = Some(social::Trade { partner, ..Default::default() });
                }
                r => {
                    let why = match r {
                        0 => "too far away",
                        1 => "that character is not available",
                        2 => "they are already trading",
                        4 => "they declined",
                        _ => "they are busy",
                    };
                    self.hud.push_chat(ro_net::ChatKind::System, format!("Trade failed: {why}."));
                }
            },
            TradeItem { id, amount, .. } => {
                if let Some(t) = self.social.trade.as_mut() {
                    if id == 0 {
                        t.zeny_theirs = amount;
                    } else {
                        t.theirs.push((id, amount));
                    }
                }
                self.load_icons_for(&[id]);
            }
            TradeAdded { index, ok } => {
                if let (true, Some(t)) = (ok, self.social.trade.as_mut()) {
                    if let Some(it) = self.inventory.items.get(&index) {
                        let amount = self.trade_offers.remove(&index).unwrap_or(it.amount as i32);
                        t.ours.push((it.id, amount));
                    }
                }
            }
            TradeLocked { partner } => {
                if let Some(t) = self.social.trade.as_mut() {
                    if partner {
                        t.they_locked = true;
                    } else {
                        t.we_locked = true;
                    }
                }
            }
            TradeEnded { completed } => {
                self.trade_stage = 0;
                tracing::info!(completed, "trade ended");
                self.social.trade = None;
                let msg = if completed { "Trade completed." } else { "Trade cancelled." };
                self.hud.push_chat(ro_net::ChatKind::System, msg.into());
            }
            Party { name, members } => {
                let hp = self.social.party.take().map(|p| p.hp).unwrap_or_default();
                self.social.party = Some(social::Party { name, members, hp });
            }
            PartyMember { name, member } => {
                let p = self.social.party.get_or_insert_with(|| social::Party { name: name.clone(), ..Default::default() });
                p.members.retain(|m| m.account_id != member.account_id);
                p.members.push(member);
            }
            PartyLeft { account_id, name } => {
                if account_id == self.account_id {
                    self.social.party = None;
                    self.hud.push_chat(ro_net::ChatKind::System, "You left the party.".into());
                } else if let Some(p) = self.social.party.as_mut() {
                    p.members.retain(|m| m.account_id != account_id);
                    self.hud.push_chat(ro_net::ChatKind::Party, format!("{name} left the party."));
                }
            }
            PartyHp { account_id, hp, max_hp } => {
                if let Some(p) = self.social.party.as_mut() {
                    p.hp.insert(account_id, (hp, max_hp));
                }
            }
            PartyInvite { party, name } => self.social.prompts.push(social::Prompt::Party { party, name }),
            WebAuth { account_id, token, world } => {
                let login = agency::Login { url: self.web_url.clone(), aid: account_id, token, world };
                self.emblem_web.set_login(login.clone());
                self.agency.set_login(login);
            }
            AgencyAsk(ask) => self.social.prompts.push(social::Prompt::Agency(ask)),
            AgencyResult { leader, refused, .. } => {
                let (id, en) = if refused { (3499, "rejected") } else { (3498, "approved") };
                let text = msgstr::get(id, &[&leader]).unwrap_or_else(|| format!("[{leader}]'s party leader has {en} your join request."));
                self.hud.push_chat(ro_net::ChatKind::System, text);
            }
            PartyCreated { ok } => {
                let msg = if ok { "Party created." } else { "Could not create the party (name taken, or already in one)." };
                self.hud.push_chat(ro_net::ChatKind::System, msg.into());
            }
            Guild { id, name } => self.social.guild = (id != 0 && !name.is_empty()).then_some(name),
            GuildBelong { id, master, name } => {
                (self.guild.id, self.guild.master) = (id, master);
                if id == 0 {
                    (self.guild.info, self.guild.members) = (None, vec![]);
                } else {
                    self.guild.name = name.clone();
                    self.guild.needs_refresh = true;
                }
                self.social.guild = (id != 0 && !name.is_empty()).then_some(name);
            }
            GuildCreated(result) => {
                let text = match result {
                    0 => "Guild created.",
                    1 => "You are already in a guild.",
                    2 => "That guild name is already taken.",
                    3 => "You need an Emperium to found a guild.",
                    _ => "Could not create the guild.",
                };
                self.hud.push_chat(ro_net::ChatKind::System, text.into());
                if result == 0 {
                    self.guild.open = true;
                    self.guild.needs_refresh = true;
                }
            }
            GuildInfo(info) => {
                self.guild.name = info.name.clone();
                if let Some(p) = self.player.as_mut() {
                    p.guild = info.id;
                }
                // Fetch the emblem when it is new or changed.
                if info.emblem != 0 && self.guild.emblem.as_ref().is_none_or(|e| e.0 != info.emblem) {
                    // The guild window needs it even if name labels already have it.
                    self.emblem_versions.remove(&info.id);
                    self.want_emblem(info.id, info.emblem);
                }
                self.guild.info = Some(info);
            }
            GuildEmblem { guild, version, data } => {
                if let Some(raw) = guild::inflate_emblem(&data) {
                    self.show_emblem(guild, version, &raw);
                }
            }
            GuildMembers(m) => self.guild.members = m,
            GuildRankNames(names) => {
                for (id, name) in names {
                    match self.guild.ranks.iter_mut().find(|r| r.id == id) {
                        Some(r) => r.name = name,
                        None => self.guild.ranks.push(ro_net::Position { id, name, ..Default::default() }),
                    }
                }
                self.guild.ranks.sort_by_key(|r| r.id);
            }
            GuildRankInfo(info) => {
                for (id, mode, tax) in info {
                    if let Some(r) = self.guild.ranks.iter_mut().find(|r| r.id == id) {
                        (r.mode, r.tax) = (mode, tax);
                    }
                }
            }
            GuildRanksChanged(list) => {
                for p in list {
                    match self.guild.ranks.iter_mut().find(|r| r.id == p.id) {
                        Some(r) => *r = p,
                        None => self.guild.ranks.push(p),
                    }
                }
                self.guild.ranks.sort_by_key(|r| r.id);
            }
            GuildRankMoves(moves) => {
                for (_, char_id, position) in moves {
                    if let Some(m) = self.guild.members.iter_mut().find(|m| m.char_id == char_id) {
                        m.position = position;
                    }
                }
            }
            GuildSkills { points, skills } => (self.guild.skill_points, self.guild.skills) = (points, skills),
            GuildNotice { subject, text } => self.guild.notice = (subject, text),
            GuildInvited { guild, name } => self.social.prompts.push(social::Prompt::Guild { guild, name }),
            GuildInviteResult(r) => {
                let text = match r {
                    0 => "That player is already in a guild.",
                    1 => "Your guild invitation was declined.",
                    2 => "A new member joined the guild.",
                    3 => "The guild is full.",
                    _ => "Guild invitation failed.",
                };
                self.hud.push_chat(ro_net::ChatKind::Guild, text.into());
                if r == 2 {
                    self.guild.needs_refresh = true;
                }
            }
            GuildMemberState { char_id, online, .. } => {
                if let Some(m) = self.guild.members.iter_mut().find(|m| m.char_id == char_id) {
                    m.online = online;
                }
            }
            GuildMemberGone { char_id, reason, expelled } => {
                let who = self.guild.members.iter().find(|m| m.char_id == char_id).map_or_else(|| "A member".to_string(), |m| m.name.clone());
                let verb = if expelled { "was expelled from" } else { "left" };
                let why = if reason.trim().is_empty() { String::new() } else { format!(" ({})", reason.trim()) };
                self.hud.push_chat(ro_net::ChatKind::Guild, format!("{who} {verb} the guild{why}."));
                self.guild.members.retain(|m| m.char_id != char_id);
            }
            Friends(list) => self.social.friends = list,
            FriendOnline { account_id, online, .. } => {
                if let Some(f) = self.social.friends.iter_mut().find(|f| f.account_id == account_id) {
                    f.online = online;
                }
            }
            FriendRequested { account_id, char_id, name } => self.social.prompts.push(social::Prompt::Friend { account_id, char_id, name }),
            FriendAdded { ok, friend } => {
                if ok {
                    self.hud.push_chat(ro_net::ChatKind::System, format!("{} is now your friend.", friend.name));
                    self.social.friends.retain(|f| f.account_id != friend.account_id);
                    self.social.friends.push(friend);
                }
            }
            FriendRemoved { account_id, .. } => self.social.friends.retain(|f| f.account_id != account_id),
            InventoryStart => self.inventory.items.clear(),
            Items(list) => {
                for it in list {
                    self.inventory.set(inv_item(it));
                }
                self.load_icons();
            }
            ItemAdded(it) => {
                tracing::debug!(id = it.id, amount = it.amount, "item added");
                let name = self.item_db.as_ref().map_or_else(|| format!("Item #{}", it.id), |d| d.name(it.id, it.identified));
                self.hud.push_chat(ro_net::ChatKind::System, format!("You got {name} ({}).", it.amount));
                self.feel(haptics::Cue::Pickup);
                self.inventory.add(inv_item(it));
                self.load_icons();
            }
            ItemRemoved { index, amount } => self.inventory.remove(index, amount),
            ItemUsed { index, amount, user, ok } => {
                if ok && user == self.account_id {
                    self.inventory.set_amount(index, amount);
                }
            }
            Equipped { index, slots, ok, .. } => {
                tracing::debug!(index, slots, ok, "equip");
                if ok {
                    self.inventory.set_equipped(index, slots);
                } else {
                    self.hud.push_chat(ro_net::ChatKind::System, "You cannot equip this item.".into());
                }
            }
            Unequipped { index, ok, .. } => {
                if ok {
                    self.inventory.set_equipped(index, 0);
                }
            }
            FloorItem(f) => {
                tracing::debug!(id = f.id, item = f.item, x = f.x, y = f.y, effect = ?f.drop_effect, "floor item");
                // Cards and rare drops fall with a light pillar that stays until picked up.
                if let Some((pillar, glow, sound)) = f.drop_effect.and_then(fx::drop_pillar) {
                    let (fx, fy) = floor_cell(&f);
                    let (x, z) = (fx * ro_render::CELL, fy * ro_render::CELL);
                    if let Some(w) = self.world.as_ref() {
                        let pos = glam::Vec3::new(x, w.floor_at(x, z), z);
                        self.play_fx_tagged(glow, pos, f.id);
                        self.play_fx_tagged(pillar, pos, f.id);
                        if let (Some(audio), Some(vfs)) = (self.audio.as_ref(), self.vfs.as_deref()) {
                            audio.play(vfs, sound);
                        }
                    }
                }
                let actor = self.item_actor(f.item, f.identified);
                self.floor.insert(f.id, (f, actor));
            }
            FloorItemGone(id) => {
                self.floor.remove(&id);
                self.stop_fx(id);
                if self.pending_pickup == Some(id) {
                    self.pending_pickup = None;
                }
            }
            CardTargets(equips) => {
                if let Some((_, list)) = self.windows.card_pick.as_mut() {
                    *list = equips;
                }
            }
            CardInserted { equip, card, ok } => {
                // The server takes the card without telling us (pc_insert_card deletes it
                // silently), as the original client expects: on success, one card leaves the
                // stack and goes into the equipment's first free slot.
                let card_id = self.inventory.items.get(&card).map(|c| c.id);
                if let (true, Some(id)) = (ok, card_id) {
                    if let Some(slot) = self.inventory.items.get_mut(&equip).and_then(|e| e.cards.iter_mut().find(|c| **c == 0)) {
                        *slot = id;
                    }
                    self.inventory.remove(card, 1);
                }
                let msg = if ok { "The card was inserted." } else { "The card could not be inserted." };
                self.hud.push_chat(ro_net::ChatKind::System, msg.to_string());
            }
            Stats(s) => self.stats = Some(s),
            StatRaised { stat, ok, value } => {
                // The ack carries one byte; past 255 the full value follows in 0x0141.
                if let (true, Some(s), true) = (ok, self.stats.as_mut(), value < 255) {
                    match stat {
                        13..=18 => s.base[(stat - 13) as usize] = value as u16,
                        219..=224 => s.traits[(stat - 219) as usize] = value as u16,
                        _ => {}
                    }
                }
            }
            StatBonus { stat, base, plus } => {
                if let Some(s) = self.stats.as_mut() {
                    let base = base.clamp(0, u16::MAX as i32) as u16;
                    match stat {
                        13..=18 => (s.base[(stat - 13) as usize], s.plus[(stat - 13) as usize]) = (base, plus as i16),
                        219..=224 => (s.traits[(stat - 219) as usize], s.trait_plus[(stat - 219) as usize]) = (base, plus as i16),
                        _ => {}
                    }
                }
            }
            StatCost { stat, cost } => {
                if let Some(s) = self.stats.as_mut() {
                    match stat {
                        32..=37 => s.cost[(stat - 32) as usize] = cost,
                        247..=252 => s.trait_cost[(stat - 247) as usize] = cost,
                        _ => {}
                    }
                }
            }
            Skills(list) => {
                self.skills = list;
                self.load_skill_icons();
            }
            SkillUpdate { id, level, sp, range, upgradable } => {
                if let Some(s) = self.skills.iter_mut().find(|s| s.id == id) {
                    (s.level, s.sp, s.range, s.upgradable) = (level, sp, range, upgradable);
                }
            }
            Hotkeys { row, keys } => {
                let n = self.hotkeys.len();
                self.hotkeys = keys;
                self.hotkeys.resize(n.max(self.hotkeys.len()), Default::default());
                self.hotkey_row = row % HOTKEY_ROWS;
            }
            Casting(c) => {
                let cell = (c.cell.0 as i32, c.cell.1 as i32);
                let target_cell = self.mob_mut(c.target).map(|m| m.walker.cell).or((c.target == 0).then_some(cell));
                if let Some(m) = self.mob_mut(c.src) {
                    if let Some(t) = target_cell {
                        m.face(t);
                    }
                    if c.ms > 0 {
                        m.set_pose(Pose::Cast, c.ms as f32);
                    }
                }
                if c.ms > 0 && c.src == self.account_id {
                    self.feel(haptics::Cue::Charge(c.ms));
                }
                if c.ms > 0 {
                    self.casts.insert(c.src, (0.0, c.ms as f32));
                    self.cast_skill.insert(c.src, c.skill);
                    self.cast_fx(&c);
                }
            }
            Fed { homun, ok, item } => {
                let name = self.item_db.as_ref().map_or_else(|| format!("Item #{item}"), |d| d.name(item as u32, true));
                let who = if homun { "homunculus" } else { "pet" };
                let text = if ok { format!("Your {who} ate {name}.") } else { format!("You don't have any {name} to feed your {who}.") };
                self.hud.push_chat(ro_net::ChatKind::System, text);
            }
            PetAct { id, value } => {
                if value < 10_000 {
                    self.handle(Emotion { id, kind: value as u8 });
                } else if let Some(name) = self.units.get(&id).map(|m| m.name.clone()) {
                    let names = &self.class_names;
                    if let Some(line) = self.pet_talk.line(value, |c| names.get(&c).cloned()) {
                        self.hud.push_chat(ro_net::ChatKind::Public, format!("{name} : {line}"));
                    }
                }
            }
            Progress { secs } => {
                tracing::debug!(secs, "script progress bar");
                self.script_progress = Some((std::time::Instant::now(), secs.max(1) as f32 * 1000.0));
            }
            ProgressCancel => self.script_progress = None,
            ReadBook { id, page } => self.open_book(id, page),
            CaptchaImage(zipped) => {
                if self.captcha.set_image(&self.egui_ctx, &zipped) {
                    self.send(ro_net::Command::Captcha(ro_net::CaptchaRequest::Downloaded));
                }
            }
            CaptchaAsk { tries, timeout_ms } => self.captcha.ask(tries, timeout_ms),
            CaptchaDone(status) => {
                let text = self.captcha.done(status);
                self.hud.push_chat(ro_net::ChatKind::System, text.into());
            }
            CaptchaApplied(status) => {
                let text = match status {
                    0 => "The player is now being checked by the macro detector.",
                    1 => "There are no captchas registered.",
                    _ => "That player is already being checked.",
                };
                self.hud.push_chat(ro_net::ChatKind::System, text.into());
            }
            Sale(note) => {
                let found = matches!(note, ro_net::SaleNote::Found(Some(_)) | ro_net::SaleNote::Selling { .. });
                let db = self.item_db.as_ref();
                if let Some(line) = self.sales.apply(note, |id| db.map_or_else(|| format!("item {id}"), |d| d.name(id, true))) {
                    self.hud.push_chat(ro_net::ChatKind::Broadcast, line);
                }
                if found {
                    let ids: Vec<u32> = self.sales.on_sale.keys().copied().collect();
                    self.load_icons_for(&ids);
                }
            }
            Roulette(note) => {
                let db = self.item_db.as_ref();
                self.roulette.apply(note, |id| db.map_or_else(|| format!("item {id}"), |d| d.name(id, true)));
                let ids = self.roulette.items();
                self.load_icons_for(&ids);
            }
            UnitProgress { id, secs } => {
                self.unit_progress.insert(id, (0.0, secs.max(1) as f32 * 1000.0));
            }
            ShowScript { id, text } => {
                self.shouts.insert(id, (text, 0.0, false));
            }
            ShowDigit { kind, value } => self.digit = Some((kind, value, 0.0)),
            Devotion { src, targets } if targets.is_empty() => {
                self.devotions.remove(&src);
            }
            Devotion { src, targets } => {
                self.devotions.insert(src, targets);
            }
            Instance(note) => {
                if let Some(text) = self.memorial.apply(note) {
                    self.hud.push_chat(ro_net::ChatKind::System, text);
                }
            }
            // As the original: only with its "Experience message" chat option, except quest
            // rewards. Losses (death) aren't shown.
            ExpGain { amount, kind, quest } => {
                if amount > 0 && (quest || self.settings.exp_messages) {
                    let n = amount.to_string();
                    let (id, what) = if kind == 2 { (488, "Job") } else { (487, "Base") };
                    let text = msgstr::get(id, &[&n]).unwrap_or_else(|| format!("You got {n} {what} EXP."));
                    self.hud.push_chat(ro_net::ChatKind::System, text);
                }
            }
            Recovery { kind, amount } => {
                // HP in green, SP in blue, rising over our head like the original.
                let color = if kind == 7 { egui::Color32::from_rgb(110, 160, 255) } else { egui::Color32::from_rgb(110, 255, 110) };
                if let Some(p) = self.player.as_ref() {
                    self.floaters.push((p.actor.pos, amount.to_string(), color, 0.0));
                }
            }
            UserCount(n) => {
                let text = msgstr::get(178, &[&n.to_string()]).unwrap_or_else(|| format!("There are {n} Players Currently Connected."));
                self.hud.push_chat(ro_net::ChatKind::System, text);
            }
            PvpRank { id, rank, total } => {
                if id == self.account_id {
                    // PvP switched off sends 0 of 0: hide the rank.
                    self.pvp_rank = (total > 0).then_some((rank, total));
                }
            }
            PvpInfo { won, lost, points } => {
                self.hud.push_chat(ro_net::ChatKind::System, format!("You have won {won} times and have lost {lost} times in PVP. Current points {points}."));
            }
            PartyLeader { old, new } => {
                for m in self.social.party.iter_mut().flat_map(|p| p.members.iter_mut()) {
                    m.leader = m.account_id == new || (m.leader && m.account_id != old);
                }
            }
            PartyDead { id, dead } => {
                if dead {
                    self.social.party_dead.insert(id);
                } else {
                    self.social.party_dead.remove(&id);
                }
            }
            ItemObtained { who, item, from_box, source } => {
                let name = |id: u32| self.item_db.as_ref().map_or_else(|| format!("item {id}"), |db| db.name(id, true));
                let text = match from_box {
                    Some(b) => format!("[{who}] has obtained [{}] from [{}].", name(item), name(b)),
                    None if !source.is_empty() => format!("[{who}] has obtained [{}] from [{source}].", name(item)),
                    None => format!("[{who}] has obtained [{}].", name(item)),
                };
                self.hud.push_chat(ro_net::ChatKind::Broadcast, text);
            }
            RefineBroadcast { who, item, level, ok } => {
                let name = self.item_db.as_ref().map_or_else(|| format!("item {item}"), |db| db.name(item, true));
                let text = if ok { format!("[{who}] successfully upgrades an equipment, [+{level} {name}] obtained.") } else { format!("[{who}] fails to upgrade [+{level} {name}].") };
                self.hud.push_chat(ro_net::ChatKind::Broadcast, text);
            }
            CastCancel(id) => {
                if id == self.account_id {
                    self.haptics.end_charge();
                }
                self.casts.remove(&id);
                if let Some(r) = self.renderer.as_mut() {
                    r.procs.retain(|p| p.shape.cast_owner() != Some(id));
                }
                if let Some(m) = self.mob_mut(id) {
                    if matches!(m.pose, Some((Pose::Cast, _))) {
                        m.pose = None;
                    }
                }
            }
            SkillFailed { skill, cause, amount, item } => {
                self.haptics.end_charge();
                self.feel(haptics::Cue::Fail);
                let name = self.skills.iter().find(|s| s.id == skill).map(|s| self.skill_names.display(&s.name)).unwrap_or_else(|| "Skill".into());
                let why = match cause {
                    1 => "not enough SP",
                    2 => "not enough HP",
                    3 => "missing required items",
                    4 => "still on cooldown",
                    5 => "not enough zeny",
                    6 => "wrong weapon equipped",
                    7 => "requires a Red Gemstone",
                    8 => "requires a Blue Gemstone",
                    9 => "carrying too much weight",
                    11 => "invalid target",
                    13 => "requires Holy Water",
                    15 | 22 => "already in effect",
                    16 => "requires another skill first",
                    23 => "conditions not met",
                    26 => "can't be used here",
                    28 => "too close to a wall",
                    32 => "no poison applied to the weapon",
                    33 | 50 => "requires a Mado Gear",
                    34 => "requires Kunai",
                    35 => "can only target players",
                    43 => "requires a Guillotine poison",
                    57 => "requires a cart",
                    71 => "requires an item",
                    72 | 95 => "requires a weapon",
                    73 => "must follow a combo",
                    74 | 96 => "not enough spirit spheres",
                    83 => "there are NPCs too close",
                    84 | 90 => "not enough bullets",
                    87 => "can't be used on this map",
                    89 => "a shop is already open",
                    91 => "requires arrows",
                    92 => "requires a party",
                    93 => "requires party leader",
                    97 => "requires Weapon Blocking",
                    98 => "too many traps",
                    100 => "not enough AP",
                    110 => "requires a shield",
                    _ => "failed",
                };
                self.casts.remove(&self.account_id);
                if let Some(p) = self.player.as_mut() {
                    p.pose = None;
                }
                // Basic Skill refusals name what was refused (trade, emotions, sit, chat room,
                // party, shout, PK, aligning); rAthena also sends "emotions" when emotes come
                // faster than one a second or two.
                let basic = match (skill, cause, amount) {
                    (1, 0, n @ 0..=6) => msgstr::get(159 + n as usize, &[]),
                    (1, 0, 7) => msgstr::get(383, &[]),
                    _ => None,
                };
                let why = if cause == 71 && item != 0 {
                    let item_name = self.item_db.as_ref().map_or_else(|| format!("Item #{item}"), |d| d.name(item, true));
                    format!("requires {amount} {item_name}")
                } else {
                    why.to_string()
                };
                self.hud.push_chat(ro_net::ChatKind::System, basic.unwrap_or_else(|| format!("{name}: {why}.")));
            }
            SkillUsed { skill, src, target } => {
                self.feel_skill_used(src, target);
                self.casts.remove(&src);
                let target_cell = self.mob_mut(target).map(|m| m.walker.cell);
                if let Some(m) = self.mob_mut(src) {
                    if let (Some(t), true) = (target_cell, src != target) {
                        m.face(t);
                    }
                    // A skill that deals no damage (a buff, a heal, a support skill): characters
                    // play their casting motion, as the original client does; monsters, which
                    // have none, their attack.
                    m.set_pose(if m.human { Pose::Cast } else { Pose::Attack }, 400.0);
                }
                if skill == 406 {
                    self.meteor_assault(src);
                }
                self.skill_fx(skill, self.unit_pos(target), false);
            }
            SkillHit { skill, act } => {
                tracing::debug!(skill, src = act.src, target = act.target, damage = act.damage, hits = act.hits, "skill hit");
                self.feel_act(&act);
                // Our skill landed: a pulse per hit.
                if act.src == self.account_id {
                    self.haptics.end_charge();
                    if act.damage > 0 && act.target != self.account_id {
                        self.feel(haptics::Cue::Strike(act.hits.max(1) as u16));
                    }
                }
                let target = act.target;
                // Grimtooth: a line of spikes erupting from the caster to the target.
                if skill == 137 {
                    if let (Some(from), Some(to)) = (self.unit_pos(act.src), self.unit_pos(target)) {
                        self.spike_line(from, to);
                    }
                }
                // Jupitel Thunder: a ball of lightning hurled at the target.
                if skill == 84 {
                    if let (Some(from), Some(to)) = (self.unit_pos(act.src), self.unit_pos(target)) {
                        self.jupitel_fx(from, to, act.hits as u32);
                    }
                }
                // Soul Breaker: a ball of energy hurled at the target.
                if skill == 379 {
                    if let (Some(from), Some(to)) = (self.unit_pos(act.src), self.unit_pos(target)) {
                        let shape = ro_render::Shape::Slash { travel: to - from, size: (1.0, 2.0) };
                        self.proc_fx("purpleslash.tga", shape, from, [1.0, 1.0, 1.0], 500.0);
                    }
                }
                if skill == 406 {
                    self.meteor_assault(act.src);
                }
                if let (7015, Some(from), Some(to)) = (skill, self.unit_pos(act.src), self.unit_pos(target)) {
                    self.crescent_gale(act.src, from, to);
                }
                self.log_combat(skill, &act);
                self.act(act);
                self.skill_hit_fx(skill, self.unit_pos(target));
            }
            SkillAt { skill, src, cell } => {
                if src == self.account_id {
                    self.haptics.end_charge();
                    self.feel(haptics::Cue::Ground);
                }
                self.casts.remove(&src);
                if let Some(m) = self.mob_mut(src) {
                    m.face((cell.0 as i32, cell.1 as i32));
                    m.set_pose(Pose::Attack, 400.0);
                }
                self.skill_fx(skill, self.cell_pos(cell), true);
            }
            Font { id, font } => self.font_change(id, font),
            UnitOption { id, option } => {
                let me = id == self.account_id;
                let costume = costume_job(option);
                let mut changed = None;
                if let Some(m) = self.mob_mut(id) {
                    m.actor.alpha = option_alpha(option, me);
                    if m.costume != costume {
                        m.costume = costume;
                        changed = m.look.clone();
                    }
                }
                if let Some(look) = changed {
                    self.redress(id, look);
                }
            }
            GroundEffect { id, cell, kind } => {
                tracing::debug!(id, ?cell, kind = format_args!("{kind:#x}"), "ground effect");
                self.ground_units.insert(id, cell);
                if let (Some(path), Some(pos)) = (fx::trap_model(kind), self.cell_pos(cell)) {
                    let draws = self.trap_draws(path, pos);
                    self.traps.insert(id, draws);
                } else if let (0x8d, Some(pos)) = (kind, self.cell_pos(cell)) {
                    // A cell of Ice Wall (EF_ICEWALL): its shards stand until the cell goes.
                    let shape = ro_render::Shape::Horns { kind: ro_render::Horns::IceWall, seed: id };
                    self.proc_fx("ice.tga", shape, pos, [1.0; 3], f32::MAX);
                } else if let (0x87, Some(pos)) = (kind, self.cell_pos(cell)) {
                    // A Fire Pillar waiting to go off (EF_FIREPILLARON): a red column for 5 s.
                    self.proc_fx("magic_red.tga", ro_render::Shape::Column { radius: 1.0, height: 3.0 }, pos, [1.0; 3], 5000.0);
                } else if let (Some(name), Some(pos)) = (fx::ground_str(kind), self.cell_pos(cell)) {
                    self.stop_fx(id);
                    self.play_fx_tagged(name, pos, id);
                } else {
                    tracing::debug!(kind = format_args!("{kind:#x}"), "no visual for ground effect");
                }
                // Quagmire: its mud on each cell under the bubbles.
                if let (0x8e, Some(pos)) = (kind, self.cell_pos(cell)) {
                    self.quagmire_mud(id, pos);
                }
            }
            GroundEffectGone(id) => {
                self.stop_fx(id);
                if let Some(r) = self.renderer.as_mut() {
                    r.procs.retain(|p| !matches!(p.shape, ro_render::Shape::Horns { kind: ro_render::Horns::IceWall, seed } | ro_render::Shape::Mud { seed, .. } if seed == id));
                }
                self.traps.remove(&id);
                self.ground_units.remove(&id);
                self.graffiti.remove(&id);
            }
            Graffiti { id, cell, text } => {
                tracing::debug!(id, ?cell, %text, "graffiti");
                self.ground_units.insert(id, cell);
                self.graffiti.insert(id, text);
            }
            TalkieBox { id, text } => {
                self.shouts.insert(id, (text, 0.0, false));
            }
            RentalTime { item, secs } => {
                let name = self.item_db.as_ref().map_or_else(|| format!("Item #{item}"), |d| d.name(item, true));
                let name = name.as_str();
                let text = match secs {
                    120.. => msgstr::get(1256, &[&name, &(secs / 60).to_string()]),
                    60.. => msgstr::get(1257, &[&name]),
                    _ => msgstr::get(1270, &[&name, &secs.to_string()]),
                };
                self.hud.push_chat(ro_net::ChatKind::System, text.map_or_else(|| format!("The [{name}] item will disappear in {} minutes.", secs / 60), |t| t.trim().to_string()));
            }
            StarSkill { name, id, star, result } => {
                let name = name.trim_end_matches(".gat").to_string();
                let body = ["Sun", "Moon", "Stars"].get(star as usize).copied().unwrap_or("Stars");
                let text = match result {
                    0 => format!("{name} has been designated as your Place of the {body}."),
                    1 => format!("Your Place of the {body}: {name}"),
                    10 => format!("{name} has been designated as your Target of the {body}."),
                    11 => format!("Your Target of the {body}: {name}"),
                    20 => msgstr::get(927, &[&name, &star.to_string()]).unwrap_or_else(|| format!("[Taekwon Mission] Target Monster: {name} ({star}%)")),
                    21 => msgstr::get(1190, &[&name]).unwrap_or_else(|| format!("[Taming Mission] Target Monster: {name}")),
                    22 => format!("[Collector Rank] Target Item: {}", self.item_db.as_ref().map_or_else(|| format!("Item #{id}"), |d| d.name(id, true))),
                    30 => "[Sun, Moon and Stars Angel] Designated places and monsters have been reset.".into(),
                    40 => format!("Target HP: {id}"),
                    _ => String::new(),
                };
                if !text.is_empty() {
                    self.hud.push_chat(ro_net::ChatKind::System, text);
                }
            }
            StarPlaceAsk(which) => self.windows.star_ask = Some(which),
            AdoptAsk { father, mother, name } => self.windows.adopt_ask = Some((father, mother, name)),
            MacroScan(ids) => self.reporter.found(ids),
            NpcName { id, name, title } => {
                if let Some(m) = self.units.get_mut(&id) {
                    if !name.is_empty() {
                        m.name = name;
                    }
                    // A title shows on the line under the name.
                    if !title.is_empty() {
                        m.titles = Some((String::new(), title, String::new()));
                    }
                }
            }
            MacroApplied(status) => self.reporter.applied(status),
            GuildStorageLog(result, entries) => match result {
                1 => self.guild.storage_log = Some(entries),
                2 => self.hud.push_chat(ro_net::ChatKind::System, "The guild storage log is empty.".into()),
                _ => self.hud.push_chat(ro_net::ChatKind::System, "The guild storage log could not be read.".into()),
            },
            AdoptFailed(why) => {
                let text = match why {
                    0 => "That couple already has a child.",
                    1 => "Both parents must be at least base level 70 to adopt.",
                    _ => "A married character cannot be adopted.",
                };
                self.hud.push_chat(ro_net::ChatKind::System, text.into());
            }
            DynamicNpc(result) => {
                let text = match result {
                    0 => return,
                    3 => "You already have this NPC.",
                    4 => "This NPC is no longer available.",
                    _ => "The NPC could not be created.",
                };
                self.hud.push_chat(ro_net::ChatKind::System, text.into());
            }
            // The area a monster's big spell will cover, on the ground for the cast (at least a
            // moment, so an instant cast still flashes it).
            SkillScale { skill, level, x, y, cast_ms, .. } => {
                if let (Some(size), Some(pos), Some(r)) = (self.skill_scales.size(skill, level), self.cell_pos((x, y)), self.renderer.as_mut()) {
                    let tex = *self.area_tex.get_or_insert_with(|| r.add_effect_texture(&skillscale::area_texture()));
                    let shape = ro_render::Shape::Area { size };
                    r.procs.push(ro_render::ProcFx { tex, shape, pos, color: [1.0, 0.18, 0.12], time_ms: 0.0, duration_ms: cast_ms.max(800) as f32 });
                }
            }
            PetEvolved(result) => {
                // msgstringtable 2571..2576 explain the failures.
                let text = match result {
                    6 => "Your pet has evolved.".to_string(),
                    r => msgstr::get(2571 + r.min(5) as usize, &[]).unwrap_or_else(|| "Pet evolution failed.".into()),
                };
                self.hud.push_chat(ro_net::ChatKind::System, text);
            }
            Camera { show: true, .. } => {
                if let Some(r) = self.renderer.as_ref() {
                    let c = &r.camera;
                    let text = format!("Camera: range {:.1}, rotation {:.1}, latitude {:.1}", c.distance, c.yaw.to_degrees(), -c.pitch.to_degrees());
                    self.hud.push_chat(ro_net::ChatKind::System, text);
                }
            }
            Camera { range, rotation, latitude, .. } => {
                if let Some(r) = self.renderer.as_mut() {
                    if range > 0.0 {
                        r.camera.distance = range.clamp(ro_render::Camera::MIN_DISTANCE, ro_render::Camera::MAX_DISTANCE);
                    }
                    r.camera.yaw = rotation.to_radians();
                    r.camera.pitch = (-latitude).to_radians().clamp(25f32.to_radians(), 75f32.to_radians());
                }
            }
            RentalExpired { index, item } => {
                let name = self.item_db.as_ref().map_or_else(|| format!("Item #{item}"), |d| d.name(item, true));
                self.inventory.remove(index, u16::MAX);
                let text = msgstr::get(1258, &[&name.as_str()]).map_or_else(|| format!("[{name}] item has been deleted from the Inventory."), |t| t.trim().to_string());
                self.hud.push_chat(ro_net::ChatKind::System, text);
            }
            // EFST_SKE on ourselves: the server's night (on) or day (off).
            StatusIcon { id, kind: 160, on, .. } if id == self.account_id => {
                tracing::info!(night = on, "day/night");
                self.server_night = on;
                let copy_night = self.world.as_ref().is_some_and(|w| map_night(&w.name));
                if let Some(r) = self.renderer.as_mut() {
                    r.night = on || copy_night;
                }
            }
            StatusIcon { id, kind, on, remain_ms } if id == self.account_id => self.buffs.set(kind, on, remain_ms),
            StatusIcon { .. } => {}
            Resurrected(id) if id == self.account_id => self.revive(),
            Resurrected(id) => {
                if let Some(m) = self.units.get_mut(&id) {
                    m.pose = None;
                    m.dying = None;
                }
            }
            Sound { name, act } => {
                // 0 plays once, 1 loops (played once here), 2 stops.
                if act != 2 {
                    if let (Some(a), Some(v)) = (self.audio.as_ref(), self.vfs.as_deref()) {
                        a.play(v, &name);
                    }
                }
            }
            Bgm(name) => {
                if let (Some(a), Some(v)) = (self.audio.as_mut(), self.vfs.as_deref()) {
                    a.play_bgm(v, &name);
                }
            }
            Viewpoint { npc, kind, x, y, id, color } => {
                if kind == 2 {
                    self.viewpoints.remove(&(npc, id));
                } else {
                    let rgb = egui::Color32::from_rgb((color >> 16) as u8, (color >> 8) as u8, color as u8);
                    // Kind 0 marks blink for a while; 1 stays until removed or the map changes.
                    let until = (kind == 0).then(|| std::time::Instant::now() + std::time::Duration::from_secs(15));
                    self.viewpoints.insert((npc, id), ((x as f32, y as f32), rgb, until));
                }
            }
            NpcSprite { id, class } => {
                if let Some(m) = self.units.remove(&id) {
                    let (x, y) = (m.walker.cell.0 as u16, m.walker.cell.1 as u16);
                    let info = ro_net::UnitInfo {
                        object_type: m.kind,
                        id,
                        speed: m.walker.speed as i16,
                        option: 0,
                        job: class as i16,
                        hair: 0,
                        hair_color: 0,
                        clothes_color: 0,
                        look: Default::default(),
                        sex: 0,
                        name: m.name.clone(),
                        pos: ro_net::UnitPos::At { x, y, dir: m.walker.dir as u8 },
                        guild: m.guild,
                        emblem: 0,
                        level: m.level,
                        font: m.font,
                        head_dir: m.actor.head_dir,
                    };
                    self.unit(info);
                }
            }
            MsgValue { msg, value } => {
                let v = value.to_string();
                if let Some(text) = msgstr::get(msg as usize, &[&v]) {
                    self.hud.push_chat(ro_net::ChatKind::System, text);
                }
            }
            MsgColor { msg, .. } => {
                if let Some(text) = msgstr::get(msg as usize, &[]) {
                    self.hud.push_chat(ro_net::ChatKind::System, text);
                }
            }
            MsgSkill { skill, msg } => {
                if let Some(text) = msgstr::get(msg as usize, &[]) {
                    let name = self.skill_names.internal(skill).map(|n| self.skill_names.display(n)).unwrap_or_else(|| format!("Skill #{skill}"));
                    self.hud.push_chat(ro_net::ChatKind::System, format!("[{name}] {text}"));
                }
            }
            RemoveEffect { id, effect } => {
                // removespecialeffect: stop that effect where it plays on the unit.
                if let (Some(name), Some(pos)) = (fx::script_effect(effect), self.unit_pos(id)) {
                    if let (Some(r), Some(vfs)) = (self.renderer.as_mut(), self.vfs.as_deref()) {
                        if let Some(asset) = self.fx.get(r, vfs, name) {
                            r.effects.retain(|e| e.asset != asset || e.pos.distance(pos) > ro_render::CELL);
                        }
                    }
                }
            }
            Notice(text) => self.hud.push_chat(ro_net::ChatKind::System, text),
            StyleResult(failed) => self.stylist.result(failed),
            MergeOpen(list) => self.merge.open = Some(list),
            Merged { index, amount, reason } => {
                // The other stacks come as item deletions; only this says the kept one's new total.
                if reason == 0 {
                    self.inventory.set_amount(index, amount);
                }
                let msg = match reason { 0 => "The items were merged.", 1 => "Those items can't be merged.", _ => "You have too many of that item to merge." };
                self.hud.push_chat(ro_net::ChatKind::System, msg.into());
            }
            GuildRelations(list) => self.guild.relations = list,
            GuildBans(list) => self.guild.bans = list,
            GuildBroken(r) => {
                let text = match r {
                    0 => {
                        self.guild.id = 0;
                        self.guild.master = false;
                        self.guild.name.clear();
                        self.guild.info = None;
                        self.guild.members.clear();
                        self.guild.bans.clear();
                        self.guild.relations.clear();
                        if let Some(p) = self.player.as_mut() {
                            p.guild = 0;
                            p.titles = None;
                        }
                        "The guild has been disbanded."
                    }
                    1 => "The guild name you entered does not match.",
                    _ => "You cannot disband the guild while it still has members.",
                };
                self.hud.push_chat(ro_net::ChatKind::Guild, text.into());
            }
            GmKicked(ok) => {
                let text = if ok { msgstr::get(151, &[]).unwrap_or_else(|| "Kick/Disconnected User.".into()) } else { msgstr::get(152, &[]).unwrap_or_else(|| "Kick has failed.".into()) };
                self.hud.push_chat(ro_net::ChatKind::System, text);
            }
            GmStatus(s) => {
                const NAMES: [&str; 6] = ["Str", "Agi", "Vit", "Int", "Dex", "Luk"];
                let stats: Vec<String> = s.stats.iter().zip(NAMES).map(|((v, need), n)| format!("{n} {v} ({need})")).collect();
                for line in [
                    stats.join("  "),
                    format!("Atk {} + {}  Matk {} ~ {}  Def {} + {}  Mdef {} + {}", s.atk.0, s.atk.1, s.matk.1, s.matk.0, s.def.0, s.def.1, s.mdef.0, s.mdef.1),
                    format!("Hit {}  Flee {} + {}  Critical {}  Aspd {}", s.hit, s.flee.0, s.flee.1, s.crit, s.aspd),
                ] {
                    self.hud.push_chat(ro_net::ChatKind::System, line);
                }
            }
            DisconnectAck(result) => {
                if result == 0 {
                    self.quit = self.quit_asked.take().is_some() || self.quit;
                } else {
                    self.quit_asked = None;
                    let text = msgstr::get(502, &[]).unwrap_or_else(|| "Please wait 10 seconds before trying to log out.".into());
                    self.hud.push_chat(ro_net::ChatKind::System, text);
                }
            }
            UnitGuild { id, guild, emblem } => {
                if let Some(m) = self.mob_mut(id) {
                    m.guild = guild;
                }
                self.want_emblem(guild, emblem);
            }
            AllyRequest { inviter, guild } => self.guild.ally_offer = Some((inviter, guild)),
            RelationNotice(text) => self.hud.push_chat(ro_net::ChatKind::Guild, text),
            RelationRemoved { guild, relation } => self.guild.relations.retain(|r| !(r.0 == relation && r.1 == guild)),
            StartCapture => {
                self.capturing = true;
                self.hud.push_chat(ro_net::ChatKind::System, "Click the monster you want to tame.".into());
            }
            Captured(ok) => {
                let msg = if ok { "You tamed it! Look for its egg in your inventory." } else { "The taming failed." };
                self.hud.push_chat(ro_net::ChatKind::System, msg.into());
            }
            Turned { id, head, dir } => {
                if let Some(m) = self.mob_mut(id) {
                    if !m.walker.is_walking() {
                        m.walker.dir = (dir % 8) as usize;
                        m.actor.head_dir = head;
                    }
                }
            }
            MonsterInfo { class, level, size, hp, def, race, mdef, element, mods } => {
                let name = self.units.values().find(|m| m.class == class as i16).map(|m| m.name.clone()).or_else(|| self.class_names.get(&(class as i16)).cloned()).unwrap_or_else(|| format!("Monster #{class}"));
                self.sense.info = Some(sense::MonsterInfo { name, level, size, hp, def, race, mdef, element, mods });
            }
            CoupleName(name) => {
                let line = if name.is_empty() { "Your partner can't be called right now.".to_string() } else { format!("Calling your partner, {name}.") };
                self.hud.push_chat(ro_net::ChatKind::System, line);
            }
            BladeStop { src, target, active } => {
                // Both stand locked in place while it lasts.
                if active {
                    for id in [src, target] {
                        if let Some(m) = self.mob_mut(id) {
                            let cell = m.walker.cell;
                            m.walker.stop_at(cell);
                        }
                    }
                }
            }
            ComboDelay { id, ms } => tracing::debug!(id, ms, "combo window"),
            GuildTitle { id, position } => {
                if let Some(t) = self.mob_mut(id).and_then(|m| m.titles.as_mut()) {
                    t.2 = position;
                }
            }
            CellType { x, y, kind, map } => {
                if let Some(w) = self.world.as_mut().filter(|w| map.trim_end_matches(".gat") == w.name) {
                    if let Some(g) = w.gat.as_mut() {
                        g.set_cell(x as i32, y as i32, kind as u32);
                    }
                }
            }
            PartyMemberInfo { account_id, job, level } => {
                if let Some(m) = self.social.party.iter_mut().flat_map(|p| p.members.iter_mut()).find(|m| m.account_id == account_id) {
                    m.job = job;
                    m.level = level;
                }
            }
            Spirits { id, count, kind } => {
                if count == 0 {
                    self.spirits.remove(&id);
                } else {
                    self.spirits.insert(id, (count, kind));
                }
            }
            SkillAdded(mut s) => {
                tracing::debug!(id = s.id, level = s.level, "skill added");
                match self.skills.iter_mut().find(|k| k.id == s.id) {
                    Some(k) => {
                        if s.name.is_empty() {
                            s.name = std::mem::take(&mut k.name);
                        }
                        *k = s;
                    }
                    None => self.skills.push(s),
                }
                self.load_skill_icons();
            }
            SkillRemoved(id) => self.skills.retain(|k| k.id != id),
            SwitchList(list) => {
                for it in self.inventory.items.values_mut() {
                    it.switch = 0;
                }
                for (index, pos) in list {
                    if let Some(it) = self.inventory.items.get_mut(&index) {
                        it.switch = pos;
                    }
                }
            }
            SwitchChanged { index, location, ok } => {
                if ok {
                    if let Some(it) = self.inventory.items.get_mut(&index) {
                        it.switch = location;
                    }
                } else {
                    self.hud.push_chat(ro_net::ChatKind::System, "That item can't go in the switch set.".into());
                }
            }
            SwitchDone(ok) => {
                if !ok {
                    self.hud.push_chat(ro_net::ChatKind::System, "The equipment switch failed.".into());
                }
            }
            Reputation(list) => self.reputation.points.extend(list),
            ReputationOpen => {
                self.reputation.load(self.vfs.as_deref());
                self.reputation.open = true;
            }
            Config { kind, on } => self.game_opts.config(kind, on),
            DenyPartyInvites(d) => self.game_opts.deny_invites = d,
            PartyOptions { exp_share, pickup_share, item_share } => self.game_opts.party = Some((exp_share, pickup_share, item_share)),
            AttackRange(r) => self.attack_range = r.max(1) as i32,
            MapProperty { flags, .. } => {
                self.map_flags = flags;
            }
            OverweightPercent(p) => self.hud.status.overweight_pct = p,
            Ranking { kind, list, mine } => {
                self.ranking.open = true;
                (self.ranking.kind, self.ranking.list, self.ranking.mine) = (kind, list, mine);
                for cid in self.ranking.missing() {
                    self.send(ro_net::Command::ReqName(cid));
                }
            }
            CharName { cid, name } => {
                self.ranking.names.insert(cid, name);
            }
            SearchOpen(uses) => self.store_search.opened(uses),
            SearchResults { first, more, uses, found } => self.store_search.results(first, more, uses, found),
            SearchFailed(r) => self.store_search.failed(r),
            SearchAt(x, y) => {
                self.viewpoints.insert((0, 250), ((x as f32, y as f32), egui::Color32::from_rgb(255, 200, 60), Some(std::time::Instant::now() + std::time::Duration::from_secs(30))));
                self.hud.push_chat(ro_net::ChatKind::System, format!("The shop is at ({x}, {y}), marked on the minimap."));
            }
            BossInfo { kind, x, y, hours, minutes, name } => {
                let gold = egui::Color32::from_rgb(255, 60, 60);
                match kind {
                    0 => {
                        self.viewpoints.remove(&(0, 251));
                        self.hud.push_chat(ro_net::ChatKind::System, "You cannot find any trace of a Boss Monster in this area.".into());
                    }
                    1 | 2 => {
                        self.viewpoints.insert((0, 251), ((x as f32, y as f32), gold, None));
                        if kind == 2 {
                            self.hud.push_chat(ro_net::ChatKind::System, format!("{name} is present in this area."));
                        }
                    }
                    _ => {
                        self.viewpoints.remove(&(0, 251));
                        self.hud.push_chat(ro_net::ChatKind::System, format!("{name} will appear in approximately {hours} hours {minutes} minutes."));
                    }
                }
            }
            DressRoom => {
                let (hair, color) = self.player.as_ref().and_then(|p| p.look.as_ref()).map_or((1, 0), |l| (l.hair, l.hair_color));
                self.stylist.open(hair, color, 0);
                self.stylist.dressing = true;
            }
            ReformOpen(tool) => self.reform.open(self.vfs.as_deref(), tool),
            LapineOpen { upgrade, trigger } => self.lapine.open(self.vfs.as_deref(), upgrade, trigger),
            LapineResult { upgrade, result } => {
                if let Some(a) = self.lapine.result(upgrade, result) {
                    self.lapine_action(a);
                }
                let msg = self.lapine.status.clone();
                self.hud.push_chat(ro_net::ChatKind::System, msg);
            }
            ItemChanged { index, cards, refine, grade } => {
                if let Some(it) = self.inventory.items.get_mut(&index) {
                    (it.cards, it.refine, it.grade) = (cards, refine, grade);
                }
            }
            ReformResult { result, .. } => self.reform.result(result == 0),
            UiOpenV3 { kind: 10, data } => self.enchant.open(self.vfs.as_deref(), data),
            UiOpenV3 { .. } => {}
            EnchantResult { msg, item } => {
                if msg == 3857 && item != 0 {
                    if let Some((index, slot)) = self.enchant.filled_slot(&self.inventory.items) {
                        if let Some(it) = self.inventory.items.get_mut(&index) {
                            it.cards[slot] = item;
                        }
                    }
                }
                self.enchant.result(msg == 3857, item, self.item_db.as_ref());
            }
            HatEffect { id, on, effects } => {
                for e in effects {
                    if let Some(style) = custom_hat_aura(e) {
                        if on {
                            self.hat_auras.insert(id, style);
                        } else if self.hat_auras.get(&id) == Some(&style) {
                            self.hat_auras.remove(&id);
                        }
                        continue;
                    }
                    let key = (id, e);
                    if on && !self.hat_fx.contains_key(&key) {
                        let Some(name) = self.hat_files.get(&e).copied() else { continue };
                        let tag = 0x7000_0000 + self.hat_fx.len() as u32 + 1 + (id & 0xffff) * 64;
                        if let Some(pos) = self.unit_pos(id) {
                            self.play_fx_tagged(name, pos, tag);
                            self.hat_fx.insert(key, tag);
                        }
                    } else if !on {
                        if let Some(tag) = self.hat_fx.remove(&key) {
                            self.stop_fx(tag);
                        }
                    }
                }
            }
            NameAll { id, party, guild, rank } => {
                let titles = (party, guild, rank);
                if id == self.account_id {
                    if let Some(p) = self.player.as_mut() {
                        p.titles = Some(titles);
                    }
                } else if let Some(m) = self.units.get_mut(&id) {
                    m.titles = Some(titles);
                }
            }
            AutorunSkill { id, target, level } => {
                // Self (and passive-flagged) skills go off at once; others wait for a target click.
                if target & 4 != 0 || target & (1 | 2 | 16 | 32) == 0 {
                    self.use_skill_on(id, level, self.account_id);
                } else {
                    self.aiming = Some((id, level));
                }
            }
            Identified(index) => {
                if let Some(it) = self.inventory.items.get_mut(&index) {
                    it.identified = true;
                    let name = self.item_db.as_ref().map_or_else(|| format!("Item #{}", it.id), |d| d.name(it.id, true));
                    self.hud.push_chat(ro_net::ChatKind::System, format!("Identified: {name}."));
                }
            }
            ArrowEquipped(index) => {
                for it in self.inventory.items.values_mut() {
                    it.equipped &= !0x8000;
                }
                if let Some(it) = self.inventory.items.get_mut(&index) {
                    it.equipped |= 0x8000;
                }
            }
            Mvp(id) => {
                if let Some(pos) = self.unit_pos(id) {
                    self.play_fx("mvp", pos);
                }
                let who = if id == self.account_id { self.hud.status.name.clone() } else { self.units.get(&id).map_or_else(|| "Someone".into(), |m| m.name.clone()) };
                self.hud.push_chat(ro_net::ChatKind::Broadcast, format!("{who} is the MVP!"));
            }
            MvpItem(item) => {
                let name = self.item_db.as_ref().map_or_else(|| format!("Item #{item}"), |d| d.name(item, true));
                self.hud.push_chat(ro_net::ChatKind::System, format!("MVP reward: {name}."));
            }
            MvpExp(exp) => self.hud.push_chat(ro_net::ChatKind::System, format!("MVP reward: {exp} experience.")),
            MvpDropped => self.hud.push_chat(ro_net::ChatKind::System, "You are the MVP, but the reward dropped to the floor because you are overweight.".into()),
            MemberPos { id, x, y, guild } => {
                if x == 0xffff || y == 0xffff {
                    self.member_pos.remove(&id);
                } else {
                    self.member_pos.insert(id, ((x as f32, y as f32), guild));
                }
            }
            EffectValue { id, effect, num } => {
                /// rAthena's EF_GREEN_NUMBER: the number is a heal.
                const EF_GREEN_NUMBER: u32 = 657;
                if effect == EF_GREEN_NUMBER {
                    if let Some(pos) = self.unit_pos(id) {
                        self.floaters.push((pos, num.to_string(), egui::Color32::from_rgb(110, 255, 110), 0.0));
                    }
                } else {
                    self.handle(ScriptEffect { id, effect });
                }
            }
            SpecialPopup(id) => {
                let popup = self.vfs.as_deref().and_then(|v| v.read(r"System\spopup.lub").ok().flatten()).and_then(|d| ro_formats::lub::eval(&d).ok()).and_then(|c| {
                    let t = c.globals.get("popuptbl")?.clone();
                    let e = &c.entries(&t).iter().find(|(k, _)| k.as_number() == Some(id as f64))?.1;
                    let ui = c.field(e, "uitype").and_then(|v| v.as_number()).unwrap_or(0.0) as u8;
                    let rgb = c.field(e, "textcolor").and_then(|v| v.as_number()).unwrap_or(16763904.0) as u32;
                    let lines: Vec<String> = c.field(e, "Description").map_or(vec![], |d| c.entries(d).iter().filter_map(|(_, v)| v.as_str().map(str::to_string)).collect());
                    Some((ui, egui::Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8), lines))
                });
                match popup {
                    Some((ui, color, lines)) => {
                        // uitype: 0 chat, 1 floating, 2 both.
                        if ui != 1 {
                            for l in &lines {
                                self.hud.push_chat(ro_net::ChatKind::System, l.clone());
                            }
                        }
                        if ui != 0 {
                            self.hud.banner = Some((lines, color, std::time::Instant::now()));
                        }
                    }
                    None => tracing::debug!(id, "special popup not in spopup.lub"),
                }
            }
            SkillMsg(id) => {
                let text = match id {
                    0x15 => "All abnormal status effects have been removed.",
                    0x16 => "You will be immune to abnormal status effects for the next minute.",
                    0x17 => "Your Max HP will stay increased for the next minute.",
                    0x18 => "Your Max SP will stay increased for the next minute.",
                    0x19 => "All of your stats will stay increased for the next minute.",
                    0x1c => "Your weapon will remain blessed with Holy power for the next minute.",
                    0x1d => "Your armor will remain blessed with Holy power for the next minute.",
                    0x1e => "Your Defense will stay increased for the next 10 seconds.",
                    0x1f => "Your Attack strength will stay increased for the next minute.",
                    0x20 => "Your Accuracy and Flee Rate will stay increased for the next minute.",
                    0x28 => "Full Strip failed because of coating.",
                    _ => "",
                };
                if !text.is_empty() {
                    self.hud.push_chat(ro_net::ChatKind::System, text.into());
                }
            }
            // EF_COLORPAPER: the wedding ceremony's paper shower.
            ScriptEffect { id, effect: 347 } | Wedding(id) => self.wedding_fx(id),
            // The map's weather (sent on entering it, and all again whenever it changes).
            ScriptEffect { effect, .. } if weather::from_effect(effect).is_some() => {
                if let Some(kind) = weather::from_effect(effect) {
                    self.weather.server_on(kind);
                }
            }
            ScriptEffect { id, effect } => {
                if let Some(q) = fx::ef_quake(effect) {
                    self.quake(q);
                }
                if let Some(pos) = self.unit_pos(id) {
                    match (fx::script_effect(effect), fx::ef_skill(effect)) {
                        (Some(name), _) => self.play_fx(name, pos),
                        (None, Some(skill)) => {
                            tracing::debug!(effect, skill, "script effect via skill");
                            self.skill_fx(skill, Some(pos), false);
                        }
                        _ => tracing::debug!(effect, "no visual for script effect"),
                    }
                }
            }
            UnitEffect { id, effect } => {
                // Base or job level up.
                if id == self.account_id && effect <= 1 {
                    self.feel(haptics::Cue::LevelUp);
                }
                if let (Some(name), Some(pos)) = (fx::misc_effect(effect), self.unit_pos(id)) {
                    self.play_fx(name, pos);
                }
            }
            MonsterHp { id, hp, max_hp } => {
                if let Some(m) = self.units.get_mut(&id) {
                    m.hp = Some((hp, max_hp));
                }
            }
            MapMove { map, x, y } => {
                // Respawning at the save point arrives as a map move.
                self.viewpoints.clear();
                self.member_pos.clear();
                self.revive();
                self.units.clear();
                self.devotions.clear();
                self.ground_units.clear();
                self.traps.clear();
                self.graffiti.clear();
                self.floor.clear();
                self.warps.clear();
                if self.world.as_ref().is_none_or(|w| w.name != map) {
                    self.enter_map(&map);
                }
                if let Some(p) = self.player.as_mut() {
                    p.walker.stop_at((x as i32, y as i32));
                }
            }
            // Back at character select: clear the world; the screens take over again.
            LeftGame => {
                self.leave_world();
                let user = self.settings.last_id.clone();
                let mut front = screens::Front::new(user, true);
                front.screen = screens::Screen::Loading;
                self.front = Some(front);
            }
            // Login and character-select replies (handled by the screens above).
            LoginFailed(_) | Servers(_) | Characters { .. } | CharCreated(_) | CharCreateFailed(_) | CharDeleteStarted { .. } | CharDeleteCanceled { .. } | CharDeleted { .. } | CharNameOk(_) | CharRenamed(_) | CharMoved { .. } | CharsBlocked(_) | Pin(_) => {}
            // Leaving the game: the connection closing is expected.
            Disconnected(reason) if self.quit_asked.is_some() => tracing::info!("disconnected: {reason}"),
            // Lost in game (server restart, dropped network): as the original, "Disconnected
            // from Server." and back to the login screen.
            Disconnected(reason) => {
                tracing::error!("disconnected: {reason}");
                self.leave_world();
                let user = self.settings.last_id.clone();
                let mut front = screens::Front::new(user.clone(), !user.is_empty());
                front.event(&LoginFailed(msgstr::get(2, &[]).unwrap_or_else(|| "Disconnected from Server.".into())));
                self.front = Some(front);
                self.net = Some(ro_net::spawn_session(self.session.clone()));
            }
        }
    }
}

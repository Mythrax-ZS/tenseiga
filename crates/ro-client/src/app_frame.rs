//! One frame: network events, movement, camera, the UI and the render. Part of `App` (app.rs).

use super::*;

impl App {
    /// One frame: network events, movement, camera, render (or capture).
    pub(crate) fn tick(&mut self, el: &ActiveEventLoop) {
        // Leave as soon as quitting is decided (the server's go-ahead arrives between window events).
        if self.quit {
            el.exit();
            return;
        }
        if self.paused {
            return;
        }
        let now = std::time::Instant::now();
        let dt = (now - self.last_tick).as_secs_f32() * 1000.0;
        self.last_tick = now;
        self.perf.frame(dt);
        // While a map loads the old world takes no controller input (see start_map_load).
        if self.front.is_none() && self.map_load.is_none() {
            self.pad_tick(dt);
            self.feedback_tick(dt);
            self.touchpad_tick(el);
        } else if let (Some(r), false) = (self.renderer.as_mut(), self.still) {
            // Behind the login screens the backdrop slowly turns.
            r.camera.rotate(dt * 0.00004, 0.0);
        }
        self.cursor_action = self.hover_cursor();
        self.steer_to_cursor(dt);
        self.companion_follow(now);

        // `--do a,b,c`: scripted actions once the inventory has arrived (tests).
        // Scripted helpers for two-client tests: accept requests, run a trade to completion.
        if self.auto_accept {
            if let Some((inviter, _)) = self.guild.ally_offer.take() {
                self.send(ro_net::Command::Relation(ro_net::RelationRequest::Answer { inviter, accept: true }));
            }
            if let Some(c) = self.social.accept_first() {
                tracing::info!(?c, "auto-accept");
                self.send(c);
            }
        }
        if let (Some(item), Some(t)) = (self.auto_trade, self.social.trade.as_ref()) {
            match self.trade_stage {
                0 => {
                    if item != 0 {
                        if let Some(index) = self.inventory.items.values().find(|i| i.id == item).map(|i| i.index) {
                            self.send(ro_net::Command::Trade(ro_net::TradeRequest::Add { index, amount: 1 }));
                        }
                    }
                    self.send(ro_net::Command::Trade(ro_net::TradeRequest::Ok));
                    self.trade_stage = 1;
                }
                1 if t.we_locked && t.they_locked => {
                    tracing::info!(theirs = ?t.theirs, ours = ?t.ours, "trade: both locked, committing");
                    self.send(ro_net::Command::Trade(ro_net::TradeRequest::Commit));
                    self.trade_stage = 2;
                }
                _ => {}
            }
        }
        if !self.script.is_empty() && !self.inventory.items.is_empty() && self.stats.is_some() && self.ready_at.is_some_and(|t| t.elapsed().as_millis() > 1500) && self.script_resume.is_none_or(|t| std::time::Instant::now() >= t) {
            let mut steps = std::mem::take(&mut self.script).into_iter();
            while let Some(step) = steps.next() {
                // wait:ms pauses the script; the rest runs on a later frame.
                if let Some(ms) = step.strip_prefix("wait:").and_then(|m| m.parse::<u64>().ok()) {
                    self.script = steps.collect();
                    self.script_resume = Some(std::time::Instant::now() + std::time::Duration::from_millis(ms));
                    break;
                }
                let item = |id: u32| self.inventory.items.values().find(|i| i.id == id).map(|i| (i.index, i.location));
                let action = match step.split_once(':').or(Some((step.as_str(), ""))) {
                    Some(("equip", id)) => id.parse().ok().and_then(item).map(|(index, location)| windows::UiAction::Equip { index, location }),
                    // equipl:id puts a one-handed weapon in the left hand (dual wielding).
                    Some(("equipl", id)) => id.parse().ok().and_then(item).map(|(index, _)| windows::UiAction::Equip { index, location: 0x20 }),
                    // macro: apply the macro detector to ourselves (GM); captcha:text answers it.
                    Some(("macro", _)) => {
                        self.send(ro_net::Command::Captcha(ro_net::CaptchaRequest::Apply(self.account_id)));
                        None
                    }
                    Some(("captcha", a)) => {
                        self.send(ro_net::Command::Captcha(ro_net::CaptchaRequest::Answer(a.to_string())));
                        None
                    }
                    // chat:<text> says it (GM commands mid-script); petevolve:<egg> asks to evolve.
                    Some(("chat", text)) => {
                        self.submit_chat(text.to_string());
                        None
                    }
                    Some(("macroscan", r)) => {
                        if let (Ok(radius), Some(p)) = (r.parse(), self.player.as_ref()) {
                            let (x, y) = p.walker.cell;
                            self.reporter.open = true;
                            self.send(ro_net::Command::MacroReport(ro_net::MacroReport::Scan { x: x as u16, y: y as u16, radius }));
                        }
                        None
                    }
                    Some(("petevolve", egg)) => {
                        if let Ok(egg) = egg.parse() {
                            self.send(ro_net::Command::Companion(ro_net::CompanionRequest::PetEvolve(egg)));
                        }
                        None
                    }
                    // write:<text> answers a Graffiti / Talkie Box message prompt.
                    Some(("write", text)) => self.windows.skill_text.take().map(|(skill, level, cell, _)| windows::UiAction::SkillText { skill, level, cell, text: text.to_string() }),
                    Some(("use", id)) => id.parse().ok().and_then(item).map(|(index, _)| windows::UiAction::UseItem(index)),
                    Some((f @ ("fav" | "unfav"), id)) => id.parse().ok().and_then(item).map(|(index, _)| windows::UiAction::Favorite { index, on: f == "fav" }),
                    Some(("invtab", n)) => {
                        self.windows.inventory_tab(n.parse().unwrap_or(0));
                        None
                    }
                    Some(("card", id)) => id.parse().ok().and_then(item).map(|(index, _)| windows::UiAction::CardTargets(index)),
                    // refsel:item picks equipment in the refine window; refine:material refines it.
                    // gradesel:item picks equipment in the grade window; gradeup:material tries.
                    Some(("gradesel", id)) => {
                        if let Some(index) = id.parse().ok().and_then(item).map(|(index, _)| index) {
                            self.grade.pick(index);
                            self.queued_commands.push(ro_net::Command::Grade(ro_net::GradeRequest::Select(index)));
                        }
                        None
                    }
                    Some(("gradeup", m)) => {
                        if let Some(r) = self.grade.upgrade(m.parse().unwrap_or(0)) {
                            self.queued_commands.push(ro_net::Command::Grade(r));
                        }
                        None
                    }
                    // navi:map:x:y guides there; navigo walks to the next stop.
                    Some(("navi", spec)) => {
                        let f: Vec<&str> = spec.split(':').collect();
                        self.navi.ensure_loaded(self.vfs.as_deref());
                        let cell = match (f.get(1).and_then(|x| x.parse().ok()), f.get(2).and_then(|y| y.parse().ok())) {
                            (Some(x), Some(y)) => Some((x, y)),
                            _ => None,
                        };
                        self.navi.go(f[0], cell, f[0].to_string());
                        None
                    }
                    Some(("navigo", _)) => {
                        let map = self.world.as_ref().map(|w| w.name.clone()).unwrap_or_default();
                        if let Some(p) = self.player.as_ref() {
                            self.navi.update(&map, (p.walker.cell.0 as u16, p.walker.cell.1 as u16));
                        }
                        self.navi_walk = self.navi.waypoint(&map);
                        None
                    }
                    // bg:Name applies solo to a battleground; bgauto enters it when ready.
                    Some(("walk", xy)) => {
                        if let Some((x, y)) = xy.split_once('_').and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?))) {
                            self.request_walk((x, y));
                        }
                        None
                    }
                    Some(("lapine", _)) => {
                        let inv: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
                        if let Some(a) = self.lapine.test_go(&inv) {
                            self.lapine_action(a);
                        }
                        None
                    }
                    // cam:yaw_pitch in degrees (test hook for camera angles).
                    Some(("cam", yp)) => {
                        if let (Some((y, p)), Some(r)) = (yp.split_once('_').and_then(|(y, p)| Some((y.parse::<f32>().ok()?, p.parse::<f32>().ok()?))), self.renderer.as_mut()) {
                            r.camera.yaw = y.to_radians();
                            r.camera.pitch = p.to_radians().clamp(25f32.to_radians(), 75f32.to_radians());
                        }
                        None
                    }
                    Some(("comptab", n)) => {
                        self.companions.select_tab(n.parse().unwrap_or(0));
                        None
                    }
                    Some(("petmenu", n)) => {
                        if let Ok(n) = n.parse() {
                            self.queued_commands.push(ro_net::Command::Companion(ro_net::CompanionRequest::PetMenu(n)));
                        }
                        None
                    }
                    Some(("bg", name)) => {
                        self.battle.ensure_loaded(self.vfs.as_deref());
                        self.battle.open = true;
                        self.queued_commands.push(ro_net::Command::Bg(ro_net::BgRequest::Apply { kind: 1, name: name.to_string() }));
                        None
                    }
                    Some(("bgauto", _)) => {
                        self.battle.auto_accept = true;
                        None
                    }
                    // mbuy:row:n buys from an open market or barter shop.
                    Some(("mbuy", spec)) => {
                        let f: Vec<u32> = spec.split(':').filter_map(|v| v.parse().ok()).collect();
                        if let (Some(row), Some(n)) = (f.first(), f.get(1)) {
                            self.market_buy = Some((*row as usize, *n));
                            if self.market.shop.is_some() {
                                self.market_test_buy();
                            }
                        }
                        None
                    }
                    Some(("emblem", f)) => {
                        self.guild.upload_emblem(f);
                        None
                    }
                    Some(("enchant", id)) => {
                        if let Some(r) = id.parse().ok().and_then(item).and_then(|(index, _)| self.enchant.test_random(index)) {
                            self.queued_commands.push(ro_net::Command::Enchant(r));
                        }
                        None
                    }
                    Some(("reform", _)) => {
                        let inv: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
                        if let Some(reform::ReformAction::Reform { tool, index }) = self.reform.test_reform(&inv) {
                            self.queued_commands.push(ro_net::Command::Reform(ro_net::ReformRequest::Reform { tool, index }));
                        }
                        None
                    }
                    Some(("style", h)) => {
                        let o = self.stylist.test_order(h.parse().unwrap_or(1));
                        self.queued_commands.push(ro_net::Command::Style(ro_net::StyleRequest::Buy { hair_color: o.hair_color, hair: o.hair, clothes_color: o.clothes_color }));
                        None
                    }
                    Some(("ssearch", id)) => {
                        let r = self.store_search.test_search(id.parse().unwrap_or(0));
                        self.queued_commands.push(ro_net::Command::Search(r));
                        None
                    }
                    Some(("sclick", _)) => {
                        if let Some(f) = self.store_search.results.first() {
                            self.queued_commands.push(ro_net::Command::Search(ro_net::SearchRequest::Click { owner: f.owner, store: f.store, item: f.item }));
                        }
                        None
                    }
                    Some(("option", spec)) => {
                        if let Some((k, v)) = spec.split_once(':') {
                            self.queued_commands.push(ro_net::Command::Option(ro_net::OptionRequest::Config { kind: k.parse().unwrap_or(0), on: v == "1" }));
                        }
                        None
                    }
                    Some(("swadd", id)) => id.parse().ok().and_then(item).map(|(index, location)| windows::UiAction::SwitchAdd { index, location: location & 0x3ff }),
                    Some(("swap", _)) => Some(windows::UiAction::SwitchAll),
                    Some(("tame", _)) => {
                        let me = self.player.as_ref().map(|p| p.walker.cell).unwrap_or_default();
                        let target = self.units.iter().filter(|(_, m)| m.kind == 5 && m.dying.is_none()).min_by_key(|(_, m)| (m.walker.cell.0 - me.0).pow(2) + (m.walker.cell.1 - me.1).pow(2)).map(|(id, _)| *id);
                        if let (Some(id), true) = (target, std::mem::take(&mut self.capturing)) {
                            self.send(ro_net::Command::Companion(ro_net::CompanionRequest::Capture(id)));
                        }
                        None
                    }
                    Some(("ally", who)) => {
                        if let Some(id) = self.units.iter().find(|(_, m)| m.name == who).map(|(id, _)| *id) {
                            self.send(ro_net::Command::Relation(ro_net::RelationRequest::Ally { target: id, me: self.account_id, char_id: self.char_id }));
                        }
                        None
                    }
                    Some(("mergeall", _)) => {
                        if let Some(list) = self.merge.open.take() {
                            let first = list.first().and_then(|i| self.inventory.items.get(i)).map(|i| i.id);
                            let same: Vec<u16> = list.into_iter().filter(|i| self.inventory.items.get(i).map(|x| x.id) == first).collect();
                            self.queued_commands.push(ro_net::Command::Merge(same));
                        }
                        None
                    }
                    Some(("equiptab", t)) => {
                        self.windows.equip_tab = t.parse().unwrap_or(0);
                        None
                    }
                    Some(("viewequip", aid)) => {
                        aid.parse().ok().map(|aid| self.queued_commands.push(ro_net::Command::ViewEquip(aid)));
                        None
                    }
                    Some(("showequip", on)) => Some(windows::UiAction::ShowEquip(on == "1")),
                    Some(("bank", _)) => {
                        self.toggle_bank();
                        None
                    }
                    Some(("deposit", z)) => {
                        z.parse().ok().map(|zeny| self.queued_commands.push(ro_net::Command::Bank(ro_net::BankRequest::Deposit { aid: self.account_id, zeny })));
                        None
                    }
                    Some(("refsel", id)) => {
                        if let Some((index, _)) = id.parse().ok().and_then(item) {
                            self.refine.selected = Some((index, 0, vec![]));
                            self.queued_commands.push(ro_net::Command::Refine(ro_net::RefineRequest::Select(index)));
                        }
                        None
                    }
                    Some(("refine", mat)) => {
                        if let (Some((index, _, _)), Ok(material)) = (self.refine.selected.as_ref(), mat.parse()) {
                            self.queued_commands.push(ro_net::Command::Refine(ro_net::RefineRequest::Refine { index: *index, material, blessing: false }));
                        }
                        None
                    }
                    Some(("info", id)) => id.parse().ok().map(|id| windows::UiAction::ItemInfo { id, refine: 0, cards: [0; 4] }),
                    // Merchant tests: cartwin opens the cart; vend:price fills the shop
                    // set-up with everything in the cart; browse:aid looks in a shop; buyone
                    // buys one of each item in the shop being browsed.
                    Some(("cartwin", _)) => {
                        self.merchant.cart_open = true;
                        None
                    }
                    Some(("vend", price)) => {
                        let price: u32 = price.parse().unwrap_or(100);
                        if let Some((slots, title, _)) = self.merchant.setup.take() {
                            let items = self.merchant.cart.values().take(slots as usize).map(|it| (it.index, it.amount, price)).collect();
                            self.queued_commands.push(ro_net::Command::Merchant(ro_net::MerchantRequest::OpenShop { title, items }));
                        }
                        None
                    }
                    // Buying store / cash shop tests: buystore:id:amount:price opens a store
                    // from the set-up; buybrowse:aid looks in one; sellall sells everything it
                    // wants that we carry; cashshop toggles the shop; cashbuy:id buys one.
                    Some(("buystore", spec)) => {
                        let f: Vec<u32> = spec.split(':').filter_map(|v| v.parse().ok()).collect();
                        if let (Some((_, title, _, _)), [id, amount, price]) = (self.stores.setup.take(), f.as_slice()) {
                            let limit = self.hud.status.zeny.clamp(0, u32::MAX as i64) as u32;
                            self.queued_commands.push(ro_net::Command::BuyingStore(ro_net::BuyingStoreRequest::Open { limit, title, items: vec![(*id, *amount as u16, *price)] }));
                        }
                        None
                    }
                    Some(("buybrowse", aid)) => {
                        aid.parse().ok().map(|aid| self.queued_commands.push(ro_net::Command::BuyingStore(ro_net::BuyingStoreRequest::Browse(aid))));
                        None
                    }
                    Some(("sellall", _)) => {
                        if let Some((buyer, store, _, items, _)) = self.stores.browsing.take() {
                            let list = items
                                .iter()
                                .filter_map(|w| self.inventory.items.values().find(|i| i.id == w.id && i.equipped == 0).map(|i| (i.index, w.id, w.amount.min(i.amount))))
                                .collect();
                            self.queued_commands.push(ro_net::Command::BuyingStore(ro_net::BuyingStoreRequest::Sell { buyer, store, items: list }));
                        }
                        None
                    }
                    // mail:to:title:zeny writes and sends a mail.
                    Some(("mail", spec)) => {
                        let f: Vec<&str> = spec.split(':').collect();
                        // mail:to:title:zeny:itemid also attaches one of that item.
                        if let [to, title, zeny, rest @ ..] = f.as_slice() {
                            let [begin, check] = self.mailbox.compose(to, title, "Sent by the test script.", zeny);
                            self.send(ro_net::Command::Mail(begin));
                            if let Some(item) = rest.first().and_then(|s| s.parse::<u32>().ok()) {
                                if let Some(i) = self.inventory.items.values().find(|i| i.id == item && i.equipped == 0) {
                                    let index = i.index;
                                    self.send(ro_net::Command::Mail(ro_net::MailRequest::AddItem { index, amount: 1 }));
                                }
                            }
                            self.send(ro_net::Command::Mail(check));
                        }
                        None
                    }
                    // room:title creates a public chat room; joinroom joins the first one in sight.
                    Some(("room", title)) => {
                        self.send(ro_net::Command::ChatRoom(ro_net::ChatRoomRequest::Create { title: title.into(), limit: 10, public: true, password: String::new(), change: false }));
                        None
                    }
                    Some(("joinroom", _)) => {
                        if let Some(id) = self.rooms.signs.values().next().map(|r| r.id) {
                            self.send(ro_net::Command::ChatRoom(ro_net::ChatRoomRequest::Join { id, password: String::new() }));
                        }
                        None
                    }
                    // postad posts a party booking ad at our level minus ten.
                    Some(("postad", _)) => {
                        let level = (self.hud.status.base_level.clamp(1, 999) as u16).saturating_sub(10).max(1);
                        self.send(ro_net::Command::Booking(ro_net::BookingRequest::Register { level, map: 0, jobs: vec![] }));
                        None
                    }
                    // altkey:x presses Alt+x.
                    Some(("altkey", k)) => {
                        self.alt_key(k);
                        None
                    }
                    // screenshot takes one, as Print Screen does.
                    Some(("screenshot", _)) => {
                        self.screenshot = true;
                        None
                    }
                    // press:k presses a key over the game: a letter, F1..F12, Space or Enter.
                    Some(("press", k)) => {
                        use winit::keyboard::{Key, NamedKey};
                        // Ctrl+ / Alt+ in front hold that modifier for the press.
                        let held = (self.ctrl, self.alt);
                        let mut k = k;
                        while let Some((m, rest)) = k.split_once('+').filter(|(m, r)| !r.is_empty() && matches!(*m, "Ctrl" | "Alt")) {
                            if m == "Ctrl" { self.ctrl = true } else { self.alt = true }
                            k = rest;
                        }
                        let named = match k {
                            "Space" => Some(NamedKey::Space),
                            "Enter" => Some(NamedKey::Enter),
                            "Escape" => Some(NamedKey::Escape),
                            "Insert" => Some(NamedKey::Insert),
                            "F11" => Some(NamedKey::F11),
                            "F12" => Some(NamedKey::F12),
                            f => f.strip_prefix('F').and_then(|n| n.parse::<usize>().ok()).and_then(|n| [NamedKey::F1, NamedKey::F2, NamedKey::F3, NamedKey::F4, NamedKey::F5, NamedKey::F6, NamedKey::F7, NamedKey::F8, NamedKey::F9].get(n.wrapping_sub(1)).copied()),
                        };
                        match named {
                            Some(NamedKey::Enter) => self.hud.focus_chat(),
                            Some(n) => self.key_pressed(Key::Named(n)),
                            None => self.key_pressed(Key::Character(k.into())),
                        }
                        (self.ctrl, self.alt) = held;
                        None
                    }
                    // pad:style[:row] draws the controller UI in that style (xbox, playstation,
                    // nintendo), as if that row's trigger were held; keys:n opens Shortcut
                    // Information on tab n.
                    Some(("pad", p)) => {
                        let mut it = p.split(':');
                        let style = match it.next() {
                            Some("playstation") => padglyph::Style::PlayStation,
                            Some("nintendo") => padglyph::Style::Nintendo,
                            _ => padglyph::Style::Xbox,
                        };
                        self.pad_preview = Some((style, it.next().and_then(|r| r.parse().ok())));
                        None
                    }
                    // stick:x:y:ms holds the left stick there (-1..1, y up) for ms.
                    Some(("stick", a)) => {
                        let v: Vec<f32> = a.split(':').filter_map(|v| v.parse().ok()).collect();
                        if let [x, y, ms] = v[..] {
                            self.test_stick = Some((glam::Vec2::new(x, y), ms));
                        }
                        None
                    }
                    // holdwalk:dx:dy:ms holds a walk target dx, dy cells from the player.
                    Some(("holdwalk", a)) => {
                        let v: Vec<f32> = a.split(':').filter_map(|v| v.parse().ok()).collect();
                        if let [dx, dy, ms] = v[..] {
                            self.hold_walk = Some(((dx as i32, dy as i32), ms));
                            self.mouse_walk_ms = 0.0;
                        }
                        None
                    }
                    // vibetest: Settings > Controls > Vibration > Test.
                    Some(("vibetest", _)) => {
                        self.settings.vibration_test = true;
                        None
                    }
                    Some(("keys", t)) => {
                        self.shortcut_win.open = true;
                        self.shortcut_win.tab = t.parse().unwrap_or(0);
                        None
                    }
                    // hover:x:y holds the UI pointer there (UI points), for tooltips.
                    Some(("hover", at)) => {
                        let f: Vec<f32> = at.split(':').filter_map(|v| v.parse().ok()).collect();
                        self.fake_pointer = (f.len() == 2).then(|| egui::pos2(f[0], f[1]));
                        None
                    }
                    // click:x:y clicks the UI there.
                    // drag:x1:y1:x2:y2 presses at one point, moves over a few frames, releases at the other.
                    // idle:n holds the next test drag for n frames.
                    Some(("idle", n)) => {
                        for _ in 0..n.parse().unwrap_or(0) {
                            self.fake_frames.push_back(vec![]);
                        }
                        None
                    }
                    Some(("drag", at)) => {
                        let f: Vec<f32> = at.split(':').filter_map(|v| v.parse().ok()).collect();
                        if f.len() == 4 {
                            let (a, b) = (egui::pos2(f[0], f[1]), egui::pos2(f[2], f[3]));
                            let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
                            self.fake_frames.push_back(vec![egui::Event::PointerMoved(a)]);
                            self.fake_frames.push_back(vec![button(a, true)]);
                            for t in [0.1, 0.3, 0.6, 0.9, 1.0, 1.0] {
                                self.fake_frames.push_back(vec![egui::Event::PointerMoved(a + (b - a) * t)]);
                            }
                            self.fake_frames.push_back(vec![button(b, false)]);
                        }
                        None
                    }
                    Some(("click", at)) => {
                        let f: Vec<f32> = at.split(':').filter_map(|v| v.parse().ok()).collect();
                        self.fake_click = (f.len() == 2).then(|| (egui::pos2(f[0], f[1]), true));
                        None
                    }
                    // wclick:x:y[:shift] left-clicks the world there (window pixels); cursor:x:y
                    // only moves the mouse there.
                    Some((step @ ("wclick" | "cursor"), at)) => {
                        let f: Vec<&str> = at.split(':').collect();
                        if let [Ok(x), Ok(y)] = [f[0].parse::<f64>(), f.get(1).unwrap_or(&"").parse::<f64>()] {
                            self.cursor = Some((x, y));
                            if step == "wclick" {
                                let held = std::mem::replace(&mut self.shift, f.get(2) == Some(&"shift"));
                                self.left_click();
                                self.shift = held;
                            }
                        }
                        None
                    }
                    // rclick:x:y right-clicks the world there (window pixels).
                    Some(("rclick", at)) => {
                        let f: Vec<f64> = at.split(':').filter_map(|v| v.parse().ok()).collect();
                        if f.len() == 2 {
                            self.cursor = Some((f[0], f[1]));
                            self.right_button(true);
                            self.right_button(false);
                        }
                        None
                    }
                    Some(("trait", i)) => i.parse().ok().filter(|i: &usize| *i < 6).map(windows::UiAction::RaiseTrait),
                    Some(("identify", id)) => id.parse().ok().and_then(|id: u32| self.inventory.items.values().find(|i| i.id == id && !i.identified)).map(|i| windows::UiAction::Identify(i.index)),
                    Some(("revive", _)) => {
                        self.send(ro_net::Command::AutoRevive);
                        None
                    }
                    Some(("exit", _)) => {
                        self.request_exit();
                        None
                    }
                    Some(("cashshop", _)) => {
                        self.toggle_cash_shop();
                        None
                    }
                    // talk:Name talks to that NPC once it is in view; pointbuy:item:amount buys
                    // from the cashshop / itemshop / pointshop NPC that is open.
                    Some(("talk", name)) => {
                        self.talk_to = Some(name.to_string());
                        None
                    }
                    Some(("shopclose", _)) => {
                        self.social.shop = None;
                        self.send(ro_net::Command::ShopClosed);
                        None
                    }
                    Some(("pointbuy", spec)) => {
                        if let Some((id, n)) = spec.split_once(':').and_then(|(i, n)| Some((i.parse().ok()?, n.parse().ok()?))) {
                            self.send(ro_net::Command::Shop(ro_net::ShopRequest::CashBuy { kafra: 0, items: vec![(id, n)] }));
                        }
                        None
                    }
                    Some(("cashtab", n)) => {
                        self.stores.cash_tab = n.parse().unwrap_or(0);
                        None
                    }
                    // salefind:name / saleadd:item:amount:hours / saledel:item drive the GM sale tool.
                    Some(("salefind", name)) => {
                        self.send(ro_net::Command::Sale(self.account_id, ro_net::SaleRequest::Search(name.to_string())));
                        None
                    }
                    // agency: open the Adventurer's Agency; agencyadd:purpose:min:max:memo registers
                    // our party; agencyjoin:name asks that ad's leader to let us in.
                    Some(("agency", _)) => {
                        if !self.agency.open {
                            self.agency.toggle();
                        }
                        None
                    }
                    Some(("agencyadd", spec)) => {
                        let mut f = spec.splitn(4, ':');
                        let mut num = || f.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                        let (purpose, min_lv, max_lv) = (num(), num(), num());
                        let memo = f.next().unwrap_or("").to_string();
                        let name = self.hud.status.name.clone();
                        self.agency.register(agency::Ad { gid: self.char_id, char_name: name, purpose, min_lv, max_lv, dealer: 1, memo, ..Default::default() });
                        None
                    }
                    Some(("partyleave", _)) => {
                        self.send(ro_net::Command::Party(ro_net::PartyRequest::Leave));
                        None
                    }
                    Some(("agencyjoin", name)) => {
                        if let Some(ad) = self.agency.find(name) {
                            self.send(ro_net::Command::Party(ro_net::PartyRequest::AgencyJoin { char_id: ad.gid, account_id: ad.aid }));
                        }
                        None
                    }
                    Some(("saleadd", spec)) => {
                        let v: Vec<u32> = spec.split(':').filter_map(|s| s.parse().ok()).collect();
                        if let [item, amount, hours] = v[..] {
                            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as u32;
                            self.send(ro_net::Command::Sale(self.account_id, ro_net::SaleRequest::Add { item, amount, start: now, hours: hours as u16 }));
                        }
                        None
                    }
                    Some(("saledel", item)) => {
                        if let Ok(item) = item.parse() {
                            self.send(ro_net::Command::Sale(self.account_id, ro_net::SaleRequest::Remove(item)));
                        }
                        None
                    }
                    Some(("cashbuy", id)) => {
                        let id: u32 = id.parse().unwrap_or(0);
                        if let Some(tab) = self.stores.cash_tabs.iter().find(|(_, items)| items.iter().any(|(i, _)| *i == id)).map(|(t, _)| *t) {
                            self.queued_commands.push(ro_net::Command::CashShop(ro_net::CashShopRequest::Buy { kafra: 0, items: vec![(id, 1, tab)] }));
                        }
                        None
                    }
                    Some(("expand", "")) => Some(windows::UiAction::ExpandInventory),
                    Some(("expand", "yes")) => Some(windows::UiAction::ExpandConfirm(true)),
                    Some(("offall", mask)) => u32::from_str_radix(mask, 16).ok().map(windows::UiAction::UnequipAll),
                    Some(("browse", aid)) => {
                        aid.parse().ok().map(|aid| self.queued_commands.push(ro_net::Command::Merchant(ro_net::MerchantRequest::Browse(aid))));
                        None
                    }
                    Some(("buyone", _)) => {
                        if let Some((seller, shop, items, _)) = self.merchant.browsing.take() {
                            let list = items.iter().map(|i| (1, i.index)).collect();
                            self.queued_commands.push(ro_net::Command::Merchant(ro_net::MerchantRequest::Buy { seller, shop, items: list }));
                        }
                        None
                    }
                    // pick:id answers the open menu with that item/skill id (or inventory index).
                    Some(("pick", id)) => {
                        let id: u32 = id.parse().unwrap_or(0);
                        self.windows.menu.take().and_then(|menu| {
                            use ro_net::{Menu, MenuReply};
                            let reply = match menu {
                                Menu::Items(ids) => ids.contains(&id).then_some(MenuReply::Item(id)),
                                Menu::Make { kind, items } => items.contains(&id).then_some(MenuReply::Make { kind, item: id }),
                                Menu::Produce(list) => list.iter().find(|(i, _)| *i == id).map(|(i, m)| MenuReply::Produce { item: *i, materials: *m }),
                                Menu::AutoSpell(s) => s.contains(&id).then_some(MenuReply::AutoSpell(id)),
                                Menu::Skill { flag, skills } => skills.contains(&(id as u16)).then_some(MenuReply::Skill { flag, skill: id as u16 }),
                                Menu::Identify(list) => {
                                    let by_item = self.inventory.items.values().find(|i| i.id == id && !i.identified && list.contains(&i.index)).map(|i| i.index);
                                    Some(MenuReply::Identify(by_item.unwrap_or(id as u16)))
                                }
                                Menu::Repair(list) => list.iter().find(|r| r.1 == id).map(|&(index, item, refine)| MenuReply::Repair { index, item, refine }),
                                Menu::WeaponRefine(list) => list.iter().find(|r| r.1 == id).map(|r| MenuReply::WeaponRefine(r.0)),
                                Menu::Cart { aid, looks } => looks.contains(&(id as u8)).then_some(MenuReply::Cart { aid, look: id as u8 }),
                                // The whole stack of that item.
                                Menu::Materials(kind) => self.inventory.items.values().find(|i| i.id == id).map(|i| MenuReply::Materials { kind, items: vec![(i.index, i.amount)] }),
                                Menu::Egg(_) => Some(MenuReply::Egg(self.inventory.items.values().find(|i| i.id == id).map_or(id as u16, |i| i.index))),
                                Menu::Warp { skill, maps } => maps.get(id as usize).map(|m| MenuReply::Warp { skill, map: m.clone() }),
                            };
                            reply.map(windows::UiAction::Menu)
                        })
                    }
                    Some(("raise", n)) => n.parse().ok().map(windows::UiAction::RaiseStat),
                    Some(("skillup", id)) => id.parse().ok().map(windows::UiAction::SkillUp),
                    // hotkey:slot=skill_id (at its learned level), key:slot presses it.
                    Some(("hotkey", spec)) => spec.split_once('=').and_then(|(slot, id)| {
                        let (index, id) = (slot.parse().ok()?, id.parse::<u16>().ok()?);
                        let level = self.skills.iter().find(|s| s.id == id)?.level as i16;
                        Some(windows::UiAction::SetHotkey { index, key: ro_net::Hotkey { skill: true, id: id as u32, count: level } })
                    }),
                    Some(("key", slot)) => slot.parse().ok().map(windows::UiAction::UseHotkey),
                    // selfcast:skill_id on ourselves.
                    Some(("selfcast", id)) => {
                        if let Ok(skill) = id.parse::<u16>() {
                            let level = self.skills.iter().find(|s| s.id == skill).map_or(1, |s| s.level.max(1));
                            self.use_skill_on(skill, level, self.account_id);
                        }
                        None
                    }
                    // ground:skill_id[:dx] on the cell dx (default 2) east of us.
                    Some(("ground", spec)) => {
                        let (id, dx) = spec.split_once(':').map_or((spec, 2), |(id, dx)| (id, dx.parse().unwrap_or(2)));
                        if let (Ok(skill), Some(p)) = (id.parse::<u16>(), self.player.as_ref()) {
                            let cell = ((p.walker.cell.0 + dx) as u16, p.walker.cell.1 as u16);
                            let level = self.skills.iter().find(|s| s.id == skill).map_or(1, |s| s.level.max(1));
                            self.use_skill_at(skill, level, cell);
                        }
                        None
                    }
                    // cast:skill_id on the nearest monster.
                    // attack: like --attack, start attacking the nearest monster.
                    Some(("attack", _)) => {
                        self.auto_attack = true;
                        None
                    }
                    Some(("cast", id)) => {
                        let me = self.player.as_ref().map_or((0, 0), |p| p.walker.cell);
                        let target = self
                            .units
                            .iter()
                            .filter(|(_, m)| m.kind == 5)
                            .min_by_key(|(_, m)| (m.walker.cell.0 - me.0).pow(2) + (m.walker.cell.1 - me.1).pow(2))
                            .map(|(id, _)| *id);
                        tracing::info!(?target, monsters = self.units.values().filter(|m| m.kind == 5).count(), "script cast {id}");
                        if let (Ok(skill), Some(target)) = (id.parse::<u16>(), target) {
                            let level = self.skills.iter().find(|s| s.id == skill).map_or(1, |s| s.level.max(1));
                            self.use_skill_on(skill, level, target);
                        }
                        None
                    }
                    Some(("party", name)) => {
                        self.send(ro_net::Command::Party(ro_net::PartyRequest::Create { name: name.into() }));
                        None
                    }
                    Some(("invite", name)) => {
                        self.send(ro_net::Command::Party(ro_net::PartyRequest::Invite { name: name.into() }));
                        None
                    }
                    Some(("guild", name)) => {
                        self.send(ro_net::Command::Guild(ro_net::GuildRequest::Create { char_id: self.char_id, name: name.into() }));
                        None
                    }
                    Some(("friend", name)) => {
                        self.send(ro_net::Command::Friend(ro_net::FriendRequest::Add { name: name.into() }));
                        None
                    }
                    Some(("trade", name)) => {
                        let id = self.units.iter().find(|(_, m)| m.name == name).map(|(id, _)| *id);
                        if let Some(target) = id {
                            self.trade_partner = Some(name.into());
                            self.send(ro_net::Command::Trade(ro_net::TradeRequest::Ask { target }));
                        }
                        None
                    }
                    Some(("drop", id)) => id.parse().ok().and_then(item).map(|(index, _)| windows::UiAction::Drop { index, amount: 1 }),
                    _ if step == "charselect" => {
                        self.send(ro_net::Command::Restart(1));
                        None
                    }
                    _ if step == "restart" => {
                        self.send(ro_net::Command::Restart(0));
                        None
                    }
                    _ if step == "pickup" => {
                        self.auto_pickup = true;
                        None
                    }
                    _ => None,
                };
                tracing::info!(%step, ?action, "script");
                if let Some(a) = action {
                    self.ui_action(a);
                }
            }
        }
        if self.auto_pickup {
            let me = self.player.as_ref().map(|p| p.walker.cell);
            let nearest = me.and_then(|a| self.floor.values().min_by_key(|(f, _)| (f.x as i32 - a.0).abs().max((f.y as i32 - a.1).abs())).map(|(f, _)| f.id));
            if let Some(id) = nearest {
                self.auto_pickup = false;
                tracing::info!(id, "picking up");
                self.pick_up(id);
            }
        }
        if std::mem::take(&mut self.open_windows) {
            let args: Vec<String> = std::env::args().collect();
            self.quests.open = args.iter().any(|a| a == "--quest-win");
            if args.iter().any(|a| a == "--skills-win") { self.windows.skills = true; self.windows.skill_list = args.iter().any(|a| a == "--skill-list"); self.windows.skill_tab = args.iter().skip_while(|a| *a != "--guild-tab").nth(1).and_then(|t| t.parse().ok()); } else if args.iter().any(|a| a == "--equip-win") { self.windows.equipment = true; } else if args.iter().any(|a| a == "--status-win") { self.windows.stats = true; } else { self.windows.guild = true; }
            self.guild.start_tab = std::env::args().skip_while(|a| a != "--guild-tab").nth(1).and_then(|t| t.parse().ok()).unwrap_or(0);
        }
        self.update_emblems();
        if self.quit_asked.is_some_and(|t| t.elapsed().as_secs() >= 3) {
            tracing::warn!("no answer to the exit request; leaving");
            self.quit = true;
        }
        for text in std::mem::take(&mut self.queued_chat) {
            self.submit_chat(text);
        }
        for c in std::mem::take(&mut self.queued_commands) {
            self.send(c);
        }
        if let Some(a) = self.queued_action.take() {
            self.ui_action(a);
        }
        // Server events in order, stopping at one that needs a map loaded: it and the ones after
        // it wait for the map loader (the connection keeps going on the net thread meanwhile).
        self.poll_map_load();
        let mut budget = 10_000; // what has arrived, not a stream that never ends
        while self.map_load.is_none() && budget > 0 {
            budget -= 1;
            let Some(e) = self.net.as_ref().and_then(|(_, rx)| rx.try_recv().ok()) else { break };
            match self.map_to_load(&e) {
                Some(map) => self.start_map_load(&map, e),
                None => self.handle(e),
            }
        }

        // Chasing an out-of-reach target: attack again once close enough (or arrived).
        if self.chasing {
            if let (Some((target, range)), Some(p)) = (self.attack, self.player.as_ref()) {
                let near = self.units.get(&target).is_some_and(|m| {
                    let (a, b) = (p.walker.cell, m.walker.cell);
                    (a.0 - b.0).abs().max((a.1 - b.1).abs()) <= range as i32
                });
                if near || (self.walk_goal.is_none() && !p.walker.is_walking()) {
                    self.chasing = false;
                    self.attack_unit(target);
                }
            } else {
                self.chasing = false;
            }
        }
        self.units.retain(|_, m| m.dying.is_none_or(|t| t > 0.0));
        self.casts.retain(|_, (t, total)| {
            *t += dt;
            *t < *total
        });
        self.floaters.retain_mut(|f| {
            f.3 += dt;
            f.3 < 1200.0
        });
        // Script progress bars: ours tells the server when it fills, so the script resumes.
        // Wall-clock timed, and reported a little late: the server takes a report that
        // arrives before its own timer runs out as a cancel and ends the script.
        if let Some((start, total)) = self.script_progress {
            if start.elapsed().as_secs_f32() * 1000.0 >= total + 150.0 {
                self.script_progress = None;
                if let Some((tx, _)) = self.net.as_ref() {
                    tracing::debug!("script progress bar filled");
                    let _ = tx.send(ro_net::Command::ProgressDone);
                }
            }
        }
        self.unit_progress.retain(|_, (t, total)| {
            *t += dt;
            *t < *total
        });
        // showscript call-outs stay up a few seconds; speech a little longer.
        self.shouts.retain(|_, (_, t, speech)| {
            *t += dt;
            *t < if *speech { 5000.0 } else { 4000.0 }
        });
        if let Some((kind, _, t)) = self.digit.as_mut() {
            *t += dt;
            // Type 0 shows for 5 s; the counters run until replaced or the map changes.
            if *kind == 0 && *t > 5000.0 {
                self.digit = None;
            }
        }

        if self.auto_attack {
            let me = self.player.as_ref().map(|p| p.walker.cell);
            let nearest = me.and_then(|a| {
                self.units
                    .iter()
                    .filter(|(_, m)| m.kind == 5 && m.dying.is_none())
                    .min_by_key(|(_, m)| (m.walker.cell.0 - a.0).abs().max((m.walker.cell.1 - a.1).abs()))
                    .map(|(id, _)| *id)
            });
            if let Some(id) = nearest {
                self.auto_attack = false;
                self.attack_unit(id);
            }
        }
        if let Some(name) = self.talk_to.clone() {
            if let Some(id) = self.units.iter().find(|(_, m)| m.name.eq_ignore_ascii_case(&name)).map(|(id, _)| *id) {
                self.talk_to = None;
                self.talk(id);
            }
        }

        // Pick up the item we were walking to once we have arrived.
        if self.pending_pickup.is_some() && self.walk_goal.is_none() && self.player.as_ref().is_some_and(|p| !p.walker.is_walking()) {
            if let Some(id) = self.pending_pickup.take() {
                tracing::debug!(id, "pickup request");
            self.send(ro_net::Command::Item(ro_net::ItemRequest::PickUp { floor_id: id }));
            }
        }

        // Talk to the NPC we were walking to once we have arrived.
        if self.pending_npc.is_some() && self.walk_goal.is_none() && self.player.as_ref().is_some_and(|p| !p.walker.is_walking()) {
            if let Some(npc) = self.pending_npc.take() {
                self.talk(npc);
            }
        }
        // `--fx hit`: a hit spark on the player every 400 ms.
        if self.fx_test == Some("hit") {
            let now = std::time::Instant::now();
            if let (Some(p), true) = (self.player.as_ref().map(|p| p.actor.pos), self.fx_test_next.is_none_or(|t| now >= t)) {
                self.fx_test_next = Some(now + std::time::Duration::from_millis(400));
                self.hit_spark(p);
            }
        }
        // `--fx skill:<id>`: replay a skill's effect on the player every 1.5 seconds (a
        // timer rather than waiting for quiet, which maps with lasting effects never are).
        if let Some(id) = self.fx_test.and_then(|n| n.strip_prefix("skill:")).and_then(|n| n.parse::<u16>().ok()) {
            let now = std::time::Instant::now();
            if self.player.is_some() && self.fx_test_next.is_none_or(|t| now >= t) {
                self.fx_test_next = Some(now + std::time::Duration::from_millis(1500));
                let pos = self.unit_pos(self.account_id).or_else(|| self.player.as_ref().map(|p| p.actor.pos));
                if let (137, Some(p)) = (id, pos) {
                    self.spike_line(p, p + glam::Vec3::new(6.0 * ro_render::CELL, 0.0, -3.0 * ro_render::CELL));
                }
                if let (7015, Some(p)) = (id, pos) {
                    self.crescent_gale(self.account_id, p, p + glam::Vec3::new(6.0 * ro_render::CELL, 0.0, -3.0 * ro_render::CELL));
                }
                self.skill_fx(id, pos, true);
            }
        }
        // Cast the skill we walked into range for (re-walks if the target moved).
        if let (Some((skill, level, target, _)), None) = (self.pending_skill, self.walk_goal.as_ref()) {
            if self.player.as_ref().is_some_and(|p| !p.walker.is_walking()) {
                if self.units.contains_key(&target) {
                    self.use_skill_on(skill, level, target);
                } else {
                    self.pending_skill = None;
                }
            }
        }

        // Continue a long walk once the current hop has finished (or the server ignored it).
        let idle = self.player.as_ref().is_some_and(|p| !p.walker.is_walking());
        if idle && self.walk_goal.as_ref().is_some_and(|g| g.sent_at.is_none_or(|t| t.elapsed().as_millis() > 500)) {
            self.next_hop();
        }

        self.update_room();
        if let Some(a) = self.companion_action.take() {
            match a {
                // With a script running, commands go to it (as Alt+right-click does in the
                // original): ATTACK_OBJECT_CMD 3, FOLLOW_CMD 9.
                companions::CompanionAction::Send(r) => {
                    if r == ro_net::CompanionRequest::MercMenu(2) {
                        // The server says nothing on a dismissal.
                        self.companions.merc = None;
                        self.companions.merc_skills.clear();
                        self.merc_ai = None;
                    }
                    let msg = match r {
                        ro_net::CompanionRequest::Attack { id, target } => Some((id, vec![3, target as i64])),
                        ro_net::CompanionRequest::Return(id) => Some((id, vec![9])),
                        _ => None,
                    };
                    let brain = msg.as_ref().and_then(|(id, _)| if *id == self.companions.homun_id { self.homun_ai.as_ref() } else { self.merc_ai.as_ref() });
                    match (brain, msg) {
                        (Some(b), Some((_, m))) => b.world.borrow_mut().msg = m,
                        _ => self.send(ro_net::Command::Companion(r)),
                    }
                }
                companions::CompanionAction::Skill { id, level, on } => self.use_skill_on(id, level, on),
                companions::CompanionAction::SkillUp(id) => self.send(ro_net::Command::Stat(ro_net::StatRequest::SkillUp(id))),
            }
        }
        if let Some(key) = self.hud.menu_click.take().or_else(|| self.windows.menu_key.take()) {
            self.alt_key(key);
        }
        if self.windows.equipment || self.stylist.open {
            self.refresh_portrait();
        }
        if let (Some(w), Some(p)) = (self.world.as_ref(), self.player.as_ref()) {
            let (map, me) = (w.name.clone(), (p.walker.cell.0 as u16, p.walker.cell.1 as u16));
            self.navi.update(&map, me);
            self.battle.map_changed(&map);
        }
        // Hat effects follow their wearer; they end when the wearer leaves view.
        if !self.hat_fx.is_empty() {
            let placed: Vec<(u32, Option<glam::Vec3>)> = self.hat_fx.iter().map(|((unit, _), tag)| (*tag, self.unit_pos(*unit))).collect();
            let gone: std::collections::HashSet<u32> = placed.iter().filter(|(_, p)| p.is_none()).map(|(t, _)| *t).collect();
            self.hat_fx.retain(|_, tag| !gone.contains(tag));
            if let Some(r) = self.renderer.as_mut() {
                for (tag, pos) in placed {
                    match pos {
                        Some(p) => r.effects.iter_mut().filter(|e| e.tag == tag).for_each(|e| e.pos = p),
                        None => r.effects.retain(|e| e.tag != tag),
                    }
                }
            }
        }
        if let Some(cell) = self.navi_walk.take() {
            self.request_walk(cell);
        }
        // The unit and the floor item under the cursor (a monster's or an item's name shows only then).
        let hovered = self.pick_unit();
        let hovered_item = self.pick_floor_item();
        // The walkable cell under the mouse, marked on the ground as the original does.
        let hover_cell = (self.front.is_none() && !self.rotating && !self.egui_ctx.is_pointer_over_egui())
            .then(|| self.pick())
            .flatten()
            .filter(|&(x, y)| self.world.as_ref().and_then(|w| w.gat.as_ref()).is_some_and(|g| g.walkable(x as i32, y as i32)));
        let (Some(r), Some(window)) = (self.renderer.as_mut(), self.window.as_ref()) else { return };
        r.actors.clear();
        r.models.clear();
        r.models.extend(self.traps.values().flatten().cloned());
        r.warps = self
            .warps
            .values()
            .filter_map(|&(x, y)| {
                let w = self.world.as_ref()?;
                if !w.shows_cell(x as i32, y as i32) {
                    return None;
                }
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let (wx, wz) = (fx * ro_render::CELL, fy * ro_render::CELL);
                let h = w.floor_at(wx, wz);
                Some(glam::Vec3::new(wx, h, wz))
            })
            .collect();
        r.auras.clear();
        if let Some(w) = self.world.as_ref() {
            let octant = (r.camera.yaw / std::f32::consts::FRAC_PI_4).round() as i32;
            // Aura textures: the level aura's as they are, custom auras' whitened to be tinted.
            let auras = match self.vfs.as_deref() {
                Some(vfs) => {
                    let mut set = |white: bool| {
                        let mut get = |n: &'static str| if white { self.fx.texture_white(r, vfs, n) } else { self.fx.texture(r, vfs, n) };
                        Some([get("ring_blue.tga")?, get("pikapika2.bmp")?, get("whitelight.tga")?])
                    };
                    let (level, white) = (set(false), set(true));
                    let disc = self.fx.fx_texture(r, vfs, fx::FxTexture::Disc);
                    AuraTextures { level, white, disc, level_off: self.settings.aura_off, simple: self.settings.aura_simple }
                }
                None => AuraTextures::default(),
            };
            self.hat_auras.retain(|id, _| *id == self.account_id || self.units.contains_key(id));
            if let Some(p) = self.player.as_mut() {
                // Combat stance (weapon drawn) while attacking something.
                let idle = if self.attack.is_some() && self.action == 0 { 4 } else { self.action };
                p.attack_group = attack_group(r, &p.actor);
                p.fit_pose(r);
                // /doridori: the head shakes left and right, then looks ahead again.
                if let Some(t) = self.doridori.as_mut() {
                    const SHAKE: [u8; 5] = [2, 1, 2, 1, 0];
                    *t += dt;
                    let i = (*t / 120.0) as usize;
                    p.actor.head_dir = SHAKE[i.min(SHAKE.len() - 1)];
                    if i >= SHAKE.len() {
                        self.doridori = None;
                    }
                }
                let a = p.update(dt, w, octant, idle);
                tracing::trace!(x = a.pos.x / ro_render::CELL, z = a.pos.z / ro_render::CELL, dt, cell = ?p.walker.cell, next = ?p.walker.next(), "walk trace");
                r.camera.target = a.pos + quake_offset(&mut self.quakes, std::time::Instant::now(), r.camera.yaw);
                let level = self.hud.status.base_level >= AURA_LEVEL as i64;
                p.aura_ms = auras.push(r, self.hat_auras.get(&self.account_id).copied(), level, &a, p.aura_ms + dt, self.account_id);
                r.actors.push(a);
            }
            for (&id, m) in self.units.iter_mut() {
                let (action, time) = (m.actor.action, m.actor.time_ms);
                m.attack_group = attack_group(r, &m.actor);
                m.fit_pose(r);
                let a = m.update(dt, w, octant, 0);
                // Attack, hurt and death frames carry their sounds in the ACT.
                if let (Some(_), Some(audio), Some(vfs)) = (m.pose, self.audio.as_ref(), self.vfs.as_deref()) {
                    let from = if a.action == action && time > 0.0 && a.time_ms >= time { time } else { -1.0 };
                    if let Some(s) = r.actor_sound(&a, from) {
                        audio.play(vfs, &s);
                    }
                }
                let level = m.kind == 0 && m.level >= AURA_LEVEL;
                let custom = self.hat_auras.get(&id).copied().or_else(|| saga::look(m.class).and_then(|l| l.aura));
                m.aura_ms = auras.push(r, custom, level, &a, m.aura_ms + dt, id);
                if let Some(Some(model)) = self.models3d.get(&m.class).filter(|_| a.alpha > 0.1 && w.shows_cell(m.walker.cell.0, m.walker.cell.1)) {
                    r.models.extend(model.draws(&a, m.walker.dir as usize));
                }
                r.actors.push(a);
            }
            // The ground cursor: texture\grid.tga's corner marks, mint green and half opaque
            // (measured from the original), lying on the cell's corners.
            if let (Some((x, y)), Some(vfs)) = (hover_cell, self.vfs.as_deref()) {
                if let Some(tex) = self.fx.texture(r, vfs, "data\\texture\\grid.tga") {
                    let (x0, z0) = (x as f32, y as f32);
                    let h = |dx: f32, dz: f32| w.floor_at((x0 + dx) * ro_render::CELL, (z0 + dz) * ro_render::CELL);
                    let shape = ro_render::Shape::Cell { heights: [h(0.001, 0.001), h(0.999, 0.001), h(0.999, 0.999), h(0.001, 0.999)] };
                    let pos = glam::Vec3::new(x0 * ro_render::CELL, 0.0, z0 * ro_render::CELL);
                    r.auras.push(ro_render::ProcFx { tex, shape, pos, color: [0.106, 0.863, 0.686], time_ms: 0.0, duration_ms: f32::MAX });
                }
            }
            // Ambient map effects (RSW emitters, EF_* ids): torches loop in place; bubbles and
            // fireflies fire again on each emitter's own timer while the player is near.
            if let (Some(vfs), Some(p)) = (self.vfs.as_deref(), self.player.as_ref()) {
                let me = p.actor.pos;
                for e in &mut self.map_fx {
                    let near = e.pos.distance(me) < fx::MAP_FX_RANGE * ro_render::CELL;
                    match e.visual {
                        Some(fx::MapVisual::Sprite(name, lift)) => {
                            e.clock_ms += dt;
                            if let (true, Some(body)) = (w.shows_cell((e.pos.x / ro_render::CELL) as i32, (e.pos.z / ro_render::CELL) as i32), self.fx.sprite(r, vfs, name)) {
                                r.actors.push(ro_render::Actor { pos: e.pos + glam::Vec3::Y * lift, body, head: None, headgear: vec![], weapon: vec![], shadow: None, action: 0, dir: ro_render::Dir::S, head_dir: 0, time_ms: e.clock_ms, once: false, alpha: 1.0, scale: 1.0, tint: [1.0; 3] });
                            }
                        }
                        Some(visual) => {
                            e.clock_ms -= dt;
                            if e.clock_ms > 0.0 {
                                continue;
                            }
                            e.clock_ms = e.every_ms;
                            if !near {
                                continue;
                            }
                            e.fired += 1;
                            match visual {
                                fx::MapVisual::Str(names) => {
                                    if let Some(asset) = self.fx.get(r, vfs, names[(e.fired as usize * 7) % names.len()]) {
                                        r.effects.push(ro_render::Fx { asset, pos: e.pos, time_ms: 0.0, looping: false, tag: 0 });
                                    }
                                }
                                _ => {
                                    if let Some(tex) = self.fx.texture(r, vfs, "pok1.bmp") {
                                        let a = e.fired as f32 * 2.399;
                                        let drift = glam::Vec3::new(a.cos(), 0.25, a.sin()) * ro_render::CELL * 0.8;
                                        let shape = ro_render::Shape::Motes { drift, size: 1.6 };
                                        r.procs.push(ro_render::ProcFx { tex, shape, pos: e.pos, color: [0.85, 0.95, 0.8], time_ms: 0.0, duration_ms: e.every_ms.clamp(1500.0, 4000.0) });
                                    }
                                }
                            }
                        }
                        None => {}
                    }
                }
                // Underwater, the player breathes out a few bubbles now and then.
                if w.submerged {
                    self.breath_ms -= dt;
                    if self.breath_ms <= 0.0 {
                        self.breath_ms = 2600.0 + (me.x * 7.3 + me.z * 3.1).rem_euclid(1400.0);
                        if let Some(asset) = self.fx.get(r, vfs, "bubble1") {
                            r.effects.push(ro_render::Fx { asset, pos: me + glam::Vec3::new(0.0, ro_render::CELL * 2.0, 0.0), time_ms: 0.0, looping: false, tag: 0 });
                        }
                    }
                }
            }
            if let (Some(name), true, Some(vfs), Some(p)) = (self.fx_test.filter(|n| !n.starts_with("skill:")), r.effects.is_empty(), self.vfs.as_deref(), self.player.as_ref()) {
                if let Some(asset) = self.fx.get(r, vfs, name) {
                    r.effects.push(ro_render::Fx { asset, pos: p.actor.pos, time_ms: 0.0, looping: false, tag: 0 });
                }
            }
            // Skill effects run once, then go.
            for e in &mut r.effects {
                e.time_ms += dt;
            }
            // Ground effects loop until the server removes them.
            let durations: Vec<f32> = r.effects.iter().map(|e| r.fx_duration(e.asset)).collect();
            for (e, d) in r.effects.iter_mut().zip(&durations) {
                if e.looping && *d > 0.0 {
                    e.time_ms %= d;
                }
            }
            let mut alive = durations.into_iter();
            r.effects.retain(|e| alive.next().is_some_and(|d| e.looping || e.time_ms < d));
            for p in &mut r.procs {
                p.time_ms += dt;
            }
            r.procs.retain(|p| p.time_ms < p.duration_ms);
            for (a, left) in &mut self.sprite_fx {
                a.time_ms += dt;
                *left -= dt;
            }
            self.sprite_fx.retain(|(_, left)| *left > 0.0);
            r.actors.extend(self.sprite_fx.iter().map(|(a, _)| a.clone()));
            // Emotes over heads (sprite\이팩트\emotion, one action per ET_* id).
            if let (false, Some(vfs)) = (self.emotes.is_empty(), self.vfs.as_deref()) {
                if let Some(sheet) = self.fx.sprite(r, vfs, "emotion") {
                    for (id, (kind, t)) in &mut self.emotes {
                        *t += dt;
                        let m = if *id == self.account_id { self.player.as_ref() } else { self.units.get(id) };
                        let action = emotes::action(*kind);
                        let Some((m, action)) = m.zip(action).filter(|&(_, a)| *t < r.action_ms(sheet, a).max(1500.0)) else {
                            *t = f32::MAX;
                            continue;
                        };
                        r.actors.push(ro_render::Actor {
                            pos: m.actor.pos + glam::Vec3::Y * ro_render::CELL * 2.8,
                            body: sheet,
                            head: None,
                            headgear: vec![],
                            weapon: vec![],
                            shadow: None,
                            action: action / 8,
                            dir: ro_render::Dir::from_index(action),
                            head_dir: 0,
                            time_ms: *t,
                            once: true,
                            alpha: 1.0, scale: 1.0, tint: [1.0; 3],
                        });
                    }
                    self.emotes.retain(|_, (_, t)| *t < f32::MAX);
                }
            }
            // Items on the ground: their sprite at the cell plus sub-cell offset.
            for (f, actor) in self.floor.values() {
                if let Some(mut a) = actor.clone() {
                    let (fx, fy) = floor_cell(f);
                    let (x, z) = (fx * ro_render::CELL, fy * ro_render::CELL);
                    let y = w.floor_at(x, z);
                    a.pos = glam::Vec3::new(x, y, z);
                    r.actors.push(a);
                }
            }
        }

        r.post_settings = self.settings.visual.post();
        if self.settings.visual.render_auto {
            // About a million world pixels, in the slider's 5% steps (shown there).
            let (w, h) = r.size();
            let auto = ((1.0e6 / (w as f32 * h as f32).max(1.0)).sqrt().clamp(0.5, 1.0) * 20.0).floor() / 20.0;
            self.settings.visual.render_scale = auto;
        }
        r.set_render_scale(self.settings.visual.render_scale);
        // Catmull-Rom on magnified ground and model textures (the sprite shader never reads it).
        r.upscale_runtime = self.settings.upscale.runtime_bicubic();
        r.underwater = if self.world.as_ref().is_some_and(|w| w.submerged) { self.settings.visual.underwater } else { 0.0 };
        r.set_water_settings(self.settings.visual.water_settings());
        // HUD: names under actors, status, chat. Laid out for the capture size when capturing.
        let size = self.capture.as_ref().map_or_else(|| r.size(), |(_, w, h)| (*w, *h));
        let mut labels = vec![];
        for (uid, m) in self.player.iter().map(|p| (self.account_id, p)).chain(self.units.iter().map(|(i, m)| (*i, m))) {
            let shown = m.actor.alpha > 0.01 && self.world.as_ref().is_none_or(|w| w.shows_cell(m.walker.cell.0, m.walker.cell.1));
            if let (true, Some(color), Some(feet)) = (shown, m.label_color(hovered == Some(uid)), r.project(m.actor.pos, size)) {
                // As the original: "Name (Party)", then "Guild [Rank]" on a second line.
                let mut text = m.name.clone();
                if let Some((party, guild, rank)) = &m.titles {
                    if !party.is_empty() {
                        text = format!("{text} ({party})");
                    }
                    if !guild.is_empty() {
                        text = if rank.is_empty() { format!("{text}
{guild}") } else { format!("{text}
{guild} [{rank}]") };
                    }
                }
                let emblem = (m.guild != 0).then(|| self.emblems.get(&m.guild).cloned().flatten()).flatten().map(|t| t.id());
                let (orbs, orb_kind) = self.spirits.get(&uid).copied().unwrap_or((0, 0));
                let sprite_px = r.sprite_pixel(m.actor.pos, size).unwrap_or(1.0);
                labels.push(hud::Label { feet, text, color, emblem, orbs, orb_kind, sprite_px });
            }
        }
        // A floor item's name, as the original: "Jellopy: 3 ea." (msgstringtable 183).
        if let Some((f, w)) = hovered_item.and_then(|id| self.floor.get(&id)).map(|(f, _)| f).zip(self.world.as_ref()) {
            let (fx, fy) = floor_cell(f);
            let (x, z) = (fx * ro_render::CELL, fy * ro_render::CELL);
            let y = w.floor_at(x, z);
            if let Some(feet) = r.project(glam::Vec3::new(x, y, z), size) {
                let name = self.item_db.as_ref().map_or_else(|| format!("Item #{}", f.item), |d| d.name(f.item, f.identified));
                let amount = f.amount.max(1).to_string();
                let text = msgstr::get(183, &[&name, &amount]).unwrap_or_else(|| format!("{name}: {amount} ea."));
                labels.push(hud::Label { feet, text, color: egui::Color32::WHITE, emblem: None, orbs: 0, orb_kind: 0, sprite_px: 1.0 });
            }
        }
        let floaters: Vec<_> = self.floaters.iter().filter_map(|(pos, text, color, age)| Some((r.project(*pos, size)?, text.clone(), *color, *age))).collect();
        let bars: Vec<_> = self
            .units
            .values()
            .filter(|m| m.dying.is_none())
            .filter_map(|m| {
                let (hp, max) = m.hp?;
                Some((r.project(m.actor.pos, size)?, hp as f32 / max.max(1) as f32))
            })
            .collect();
        // The controller's target, marked over its head.
        let pad_marker = self.pad_target.filter(|_| self.pad.in_use()).and_then(|t| self.units.get(&t)).and_then(|m| r.project(m.actor.pos, size));
        // Cast bars over whoever is chanting.
        // Shop signs over vending players.
        let shop_signs: Vec<(u32, glam::Vec2)> = self
            .merchant
            .signs
            .keys()
            .filter_map(|id| { let m = if *id == self.account_id { self.player.as_ref() } else { self.units.get(id) }?; Some((*id, r.project(m.actor.pos + glam::Vec3::Y * ro_render::CELL * 3.2, size)?)) })
            .collect();
        let room_signs: Vec<(u32, glam::Vec2)> = self
            .rooms
            .signs
            .keys()
            .filter_map(|id| { let m = if *id == self.account_id { self.player.as_ref() } else { self.units.get(id) }?; Some((*id, r.project(m.actor.pos + glam::Vec3::Y * ro_render::CELL * 3.2, size)?)) })
            .collect();
        let buy_signs: Vec<(u32, glam::Vec2)> = self
            .stores
            .signs
            .keys()
            .filter_map(|id| { let m = if *id == self.account_id { self.player.as_ref() } else { self.units.get(id) }?; Some((*id, r.project(m.actor.pos + glam::Vec3::Y * ro_render::CELL * 3.2, size)?)) })
            .collect();
        // Quest marks over NPC heads (screen pixels).
        let quest_marks: Vec<(glam::Vec2, u16, u16)> = self
            .quests
            .marks
            .iter()
            .filter_map(|(id, &(mark, color))| Some((r.project(self.units.get(id)?.actor.pos + glam::Vec3::Y * ro_render::CELL * 3.0, size)?, mark, color)))
            .collect();
        let cast_bars: Vec<_> = self
            .casts
            .iter()
            .filter_map(|(id, &(t, total))| {
                let m = if *id == self.account_id { self.player.as_ref() } else { self.units.get(id) }?;
                Some((r.project(m.actor.pos, size)?, t / total))
            })
            // Script progress bars draw like cast bars (green, as the original).
            .chain(self.script_progress.map(|(start, total)| (self.account_id, start.elapsed().as_secs_f32() * 1000.0 / total)).into_iter().chain(self.unit_progress.iter().map(|(id, (t, total))| (*id, t / total))).filter_map(|(id, frac)| {
                let m = if id == self.account_id { self.player.as_ref() } else { self.units.get(&id) }?;
                Some((r.project(m.actor.pos, size)?, frac))
            }))
            .collect();
        let says: Vec<(glam::Vec2, String, bool)> = self
            .shouts
            .iter()
            .filter_map(|(id, (text, _, speech))| {
                let m = if *id == self.account_id { self.player.as_ref() } else { self.units.get(id) };
                // Talkie Boxes are ground units, not actors.
                let pos = m.map(|m| m.actor.pos).or_else(|| cell_pos_in(self.world.as_ref()?, *self.ground_units.get(id)?))?;
                Some((r.project(pos, size)?, text.clone(), *speech))
            })
            .collect();
        let scrawls: Vec<(glam::Vec2, String)> =
            self.graffiti.iter().filter_map(|(id, text)| Some((r.project(cell_pos_in(self.world.as_ref()?, *self.ground_units.get(id)?)?, size)?, text.clone()))).collect();
        self.hud.scrawls = scrawls;
        // Devotion links run chest to chest, from the Crusader to each protected unit.
        let chest = |id: u32| {
            let m = if id == self.account_id { self.player.as_ref() } else { self.units.get(&id) }?;
            r.project(m.actor.pos + glam::Vec3::Y * ro_render::CELL * 1.2, size)
        };
        let chest = &chest;
        let beams = self.devotions.iter().flat_map(|(&src, targets)| targets.iter().filter_map(move |&t| Some((chest(src)?, chest(t)?)))).collect();
        self.hud.beams = beams;
        self.hud.digit_text = self.digit.map(|(kind, value, t)| digit_text(kind, value, t));
        self.hud.pvp_text = self.pvp_rank.map(|(rank, total)| format!("{rank} / {total}"));
        if let Some(state) = self.egui_state.as_mut() {
            // Full screen follows the setting (F11 or the settings window). Android apps are
            // always full screen and refuse the request, which would otherwise repeat each frame.
            if !cfg!(target_os = "android") && self.capture.is_none() && window.fullscreen().is_some() != self.settings.fullscreen {
                window.set_fullscreen(self.settings.fullscreen.then_some(winit::window::Fullscreen::Borderless(None)));
            }
            let mut input = state.take_egui_input(window);
            if let Some(p) = self.fake_pointer.take() {
                input.events.push(egui::Event::PointerMoved(p));
            }
            if let Some(events) = self.fake_frames.pop_front() {
                input.events.extend(events);
            }
            if let Some((p, pressed)) = self.fake_click.take() {
                input.events.push(egui::Event::PointerMoved(p));
                input.events.push(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
                if pressed {
                    self.fake_click = Some((p, false));
                }
            }
            let ppp = window.scale_factor() as f32 * self.ui_scale;
            input.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.0 as f32, size.1 as f32) / ppp));
            // Android: the on-screen keyboard shrinks the activity's content rect, not the
            // window (NativeActivity's adjustResize), so the interface fits above it: the login
            // form, a dialog or the chat being typed in stays in view. The world still fills
            // the screen.
            #[cfg(target_os = "android")]
            {
                use winit::platform::android::WindowExtAndroid;
                let visible = window.content_rect().bottom;
                if visible > 0 && (visible as u32) < size.1 {
                    input.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.0 as f32, visible as f32) / ppp));
                }
            }
            self.egui_ctx.set_pixels_per_point(ppp);
            let mut sent = None;
            let mut answer = None;
            let mut ui_action = None;
            if self.skin.is_none() && self.settings.classic_ui {
                if let Some(vfs) = self.vfs.as_deref() {
                    self.skin = Some(classic::Skin::load(&self.egui_ctx, vfs));
                }
            }
            self.hud.classic_bar = self.skin.as_ref().filter(|_| self.settings.classic_ui).and_then(|s| s.texture("dialog_bg")).cloned();
            classic::install(&self.egui_ctx, self.skin.as_ref().filter(|_| self.settings.classic_ui));
            let guild_name = self.guild.name.clone();
            let items: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
            let skill_names = |n: &str| self.skill_names.display(n);
            // A copy: the UI below may change the settings while the view is held.
            let battle_keys = self.settings.battle_keys.clone();
            let keys = self.settings.keys.clone();
            let view = windows::View {
                portrait: self.portrait.as_ref().and_then(|p| p.1.as_ref()),
                skin: self.skin.as_ref().filter(|_| self.settings.classic_ui),
                guild_name: Some(guild_name.as_str()),
                items: &items,
                db: self.item_db.as_ref(),
                icons: &self.icons,
                unid_icons: &self.unid_icons,
                skill_icons: &self.skill_icons,
                stats: self.stats.as_ref(),
                fourth_job: jobs::is_fourth(self.hud.status.job_id),
                skills: &self.skills,
                skill_names: &skill_names,
                skill_info: Some(&self.skill_names),
                skill_tree: Some(&self.skill_tree),
                job: self.hud.status.job_id,
                skill_points: self.hud.status.skill_points,
                weight: (self.hud.status.weight, self.hud.status.max_weight),
                zeny: self.hud.status.zeny,
                screen_width: size.0 as f32 / (window.scale_factor() as f32 * self.ui_scale),
                hotkeys: &self.hotkeys,
                hotkey_row: self.hotkey_row,
                hotkey_rows: self.settings.hotkey_rows,
                battle_mode: self.settings.battle_mode,
                battle_keys: &battle_keys,
                keys: &keys,
                cooldowns: &self.cooldowns,
                has_cart: self.merchant.has_cart(),
                show_equip: self.show_equip,
                pad: self.pad_preview.map(|p| p.0).or_else(|| self.pad.in_use().then(|| self.settings.pad_style.resolve(self.pad.detected))),
                pad_row: self.pad_preview.and_then(|p| p.1).or(self.pad_row),
            };
            let my_name = self.hud.status.name.clone();
            let social_ctx = social::Ctx {
                db: self.item_db.as_ref(),
                icons: &self.icons,
                inventory: &items,
                zeny: self.hud.status.zeny,
                my_name: &my_name,
            };
            let mut social_cmds = vec![];
            let mut front_action = None;
            let mut player_cmd = None;
            let party: std::collections::HashSet<&str> = self.social.party.iter().flat_map(|p| p.members.iter().map(|m| m.name.as_str())).collect();
            let mut dots: Vec<_> = self
                .units
                .values()
                .filter(|m| m.dying.is_none() && self.world.as_ref().is_none_or(|w| w.shows_cell(m.walker.cell.0, m.walker.cell.1)))
                .filter_map(|m| {
                    let color = match m.kind {
                        0 if party.contains(m.name.as_str()) => egui::Color32::from_rgb(255, 130, 200),
                        0 => egui::Color32::from_rgb(120, 180, 255),
                        5 => egui::Color32::from_rgb(230, 60, 50),
                        k if is_npc(k) => egui::Color32::from_rgb(90, 220, 90),
                        _ => return None,
                    };
                    Some(((m.walker.cell.0 as f32, m.walker.cell.1 as f32), color))
                })
                .collect();
            dots.extend(self.member_pos.iter().filter(|(id, _)| !self.units.contains_key(id) && **id != self.account_id).map(|(_, (c, guild))| (*c, if *guild { egui::Color32::from_rgb(170, 255, 170) } else { egui::Color32::from_rgb(255, 130, 200) })));
            dots.extend(self.battle.team_dots().into_iter().map(|c| (c, egui::Color32::from_rgb(255, 160, 200))));
            let me = self.player.as_ref().map(|p| (p.walker.cell.0 as f32, p.walker.cell.1 as f32));
            let map_name = self.world.as_ref().map(|w| w.name.clone()).unwrap_or_default();
            if self.settings.perf_overlay && self.perf.vram_due() {
                self.perf.vram = r.vram();
            }
            let perf_lines = self.settings.perf_overlay.then(|| {
                let p = &self.perf;
                let (w, h) = r.size();
                let at = self.player.as_ref().map(|p| p.walker.cell).unwrap_or_default();
                vec![
                    format!("{:.0} fps  {:.1} ms (slowest {:.1})", p.fps, p.avg, p.worst),
                    format!("build {:.1} ms  draw {:.1} ms", p.cpu, p.render),
                    self.ping.map_or("ping -".to_string(), |ms| if ms < 10.0 { format!("ping {ms:.1} ms") } else { format!("ping {ms:.0} ms") }),
                    format!("{map_name} {},{}  {} units  {} drawn", at.0, at.1, self.units.len(), r.actors.len()),
                    match r.scene_size() {
                        (sw, sh) if (sw, sh) != (w, h) => format!("{w}x{h} (world {sw}x{sh})  {}", r.adapter()),
                        _ => format!("{w}x{h}  {}", r.adapter()),
                    },
                    p.vram.map_or("GPU memory -".to_string(), |(used, reserved)| format!("GPU memory {} MB ({} MB reserved)", used >> 20, reserved >> 20)),
                ]
            });
            let load_progress = self.map_load.as_ref().map(MapLoad::progress);
            let out = self.egui_ctx.run_ui(input, |ui| {
                if let Some(lines) = &perf_lines {
                    self.perf.show(ui.ctx(), lines);
                }
                // The game cursor on every screen (its own top layer, so draw order does not matter).
                if let (Some(c), Some((x, y)), false) = (self.cursor_sprite.as_ref(), self.cursor, self.screenshot) {
                    let time = ui.ctx().input(|i| i.time);
                    let action = if self.front.is_some() { hud::cursor::ARROW } else { self.cursor_action };
                    c.draw(ui.ctx(), egui::pos2(x as f32 / ppp, y as f32 / ppp), action, time);
                }
                // Start-up splash (age rating), then the title art behind the login screens.
                if let (Some(s), true) = (self.splash.as_mut(), self.front.is_some()) {
                    if s.intro(ui.ctx()) {
                        return;
                    }
                    s.title(ui.ctx());
                }
                // Loading screen over a map being entered (fades once it is in).
                // From character select too: the screens give way once the map is in.
                let progress = load_progress;
                if let (Some(s), true) = (self.splash.as_mut(), self.front.is_none() || progress.is_some()) {
                    if s.loading(ui.ctx(), progress.is_none() && self.world.is_some() && self.player.is_some(), progress.unwrap_or(1.0)) {
                        return;
                    }
                }
                // Login and character screens replace the game UI until a character is picked.
                if let Some(front) = self.front.as_mut() {
                    let tables = &self.look_tables;
                    let accessory = |v: u16| tables.accessory(v as u32).map(str::to_string);
                    let fctx = screens::Ctx { vfs: self.vfs.as_deref(), job_title: &job_title, job_sprite: &job_sprite, accessory: &accessory };
                    front_action = front.show(ui.ctx(), &fctx);
                    return;
                }
                sent = self.hud.show(ui.ctx(), &labels, &floaters, &bars, &cast_bars, &says);
                // Item links in chat: names for the ones it met, and the one clicked.
                for id in std::mem::take(&mut self.hud.missing_links) {
                    let named = self.item_db.as_ref().and_then(|d| d.get(id)).map(|i| (i.name.clone(), i.slots));
                    self.hud.link_names.insert(id, named.unwrap_or_else(|| (format!("Item #{id}"), 0)));
                }
                // Links in NPC dialogs: <NAVI> plans the route in the Navigation window, as the original.
                match self.hud.dialog_link.take() {
                    Some(npcmarkup::Action::Navi { map, x, y }) => {
                        self.navi.ensure_loaded(self.vfs.as_deref());
                        let label = self.navi.map_name(&map).map_or_else(|| format!("{map} {x}, {y}"), |n| format!("{n} {x}, {y}"));
                        self.navi.go(&map, Some((x, y)), label);
                    }
                    Some(npcmarkup::Action::Url(url)) => self.egui_ctx.open_url(egui::OpenUrl::new_tab(url)),
                    Some(npcmarkup::Action::Item(link)) => self.hud.clicked_link = Some(link),
                    Some(npcmarkup::Action::Tip(_)) | None => {}
                }
                if let (Some(link), Some(vfs)) = (self.hud.clicked_link.take(), self.vfs.as_deref()) {
                    self.item_info.show_item(&self.egui_ctx, vfs, self.item_db.as_ref(), link.id, link.refine, link.cards);
                }
                if let (true, Some(map), Some(me)) = (self.settings.minimap, self.minimap.as_ref(), me) {
                    {
                        let now = std::time::Instant::now();
                        self.viewpoints.retain(|_, v| v.2.is_none_or(|t| t > now));
                        let mut marks: Vec<_> = self.viewpoints.values().map(|v| (v.0, v.1)).collect();
                        marks.extend(self.navi.waypoint(&map_name).map(|(x, y)| ((x as f32, y as f32), egui::Color32::from_rgb(255, 80, 220))));
                        hud::show_minimap(ui.ctx(), map, &map_name, me, &dots, &marks);
                    }
                }
                if let Some(vfs) = self.vfs.as_deref() {
                    self.buffs.show(ui.ctx(), vfs, if self.settings.minimap && self.minimap.is_some() { 270.0 } else { 60.0 });
                }
                if let Some(vfs) = self.vfs.as_deref() {
                    let ppp = ui.ctx().pixels_per_point();
                    let painter = ui.ctx().layer_painter(egui::LayerId::background());
                    for (p, mark, color) in &quest_marks {
                        if let Some(tex) = self.quests.mark_icon(ui.ctx(), vfs, *mark, *color) {
                            let rect = egui::Rect::from_center_size(egui::pos2(p.x / ppp, p.y / ppp), tex.ui_size() * 2.0);
                            painter.image(tex.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                        }
                    }
                }
                if let Some((id, refine, cards)) = self.item_info.show_viewing(ui.ctx(), self.item_db.as_ref(), &self.icons) {
                    if let Some(vfs) = self.vfs.as_deref() {
                        self.item_info.show_item(ui.ctx(), vfs, self.item_db.as_ref(), id, refine, cards);
                    }
                }
                self.item_info.show(ui.ctx(), self.item_db.as_ref());
                let inv: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
                if let Some(req) = self.refine.show(ui.ctx(), &inv, self.item_db.as_ref(), &self.icons, self.hud.status.zeny) {
                    self.queued_commands.push(ro_net::Command::Refine(req));
                }
                let (bank_move, bank_closed) = self.merchant.show_bank(ui.ctx(), self.hud.status.zeny);
                let aid = self.account_id;
                if let Some(z) = bank_move {
                    let req = if z > 0 { ro_net::BankRequest::Deposit { aid, zeny: z as i32 } } else { ro_net::BankRequest::Withdraw { aid, zeny: (-z) as i32 } };
                    self.queued_commands.push(ro_net::Command::Bank(req));
                }
                if bank_closed {
                    self.queued_commands.push(ro_net::Command::Bank(ro_net::BankRequest::Close(aid)));
                }
                let zeny = self.hud.status.zeny;
                if let Some(req) = self.merchant.show(ui.ctx(), self.item_db.as_ref(), &self.icons, zeny) {
                    self.queued_commands.push(ro_net::Command::Merchant(req));
                }
                if let Some(aid) = self.merchant.show_signs(ui.ctx(), &shop_signs) {
                    self.queued_commands.push(ro_net::Command::Merchant(ro_net::MerchantRequest::Browse(aid)));
                }
                if let Some(aid) = self.stores.show_signs(ui.ctx(), &buy_signs) {
                    self.queued_commands.push(ro_net::Command::BuyingStore(ro_net::BuyingStoreRequest::Browse(aid)));
                }
                let inv: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
                if let Some(req) = self.stores.show_buying(ui.ctx(), &inv, self.item_db.as_ref(), &self.icons, zeny) {
                    self.queued_commands.push(ro_net::Command::BuyingStore(req));
                }
                self.stores.sale_badges = self.stores.cash_tabs.get(&8).into_iter().flatten().filter_map(|(id, _)| Some((*id, self.sales.badge(*id)?))).collect();
                let shown = if self.settings.classic_ui { self.stores.show_cash_classic(ui.ctx(), self.item_db.as_ref(), &self.icons) } else { self.stores.show_cash_modern(ui.ctx(), self.item_db.as_ref(), &self.icons) };
                if let Some(req) = shown {
                    self.queued_commands.push(ro_net::Command::CashShop(req));
                }
                if std::mem::take(&mut self.windows.cash_toggle) {
                    let req = if self.stores.cash.take().is_some() { ro_net::CashShopRequest::Close } else { ro_net::CashShopRequest::Open };
                    self.queued_commands.push(ro_net::Command::CashShop(req));
                }
                self.windows.cash_shown = self.stores.cash.is_some();
                if std::mem::take(&mut self.windows.menu_toggled) {
                    self.settings.menu_collapsed = self.windows.menu_collapsed;
                    self.settings.save();
                } else {
                    self.windows.menu_collapsed = self.settings.menu_collapsed;
                }
                let target = self.attack.map(|a| a.0).or_else(|| { let me = self.player.as_ref()?.walker.cell; self.units.iter().filter(|(_, m)| m.kind == 5).filter(|(_, m)| (m.walker.cell.0 - me.0).abs().max((m.walker.cell.1 - me.1).abs()) <= 14).min_by_key(|(_, m)| (m.walker.cell.0 - me.0).pow(2) + (m.walker.cell.1 - me.1).pow(2)).map(|(id, _)| *id) });
                self.sense.show(ui.ctx());
                // What our pet can evolve into, with the materials we hold.
                self.companions.evolutions = match (&self.companions.pet, self.companions.open) {
                    (Some(p), true) => {
                        let name = |id: u32| self.item_db.as_ref().map_or_else(|| format!("Item #{id}"), |d| d.name(id, true));
                        let have = |id: u32| self.inventory.items.values().filter(|i| i.id == id).map(|i| i.amount as u32).sum::<u32>();
                        self.pet_evo.for_class(p.class).iter().map(|(egg, mats)| (*egg, name(*egg), mats.iter().map(|&(m, n)| (name(m), have(m), n)).collect())).collect()
                    }
                    _ => vec![],
                };
                if let Some(a) = self.companions.show(ui.ctx(), target) {
                    self.companion_action = Some(a);
                }
                let mailable: Vec<(u16, u32, u16)> = {
                    let mut v: Vec<_> = self.inventory.items.values().filter(|i| i.equipped == 0).map(|i| (i.index, i.id, i.amount)).collect();
                    v.sort_unstable();
                    v
                };
                self.clan.show(ui.ctx());
                if let Some(v) = self.merge.show(ui.ctx(), &self.inventory.items, self.item_db.as_ref(), &self.icons) {
                    self.queued_commands.push(ro_net::Command::Merge(v));
                }
                if let Some((index, item, set)) = self.packages.show(ui.ctx(), self.item_db.as_ref(), &self.icons) {
                    self.queued_commands.push(ro_net::Command::Item(ro_net::ItemRequest::UsePackage { index, account_id: self.account_id, item, set }));
                }
                if let Some((inviter, accept)) = self.guild.show_offer(ui.ctx()) {
                    self.queued_commands.push(ro_net::Command::Relation(ro_net::RelationRequest::Answer { inviter, accept }));
                }
                self.reputation.show(ui.ctx());
                let db = self.item_db.as_ref();
                self.guild.show_storage_log(ui.ctx(), |id| db.map_or_else(|| format!("Item #{id}"), |d| d.name(id, true)));
                {
                    let me = self.hud.status.name.clone();
                    let leader = self.social.party.as_ref().is_some_and(|p| p.members.iter().any(|m| m.leader && m.name == me));
                    self.game_opts.gm_look = self.gm.then(|| self.player.as_ref().is_some_and(|p| p.font == GM_LOOK_ON));
                    for r in self.game_opts.show(ui.ctx(), leader) {
                        self.queued_commands.push(ro_net::Command::Option(r));
                    }
                    if let Some(on) = self.game_opts.gm_look_asked.take() {
                        self.queued_chat.push(format!("@gmlook {}", if on { "on" } else { "off" }));
                    }
                }
                if let Some(k) = self.ranking.show(ui.ctx()) {
                    self.queued_commands.push(ro_net::Command::Ranking(k));
                }
                for r in self.store_search.show(ui.ctx(), self.item_db.as_ref(), &self.icons) {
                    self.queued_commands.push(ro_net::Command::Search(r));
                }
                {
                    let base = self.portrait.as_ref().map(|p| p.0.clone());
                    match self.stylist.show(ui.ctx(), self.vfs.as_deref(), base) {
                        Some(Some(o)) => self.queued_commands.push(ro_net::Command::Style(ro_net::StyleRequest::Buy { hair_color: o.hair_color, hair: o.hair, clothes_color: o.clothes_color })),
                        Some(None) => self.queued_commands.push(ro_net::Command::Style(ro_net::StyleRequest::Close)),
                        None => {}
                    }
                }
                {
                    let inv: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
                    if let Some(a) = self.lapine.show(ui.ctx(), &inv, self.item_db.as_ref(), &self.icons) {
                        self.queued_commands.push(lapine_command(a));
                    }
                }
                {
                    let inv: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
                    if let Some(a) = self.reform.show(ui.ctx(), &inv, self.item_db.as_ref(), &self.icons) {
                        self.queued_commands.push(ro_net::Command::Reform(match a {
                            reform::ReformAction::Reform { tool, index } => ro_net::ReformRequest::Reform { tool, index },
                            reform::ReformAction::Close => ro_net::ReformRequest::Close,
                        }));
                    }
                }
                {
                    let inv: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
                    let reqs = self.enchant.show(ui.ctx(), &inv, self.item_db.as_ref(), &self.icons, self.hud.status.zeny);
                    self.queued_commands.extend(reqs.into_iter().map(ro_net::Command::Enchant));
                }
                {
                    let inv = &self.inventory;
                    let have = |id: u32| inv.items.values().filter(|i| i.id == id && i.equipped == 0).map(|i| i.amount as u32).sum::<u32>();
                    let reqs = self.market.show(ui.ctx(), self.item_db.as_ref(), &self.icons, self.hud.status.zeny, &have);
                    self.queued_commands.extend(reqs.into_iter().map(ro_net::Command::Market));
                }
                let mut macros_changed = false;
                self.shortcut_win.show(
                    ui.ctx(),
                    shortcuts::Ctx { battle_mode: self.settings.battle_mode, battle_keys: &self.settings.battle_keys, battle_space_chat: self.settings.battle_space_chat, pad: self.pad_preview.map_or_else(|| self.settings.pad_style.resolve(self.pad.detected), |p| p.0), keys: &self.settings.keys },
                );
                if std::mem::take(&mut self.shortcut_win.open_keys) | std::mem::take(&mut self.settings.open_keys) {
                    self.key_win.open = true;
                }
                if self.key_win.show(ui.ctx(), &mut self.settings.keys) {
                    self.settings.save();
                }
                if let Some(line) = self.emote_win.show(ui.ctx(), self.vfs.as_deref(), &mut self.settings.macros, &mut macros_changed) {
                    self.queued_chat.push(line);
                }
                if macros_changed {
                    self.settings.save();
                }
                let (bg_reqs, castle_reqs) = self.battle.show(ui.ctx(), self.hud.status.base_level.max(0) as u32);
                self.queued_commands.extend(bg_reqs.into_iter().map(ro_net::Command::Bg).chain(castle_reqs.into_iter().map(ro_net::Command::Castle)));
                if let Some(navigation::NaviAction::Walk(x, y)) = self.navi.show(ui.ctx(), &map_name) {
                    self.navi_walk = Some((x, y));
                }
                let inv: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
                for r in self.grade.show(ui.ctx(), &inv, self.item_db.as_ref(), &self.icons, self.hud.status.zeny) {
                    self.queued_commands.push(ro_net::Command::Grade(r));
                }
                for req in self.mailbox.show(ui.ctx(), self.item_db.as_ref(), &self.icons, self.hud.status.zeny, &mailable) {
                    self.queued_commands.push(ro_net::Command::Mail(req));
                }
                let me = self.hud.status.name.clone();
                for req in self.rooms.show(ui.ctx(), &me).into_iter().chain(self.rooms.show_signs(ui.ctx(), &room_signs)) {
                    self.queued_commands.push(ro_net::Command::ChatRoom(req));
                }
                let my_level = self.hud.status.base_level.clamp(1, 999) as u16;
                if std::mem::take(&mut self.social.open_agency) {
                    self.agency.toggle();
                }
                for line in self.agency.poll() {
                    self.hud.push_chat(ro_net::ChatKind::System, line);
                }
                let leader = self.social.party.as_ref().map(|p| p.members.iter().any(|m| m.leader && m.char_id == self.char_id));
                let agency_me = agency::Me { char_id: self.char_id, name: &me, level: my_level, leader };
                for a in self.agency.show(ui.ctx(), &agency_me) {
                    match a {
                        agency::AgencyAction::Join { char_id, account_id } => {
                            self.queued_commands.push(ro_net::Command::Party(ro_net::PartyRequest::AgencyJoin { char_id, account_id }));
                        }
                        agency::AgencyAction::Whisper(name) => self.hud.set_input(format!("/w \"{name}\" ")),
                        agency::AgencyAction::Notice(text) => self.hud.push_chat(ro_net::ChatKind::System, text),
                    }
                }
                let job_name = |j: u16| jobs::title(j as i16).to_string();
                for a in self.booking.show(ui.ctx(), my_level, &job_name) {
                    match a {
                        booking::BookingAction::Send(r) => self.queued_commands.push(ro_net::Command::Booking(r)),
                        booking::BookingAction::Whisper(name) => self.hud.set_input(format!("/w \"{name}\" ")),
                    }
                }
                if let Some(req) = self.progress.show(ui.ctx(), self.item_db.as_ref(), &self.icons) {
                    self.queued_commands.push(ro_net::Command::Progress(req));
                }
                if let Some(req) = self.progress.show_attendance(ui.ctx()) {
                    self.queued_commands.push(ro_net::Command::Progress(req));
                }
                if self.quests.open {
                    self.navi.ensure_loaded(self.vfs.as_deref());
                }
                let navi_names = &self.navi;
                if let Some((quest, active)) = self.quests.show(ui.ctx(), &|m| navi_names.map_name(m).map(str::to_string)) {
                    self.queued_commands.push(ro_net::Command::QuestActive { quest, active });
                }
                if let Some((map, cell, label)) = self.quests.navigate.take() {
                    self.navi.ensure_loaded(self.vfs.as_deref());
                    self.navi.go(&map, cell, label);
                }
                if self.esc_menu {
                    match hud::esc_menu(ui.ctx()) {
                        Some(hud::EscChoice::CharSelect) => {
                            self.esc_menu = false;
                            self.queued_commands.push(ro_net::Command::Restart(1));
                        }
                        Some(hud::EscChoice::Options) => {
                            self.esc_menu = false;
                            self.windows.settings = true;
                        }
                        Some(hud::EscChoice::Shortcuts) => {
                            self.esc_menu = false;
                            self.shortcut_win.open = true;
                        }
                        Some(hud::EscChoice::Keys) => {
                            self.esc_menu = false;
                            self.key_win.open = true;
                        }
                        // As request_exit (self is borrowed here): ask the server before leaving.
                        Some(hud::EscChoice::Exit) => {
                            self.esc_menu = false;
                            if self.net.is_some() {
                                self.queued_commands.push(ro_net::Command::Quit);
                                self.quit_asked = Some(std::time::Instant::now());
                            } else {
                                self.quit = true;
                            }
                        }
                        Some(hud::EscChoice::Back) => self.esc_menu = false,
                        None => {}
                    }
                }
                if self.dead {
                    let hint = self.pad_preview.map(|p| p.0).or_else(|| self.pad.in_use().then(|| self.settings.pad_style.resolve(self.pad.detected)));
                    // A Token of Siegfried in any of its forms, or Light of Regene, revives in place.
                    const TOKENS: [u32; 4] = [7621, 6293, 6316, 25310];
                    let token = self.inventory.items.values().find(|i| TOKENS.contains(&i.id) && i.equipped == 0);
                    let revive = token
                        .map(|t| self.item_db.as_ref().map_or_else(|| "Token of Siegfried".to_string(), |db| db.name(t.id, true)))
                        .or_else(|| self.buffs.has("EFST_LIGHT_OF_REGENE").then(|| "Light of Regene".to_string()));
                    match hud::death_window(ui.ctx(), hint, revive.as_deref()) {
                        Some(hud::DeathChoice::SavePoint) => self.queued_commands.push(ro_net::Command::Restart(0)),
                        Some(hud::DeathChoice::Revive) => self.queued_commands.push(ro_net::Command::AutoRevive),
                        None => {}
                    }
                }
                if let Some(feet) = pad_marker {
                    hud::target_marker(ui.ctx(), feet, ppp, self.aiming.is_some());
                }
                // Arrange: the layout editor takes over from the settings window.
                if std::mem::take(&mut self.settings.arrange) {
                    self.arrange.open = true;
                    self.windows.settings = false;
                }
                if self.arrange.show(ui.ctx(), &mut self.settings.hud_layout) {
                    self.settings.save();
                }
                hudlayout::apply(ui.ctx(), &self.settings.hud_layout);
                self.settings.pad_live = self.pad.connected().then_some(self.pad.raw);
                if self.windows.settings && self.settings.show(ui.ctx(), &mut self.windows.settings) {
                    self.ui_scale = self.settings.ui_scale;
                    uitex::set_upscaler(ro_render::Upscaler::new(self.settings.upscale.clone()));
                    if let Some(a) = self.audio.as_mut() {
                        a.set_bgm_volume(if self.settings.bgm_off { 0.0 } else { self.settings.bgm_volume });
                        a.sfx_volume = if self.settings.sfx_off { 0.0 } else { self.settings.sfx_volume };
                    }
                }
                self.books.show(ui.ctx());
                if let (Some(a), Some((tx, _))) = (self.captcha.show(ui.ctx()), self.net.as_ref()) {
                    let _ = tx.send(ro_net::Command::Captcha(ro_net::CaptchaRequest::Answer(a)));
                }
                let (units, my_id, my_name) = (&self.units, self.account_id, self.hud.status.name.clone());
                let me = self.player.as_ref().map(|p| p.walker.cell);
                let name_of = |id: u32| if id == my_id { my_name.clone() } else { units.get(&id).map_or_else(|| format!("Player #{id}"), |m| m.name.clone()) };
                if let Some(act) = self.reporter.show(ui.ctx(), name_of) {
                    let req = match act {
                        captcha::ReporterAction::Scan(radius) => me.map(|(x, y)| ro_net::MacroReport::Scan { x: x as u16, y: y as u16, radius }),
                        captcha::ReporterAction::Apply(id) => Some(ro_net::MacroReport::Apply(id)),
                    };
                    if let (Some(req), Some((tx, _))) = (req, self.net.as_ref()) {
                        let _ = tx.send(ro_net::Command::MacroReport(req));
                    }
                }
                let db = self.item_db.as_ref();
                if let Some(req) = self.sales.show_tool(
ui.ctx(), &self.icons, |id| db.map_or_else(|| format!("item {id}"), |d| d.name(id, true))) {
                    if let Some((tx, _)) = self.net.as_ref() {
                        let _ = tx.send(ro_net::Command::Sale(self.account_id, req));
                    }
                }
                let db = self.item_db.as_ref();
                if let Some(act) = self.roulette.show(
ui.ctx(), &self.icons, |id| db.map_or_else(|| format!("item {id}"), |d| d.name(id, true))) {
                    let r = match act {
                        roulette::Action::Spin => ro_net::RouletteRequest::Spin,
                        roulette::Action::Claim => ro_net::RouletteRequest::Claim,
                        roulette::Action::Close => ro_net::RouletteRequest::Close,
                    };
                    if let Some((tx, _)) = self.net.as_ref() {
                        let _ = tx.send(ro_net::Command::Roulette(r));
                    }
                }
                if self.memorial.show(ui.ctx()) {
                    if let Some((tx, _)) = self.net.as_ref() {
                        let _ = tx.send(ro_net::Command::MemorialDestroy);
                    }
                }
                answer = self.hud.show_dialog(ui.ctx(), &mut self.dialog, self.cutin.as_ref());
                ui_action = self.windows.show(ui.ctx(), &view);
                self.social.show_party = self.windows.party;
                self.social.show_friends = self.windows.friends;
                social_cmds = self.social.show(ui.ctx(), &social_ctx);
                // Guild window (menu button / Alt+G).
                if self.windows.guild != self.guild.open {
                    self.guild.toggle();
                }
                let guild_ctx = guild::Ctx {
                    account_id: self.account_id,
                    char_id: self.char_id,
                    has_emperium: items.iter().any(|i| i.id == guild::EMPERIUM),
                    job_name: &job_title,
                };
                social_cmds.extend(self.guild.show(ui.ctx(), &guild_ctx));
                self.windows.guild = self.guild.open;
                (self.windows.party, self.windows.friends) = (self.social.show_party, self.social.show_friends);
                // Menu for a clicked player.
                if let Some((id, name, pos)) = self.player_menu.clone() {
                    let mut keep = true;
                    egui::Area::new(egui::Id::unique("player menu")).fixed_pos(pos).show(ui.ctx(), |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.label(egui::RichText::new(&name).strong());
                            if ui.button("Trade").clicked() {
                                player_cmd = Some((ro_net::Command::Trade(ro_net::TradeRequest::Ask { target: id }), Some(name.clone())));
                                keep = false;
                            }
                            if ui.button("View equipment").clicked() {
                                player_cmd = Some((ro_net::Command::ViewEquip(id), None));
                                keep = false;
                            }
                            if ui.button("Invite to party").clicked() {
                                player_cmd = Some((ro_net::Command::Party(ro_net::PartyRequest::Invite { name: name.clone() }), None));
                                keep = false;
                            }
                            if ui.button("Add friend").clicked() {
                                player_cmd = Some((ro_net::Command::Friend(ro_net::FriendRequest::Add { name: name.clone() }), None));
                                keep = false;
                            }
                            if ui.button("Adopt").on_hover_text("A married couple can adopt a player as their child").clicked() {
                                player_cmd = Some((ro_net::Command::Adopt(ro_net::AdoptRequest::Ask(id)), None));
                                keep = false;
                            }
                            if self.guild.id != 0 && ui.button("Invite to guild").clicked() {
                                player_cmd = Some((ro_net::Command::Guild(ro_net::GuildRequest::Invite { name: name.clone() }), None));
                                keep = false;
                            }
                            // Guild masters: offer an alliance, or mark the other guild an enemy.
                            let other_guild = self.units.get(&id).map_or(0, |m| m.guild);
                            if self.guild.master && other_guild != 0 && other_guild != self.guild.id {
                                if ui.button("Ask for an alliance").clicked() {
                                    player_cmd = Some((ro_net::Command::Relation(ro_net::RelationRequest::Ally { target: id, me: self.account_id, char_id: self.char_id }), None));
                                    keep = false;
                                }
                                if ui.button("Set guild as enemy").clicked() {
                                    player_cmd = Some((ro_net::Command::Relation(ro_net::RelationRequest::Enemy(id)), None));
                                    keep = false;
                                }
                            }
                            // Game Masters (config `gm`): the original's GM right-click actions.
                            if self.gm {
                                ui.separator();
                                let gm = |r| Some((ro_net::Command::Gm(r), None));
                                let kick = msgstr::get(127, &[]).unwrap_or_else(|| "Kick Character Out".into());
                                for (label, r) in [
                                    (kick.as_str(), ro_net::GmRequest::Kick(id)),
                                    ("Mute for 60 minutes", ro_net::GmRequest::Mute(name.clone())),
                                    ("Remove all equipment", ro_net::GmRequest::FullStrip(id)),
                                    ("Recall here", ro_net::GmRequest::RecallId(id)),
                                    ("Go to", ro_net::GmRequest::GoTo(id)),
                                    ("Check stats", ro_net::GmRequest::Check(name.clone())),
                                ] {
                                    if ui.button(label).clicked() {
                                        player_cmd = gm(r);
                                        keep = false;
                                    }
                                }
                            }
                            if ui.button("Close").clicked() {
                                keep = false;
                            }
                        });
                    });
                    if !keep {
                        self.player_menu = None;
                    }
                }
                if let Some((skill, _)) = self.aiming {
                    let name = self.skills.iter().find(|s| s.id == skill).map(|s| self.skill_names.display(&s.name)).unwrap_or_default();
                    egui::Area::new(egui::Id::unique("aiming")).anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 16.0)).show(ui.ctx(), |ui| {
                        ui.label(egui::RichText::new(format!("{name}: choose a target (Esc to cancel)")).size(15.0).color(egui::Color32::from_rgb(255, 220, 120)));
                    });
                }
            });
            drop(view);
            // Handled next tick: the renderer is borrowed here.
            if ui_action.is_some() {
                self.queued_action = ui_action;
            }
            if let Some((cmd, partner)) = player_cmd {
                self.trade_partner = partner.or(self.trade_partner.take());
                social_cmds.push(cmd);
            }
            self.queued_commands.extend(social_cmds);
            match front_action {
                Some(screens::Action::Send(cmd)) => self.queued_commands.push(cmd),
                Some(screens::Action::Logout) => {
                    // A fresh session back at the login screen.
                    let remember = self.front.as_ref().is_some_and(|f| f.remember);
                    let user = self.front.as_ref().map(|f| f.user().to_string()).unwrap_or_default();
                    self.front = Some(screens::Front::new(user, remember));
                    if let (Some(a), Some(v)) = (self.audio.as_mut(), self.vfs.as_deref()) {
                        a.title(v);
                    }
                    self.net = Some(ro_net::spawn_session(self.session.clone()));
                }
                Some(screens::Action::Quit) => self.quit = true,
                None => {}
            }
            if answer.is_none() {
                if let Some(d) = self.dialog.as_mut().filter(|d| !matches!(d.mode, hud::DialogMode::Reading)) {
                    if let Some(a) = self.npc_answers.pop_front() {
                        tracing::info!(answer = %a, "npc answer");
                        let reply = match a.split_once(':') {
                            Some(("menu", n)) => ro_net::NpcReply::Menu(n.parse().unwrap_or(1)),
                            Some(("num", n)) => ro_net::NpcReply::Number(n.parse().unwrap_or(0)),
                            Some(("text", t)) => ro_net::NpcReply::Text(t.to_string()),
                            _ if a == "close" => ro_net::NpcReply::Close,
                            _ => ro_net::NpcReply::Next,
                        };
                        let npc = d.npc;
                        match reply {
                            ro_net::NpcReply::Close => self.dialog = None,
                            ro_net::NpcReply::Next | ro_net::NpcReply::Menu(_) => {
                                d.mode = hud::DialogMode::Reading;
                                d.lines.clear();
                            }
                            _ => d.mode = hud::DialogMode::Reading,
                        }
                        answer = Some((npc, reply));
                    }
                }
            }
            if let (Some((npc, reply)), Some((tx, _))) = (answer, self.net.as_ref()) {
                if reply == ro_net::NpcReply::Close {
                    self.cutin = None;
                }
                let _ = tx.send(ro_net::Command::Npc { npc, reply });
            }
            // The game draws its own cursor; egui would otherwise show the system one again.
            let mut platform = out.platform_output;
            if self.cursor_sprite.is_some() {
                platform.cursor_icon = egui::CursorIcon::None;
            }
            state.handle_platform_output(window, platform);
            let primitives = self.egui_ctx.tessellate(out.shapes, out.pixels_per_point);
            r.set_ui(ro_render::UiFrame { primitives, textures: out.textures_delta, pixels_per_point: out.pixels_per_point });
            if let Some(text) = sent {
                self.queued_chat.push(text);
                if !self.settings.battle_mode {
                    self.hud.focus_chat();
                }
            }
        }

        if let Some((path, w, h)) = &self.capture {
            let ready = self.ready_at.is_some_and(|t| t.elapsed().as_secs_f32() * 1000.0 >= self.capture_after);
            if ready {
                match r.capture(*w, *h) {
                    Ok(img) => match image::save_buffer(path, &img.pixels, img.width, img.height, image::ColorType::Rgba8) {
                        Ok(()) => {
                            let at = self.player.as_ref().map(|p| p.walker.cell);
                            tracing::info!(path = %path.display(), ?at, "captured");
                        }
                        Err(e) => tracing::error!("save capture: {e:#}"),
                    },
                    Err(e) => tracing::error!("capture: {e:#}"),
                }
                self.capture = None;
                el.exit();
            }
            return;
        }

        if std::mem::take(&mut self.screenshot) {
            // Rendered again offscreen (with or without the UI) and saved; the window keeps
            // the previous frame for this one.
            let (w, h) = r.size();
            match r.capture_with(w, h, self.settings.screenshot_ui) {
                Ok(img) => {
                    let path = screenshot_path();
                    self.hud.push_chat(ro_net::ChatKind::System, format!("Screenshot saved: {}", path.display()));
                    // PNG (lossless), encoded off the frame loop.
                    std::thread::spawn(move || {
                        if let Err(e) = image::save_buffer(&path, &img.pixels, img.width, img.height, image::ColorType::Rgba8) {
                            tracing::error!(path = %path.display(), "save screenshot: {e:#}");
                        }
                    });
                }
                Err(e) => tracing::error!("screenshot: {e:#}"),
            }
        } else {
            let drawing = std::time::Instant::now();
            // Minimised: nothing to see (the UI's texture changes wait for the next frame drawn).
            if !self.minimised {
                if let Err(e) = r.frame() {
                    tracing::error!("frame: {e:#}");
                }
            }
            self.perf.split((drawing - self.last_tick).as_secs_f32() * 1000.0, drawing.elapsed().as_secs_f32() * 1000.0);
        }
        if let Some(fps) = self.fps.tick() {
            let (w, h) = r.size();
            let at = self.player.as_ref().map(|p| p.walker.cell).unwrap_or_default();
            let map = self.world.as_ref().map_or("-", |w| w.name.as_str());
            let title = match &self.status {
                Some(s) => format!("Tenseiga — {s}"),
                None if self.world.is_none() && self.net.is_some() => "Tenseiga — connecting…".to_string(),
                None => format!("Tenseiga — {map} {},{} — {w}x{h} — {fps:.0} fps", at.0, at.1),
            };
            window.set_title(&title);
        }
    }

    /// How long a frame should take when the window isn't in front: minimised, the game still
    /// ticks (server messages, chat, walking) ten times a second; behind another window, at
    /// Settings' background frame rate. None: as fast as the display allows.
    pub(crate) fn frame_gap(&self) -> Option<std::time::Duration> {
        if self.capture.is_some() {
            return None; // hidden capture windows never have focus
        }
        if self.minimised {
            Some(std::time::Duration::from_millis(100))
        } else if !self.focused && self.settings.background_fps > 0 {
            Some(std::time::Duration::from_secs(1) / self.settings.background_fps)
        } else {
            None
        }
    }
}

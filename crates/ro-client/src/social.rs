//! Shops, Kafra storage, trading, party, friends and guild: state from the server and the
//! windows that act on it. `show` returns the commands the player chose.

use std::collections::{BTreeMap, HashMap};

use egui::{Color32, RichText, Vec2};
use ro_game::items::{InvItem, ItemDb};
use ro_net::{Command, Friend, FriendRequest, PartyMember, PartyRequest, SellOffer, ShopItem, ShopRequest, StorageRequest, TradeRequest};

/// An NPC shop in progress.
pub enum Shop {
    /// The NPC asked: buy or sell?
    Choose { npc: u32 },
    Buy { items: Vec<ShopItem>, cart: BTreeMap<u32, u16> },
    Sell { offers: Vec<SellOffer>, cart: BTreeMap<u16, u16> },
    /// A cashshop / itemshop / pointshop NPC: prices in its currency (`cash` held), with
    /// `use_kafra` of a purchase paid from Kafra points.
    Cash { items: Vec<ShopItem>, cart: BTreeMap<u32, u16>, cash: u32, kafra: u32, use_kafra: u32 },
}

#[derive(Default)]
pub struct Storage {
    pub open: bool,
    pub items: BTreeMap<u16, InvItem>,
    pub count: (u16, u16),
    /// A stack being moved, waiting for "how many": (move, amount typed).
    asking: Option<(StorageDrag, String)>,
}

/// A trade in progress (or being requested).
#[derive(Default)]
pub struct Trade {
    pub partner: String,
    pub ours: Vec<(u32, i32)>,
    pub theirs: Vec<(u32, i32)>,
    pub zeny_ours: i32,
    pub zeny_theirs: i32,
    pub we_locked: bool,
    pub they_locked: bool,
    pub zeny_input: String,
}

#[derive(Default)]
pub struct Party {
    pub name: String,
    pub members: Vec<PartyMember>,
    pub hp: HashMap<u32, (i32, i32)>,
}

/// A yes/no question from another player.
pub enum Prompt {
    Trade { name: String },
    Party { party: u32, name: String },
    Friend { account_id: u32, char_id: u32, name: String },
    Guild { guild: u32, name: String },
    /// An Adventurer's Agency join request to our party.
    Agency(ro_net::AgencyAsk),
}

#[derive(Default)]
pub struct Social {
    pub shop: Option<Shop>,
    pub storage: Storage,
    pub trade: Option<Trade>,
    pub party: Option<Party>,
    /// Party members the server reported dead (ZC_GROUP_ISALIVE), by account id.
    pub party_dead: std::collections::HashSet<u32>,
    pub friends: Vec<Friend>,
    pub guild: Option<String>,
    pub prompts: Vec<Prompt>,
    pub show_party: bool,
    pub show_friends: bool,
    /// The party window's Recruit button was pressed (opens the Adventurer's Agency).
    pub open_agency: bool,
    party_name: String,
    invite_name: String,
    friend_name: String,
}

/// What the windows need from the rest of the client.
pub struct Ctx<'a> {
    pub db: Option<&'a ItemDb>,
    pub icons: &'a HashMap<u32, egui::TextureHandle>,
    pub inventory: &'a [&'a InvItem],
    pub zeny: i64,
    pub my_name: &'a str,
}

fn name(db: Option<&ItemDb>, id: u32) -> String {
    db.map_or_else(|| format!("Item #{id}"), |d| d.name(id, true))
}

fn icon(ui: &mut egui::Ui, icons: &HashMap<u32, egui::TextureHandle>, id: u32) {
    match icons.get(&id) {
        Some(t) => {
            ui.image((t.id(), Vec2::splat(24.0)));
        }
        None => {
            ui.allocate_exact_size(Vec2::splat(24.0), egui::Sense::hover());
        }
    }
}

impl Social {
    /// Accepts the oldest pending request (scripted tests).
    pub fn accept_first(&mut self) -> Option<Command> {
        if self.prompts.is_empty() {
            return None;
        }
        Some(match self.prompts.remove(0) {
            Prompt::Trade { .. } => Command::Trade(TradeRequest::Answer { accept: true }),
            Prompt::Party { party, .. } => Command::Party(PartyRequest::Answer { party, accept: true }),
            Prompt::Friend { account_id, char_id, .. } => Command::Friend(FriendRequest::Answer { account_id, char_id, accept: true }),
            Prompt::Guild { guild, .. } => Command::Guild(ro_net::GuildRequest::Answer { guild, accept: true }),
            Prompt::Agency(a) => Command::Party(PartyRequest::AgencyAnswer { char_id: a.char_id, account_id: a.account_id, accept: true }),
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, c: &Ctx) -> Vec<Command> {
        let mut out = vec![];
        self.prompts(ctx, &mut out);
        self.shop_window(ctx, c, &mut out);
        self.storage_window(ctx, c, &mut out);
        self.trade_window(ctx, c, &mut out);
        self.party_window(ctx, c, &mut out);
        self.friends_window(ctx, &mut out);
        out
    }

    fn prompts(&mut self, ctx: &egui::Context, out: &mut Vec<Command>) {
        let mut answered = None;
        if let Some((i, p)) = self.prompts.iter().enumerate().next() {
            let text = match p {
                Prompt::Trade { name } => format!("{name} wants to trade with you."),
                Prompt::Party { name, .. } => format!("You are invited to the party \"{name}\"."),
                Prompt::Friend { name, .. } => format!("{name} wants to be your friend."),
                Prompt::Guild { name, .. } => format!("You are invited to the guild \"{name}\"."),
                Prompt::Agency(a) => {
                    let asked = crate::msgstr::get(3378, &[&a.name]).unwrap_or_else(|| format!("[{}] has requested to join the party.", a.name));
                    format!("{asked}\nLv {} {}", a.level, crate::jobs::title(a.job as i16))
                }
            };
            crate::classic::show(ctx, egui::Window::new("Request").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, Vec2::new(0.0, 120.0)), "Request", false, |ui| {
                ui.label(text);
                ui.horizontal(|ui| {
                    let yes = ui.button("Accept").clicked();
                    let no = ui.button("Decline").clicked();
                    if yes || no {
                        answered = Some(i);
                        out.push(match p {
                            Prompt::Trade { .. } => Command::Trade(TradeRequest::Answer { accept: yes }),
                            Prompt::Party { party, .. } => Command::Party(PartyRequest::Answer { party: *party, accept: yes }),
                            Prompt::Friend { account_id, char_id, .. } => {
                                Command::Friend(FriendRequest::Answer { account_id: *account_id, char_id: *char_id, accept: yes })
                            }
                            Prompt::Guild { guild, .. } => Command::Guild(ro_net::GuildRequest::Answer { guild: *guild, accept: yes }),
                            Prompt::Agency(a) => Command::Party(PartyRequest::AgencyAnswer { char_id: a.char_id, account_id: a.account_id, accept: yes }),
                        });
                    }
                });
            });
        }
        if let Some(i) = answered {
            self.prompts.remove(i);
        }
    }

    fn shop_window(&mut self, ctx: &egui::Context, c: &Ctx, out: &mut Vec<Command>) {
        let Some(shop) = self.shop.as_mut() else { return };
        let mut close = false;
        let mut open = true;
        let title = match shop {
            Shop::Choose { .. } => "Shop",
            Shop::Buy { .. } => "Buy",
            Shop::Sell { .. } => "Sell",
            Shop::Cash { .. } => "Shop",
        };
        crate::classic::show(ctx, egui::Window::new(title).open(&mut open).resizable(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO), &title, true, |ui| match shop {
            Shop::Choose { npc } => {
                ui.horizontal(|ui| {
                    if ui.button("Buy").clicked() {
                        out.push(Command::Shop(ShopRequest::Deal { npc: *npc, buy: true }));
                    }
                    if ui.button("Sell").clicked() {
                        out.push(Command::Shop(ShopRequest::Deal { npc: *npc, buy: false }));
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            }
            Shop::Buy { items, cart } => {
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    egui::Grid::new("buy").num_columns(4).spacing(Vec2::new(8.0, 4.0)).show(ui, |ui| {
                        for it in items.iter() {
                            icon(ui, c.icons, it.id);
                            ui.label(name(c.db, it.id));
                            ui.label(RichText::new(format!("{} z", it.discount_price)).color(crate::classic::ink(Color32::from_rgb(240, 210, 120))));
                            let n = cart.entry(it.id).or_insert(0);
                            ui.add(egui::DragValue::new(n).range(0..=999));
                            ui.end_row();
                        }
                    });
                });
                cart.retain(|_, n| *n > 0);
                let total: u64 = items.iter().map(|i| i.discount_price as u64 * *cart.get(&i.id).unwrap_or(&0) as u64).sum();
                ui.separator();
                ui.horizontal(|ui| {
                    let affordable = total as i64 <= c.zeny;
                    ui.label(RichText::new(format!("Total {total} z  (you have {} z)", c.zeny)).color(crate::classic::ink(if affordable { Color32::LIGHT_GRAY } else { Color32::from_rgb(255, 120, 100) })));
                    if ui.add_enabled(!cart.is_empty() && affordable, egui::Button::new("Buy")).clicked() {
                        out.push(Command::Shop(ShopRequest::Buy(cart.iter().map(|(id, n)| (*id, *n)).collect())));
                        close = true;
                    }
                });
            }
            Shop::Cash { items, cart, cash, kafra, use_kafra } => {
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    egui::Grid::new("cash_buy").num_columns(4).spacing(Vec2::new(8.0, 4.0)).show(ui, |ui| {
                        for it in items.iter() {
                            icon(ui, c.icons, it.id);
                            ui.label(name(c.db, it.id));
                            ui.label(RichText::new(format!("{} P", it.price)).color(crate::classic::ink(Color32::from_rgb(140, 210, 255))));
                            let n = cart.entry(it.id).or_insert(0);
                            ui.add(egui::DragValue::new(n).range(0..=99));
                            ui.end_row();
                        }
                    });
                });
                cart.retain(|_, n| *n > 0);
                let total: u64 = items.iter().map(|i| i.price as u64 * *cart.get(&i.id).unwrap_or(&0) as u64).sum();
                ui.separator();
                if *kafra > 0 {
                    ui.horizontal(|ui| {
                        ui.label(format!("Use Kafra points ({kafra})"));
                        ui.add(egui::DragValue::new(use_kafra).range(0..=(*kafra).min(total as u32)));
                    });
                }
                *use_kafra = (*use_kafra).min(total as u32);
                ui.horizontal(|ui| {
                    let affordable = total <= *cash as u64 + *use_kafra as u64;
                    let color = if affordable { Color32::LIGHT_GRAY } else { Color32::from_rgb(255, 120, 100) };
                    ui.label(RichText::new(format!("Total {total} P  (you have {cash} P)")).color(crate::classic::ink(color)));
                    if ui.add_enabled(!cart.is_empty() && affordable, egui::Button::new("Buy")).clicked() {
                        out.push(Command::Shop(ShopRequest::CashBuy { kafra: *use_kafra, items: cart.iter().map(|(id, n)| (*id, *n)).collect() }));
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            }
            Shop::Sell { offers, cart } => {
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    egui::Grid::new("sell").num_columns(4).spacing(Vec2::new(8.0, 4.0)).show(ui, |ui| {
                        for o in offers.iter() {
                            let Some(it) = c.inventory.iter().find(|i| i.index == o.index) else { continue };
                            if it.equipped != 0 {
                                continue;
                            }
                            icon(ui, c.icons, it.id);
                            ui.label(format!("{} x{}", name(c.db, it.id), it.amount));
                            ui.label(RichText::new(format!("{} z", o.overcharge_price)).color(crate::classic::ink(Color32::from_rgb(240, 210, 120))));
                            let n = cart.entry(o.index).or_insert(0);
                            ui.add(egui::DragValue::new(n).range(0..=it.amount));
                            ui.end_row();
                        }
                    });
                });
                cart.retain(|_, n| *n > 0);
                let total: u64 = offers.iter().map(|o| o.overcharge_price as u64 * *cart.get(&o.index).unwrap_or(&0) as u64).sum();
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(format!("Total {total} z"));
                    if ui.add_enabled(!cart.is_empty(), egui::Button::new("Sell")).clicked() {
                        out.push(Command::Shop(ShopRequest::Sell(cart.iter().map(|(i, n)| (*i, *n)).collect())));
                        close = true;
                    }
                });
            }
        });
        if close || !open {
            out.push(Command::ShopClosed);
            self.shop = None;
        }
    }

    fn storage_window(&mut self, ctx: &egui::Context, c: &Ctx, out: &mut Vec<Command>) {
        if !self.storage.open {
            return;
        }
        let mut open = true;
        egui::Window::new(format!("Storage ({}/{})", self.storage.count.0, self.storage.count.1))
            .id(egui::Id::unique("storage"))
            .open(&mut open)
            .resizable(false)
            .default_pos(egui::pos2(60.0, 280.0))
            .show(ctx, |ui| {
                // Items move by double-click or by dragging them to the other column (from
                // the inventory window too). The bool is a Shift+right-click: to the favourites tab.
                let row = |ui: &mut egui::Ui, id: u32, amount: u16, drag: StorageDrag, tip: &str| {
                    let r = ui
                        .horizontal(|ui| {
                            icon(ui, c.icons, id);
                            ui.label(format!("{} x{amount}", name(c.db, id)));
                        })
                        .response;
                    let r = ui.interact(r.rect, ui.scope_id().with(("storage_row", drag.clone())), egui::Sense::click_and_drag());
                    if r.hovered() {
                        ui.painter().rect_filled(r.rect, 3.0, Color32::from_white_alpha(18));
                    }
                    if r.drag_started() {
                        r.dnd_set_drag_payload(drag.clone());
                    }
                    let favorite = r.secondary_clicked() && ui.input(|i| i.modifiers.shift);
                    (r.on_hover_text(tip).double_clicked() || favorite).then_some((drag, favorite))
                };
                let mut moved = vec![];
                ui.columns(2, |cols| {
                    cols[0].label(RichText::new("Storage").strong());
                    let dropped = drop_column(&mut cols[0], |ui| {
                        ui.set_min_size(Vec2::new(ui.available_width(), 300.0));
                        egui::ScrollArea::vertical().id_salt("st").max_height(300.0).show(ui, |ui| {
                            for it in self.storage.items.values() {
                                // Shift+right-click: into the inventory's favourites tab, as the original.
                                match row(ui, it.id, it.amount, StorageDrag::Take(it.index, it.amount), "Double-click or drag to take; Shift+right-click to put it in the inventory's favourites") {
                                    Some((_, true)) => out.push(Command::Storage(StorageRequest::TakeFavorite { index: it.index, amount: it.amount as u32 })),
                                    r => moved.extend(r.map(|(d, _)| d)),
                                }
                            }
                        });
                    });
                    moved.extend(dropped.filter(|d| matches!(**d, StorageDrag::Put(..))).map(|d| (*d).clone()));
                    cols[1].label(RichText::new("Inventory").strong());
                    let dropped = drop_column(&mut cols[1], |ui| {
                        ui.set_min_size(Vec2::new(ui.available_width(), 300.0));
                        egui::ScrollArea::vertical().id_salt("inv").max_height(300.0).show(ui, |ui| {
                            for it in c.inventory.iter().filter(|i| i.equipped == 0) {
                                moved.extend(row(ui, it.id, it.amount, StorageDrag::Put(it.index, it.amount), "Double-click or drag to store").filter(|(_, fav)| !fav).map(|(d, _)| d));
                            }
                        });
                    });
                    moved.extend(dropped.filter(|d| matches!(**d, StorageDrag::Take(..))).map(|d| (*d).clone()));
                });
                // An item dragged in from the inventory window carries its item id.
                let window = ui.min_rect();
                if let Some(d) = egui::DragAndDrop::payload::<crate::windows::ItemDrag>(ui.ctx()).filter(|d| d.loose) {
                    let over = ui.ctx().pointer_hover_pos().is_some_and(|p| window.contains(p));
                    if over && ui.input(|i| i.pointer.any_released()) {
                        moved.push(StorageDrag::Put(d.index, d.amount));
                        egui::DragAndDrop::clear_payload(ui.ctx());
                    }
                }
                // Single items move at once; stacks ask how many, as the original does.
                for m in moved {
                    let n = match &m { StorageDrag::Take(_, n) | StorageDrag::Put(_, n) => *n };
                    if n > 1 {
                        self.storage.asking = Some((m.clone(), n.to_string()));
                    } else {
                        out.push(storage_move(&m, n as u32));
                    }
                }
            });
        if let Some((m, typed)) = self.storage.asking.as_mut() {
            let max = match m { StorageDrag::Take(_, n) | StorageDrag::Put(_, n) => *n as u32 };
            let mut done = None;
            egui::Window::new("How many?").id(egui::Id::unique("storage_amount")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let r = ui.add(egui::TextEdit::singleline(typed).desired_width(80.0));
                    r.request_focus();
                    ui.label(format!("/ {max}"));
                });
                let n = typed.trim().parse::<u32>().unwrap_or(0).min(max);
                ui.horizontal(|ui| {
                    if ui.add_enabled(n > 0, egui::Button::new("OK")).clicked() || (n > 0 && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                        done = Some(Some(n));
                    }
                    if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        done = Some(None);
                    }
                });
            });
            if let Some(choice) = done {
                if let Some(n) = choice {
                    out.push(storage_move(m, n));
                }
                self.storage.asking = None;
            }
        }
        // What is being dragged, under the pointer.
        if let (Some(d), Some(p)) = (egui::DragAndDrop::payload::<StorageDrag>(ctx), ctx.pointer_interact_pos()) {
            let (index, take) = match *d { StorageDrag::Take(i, _) => (i, true), StorageDrag::Put(i, _) => (i, false) };
            let id = if take { self.storage.items.values().find(|i| i.index == index).map(|i| i.id) } else { c.inventory.iter().find(|i| i.index == index).map(|i| i.id) };
            if let Some(t) = id.and_then(|id| c.icons.get(&id)) {
                egui::Area::new(egui::Id::unique("storage_drag")).fixed_pos(p + Vec2::new(-12.0, -12.0)).order(egui::Order::Tooltip).interactable(false).show(ctx, |ui| {
                    ui.image((t.id(), Vec2::splat(24.0)));
                });
            }
        }
        if !open {
            out.push(Command::Storage(StorageRequest::Close));
            self.storage.open = false;
        }
    }

    fn trade_window(&mut self, ctx: &egui::Context, c: &Ctx, out: &mut Vec<Command>) {
        let Some(t) = self.trade.as_mut() else { return };
        let mut open = true;
        crate::classic::show(ctx, egui::Window::new(format!("Trade with {}", t.partner)).id(egui::Id::unique("trade")).open(&mut open).resizable(false), &format!("Trade with {}", t.partner), true, |ui| {
            ui.columns(2, |cols| {
                for (col, title, items, zeny, locked) in [
                    (0, t.partner.as_str(), &t.theirs, t.zeny_theirs, t.they_locked),
                    (1, c.my_name, &t.ours, t.zeny_ours, t.we_locked),
                ] {
                    let ui = &mut cols[col];
                    ui.label(RichText::new(title).strong().color(crate::classic::ink(if locked { Color32::from_rgb(120, 220, 120) } else { Color32::WHITE })));
                    for (id, n) in items {
                        ui.horizontal(|ui| {
                            icon(ui, c.icons, *id);
                            ui.label(format!("{} x{n}", name(c.db, *id)));
                        });
                    }
                    ui.label(format!("{zeny} z"));
                }
            });
            ui.separator();
            if !t.we_locked {
                ui.label(RichText::new("Add from inventory (double-click):").size(11.0).color(crate::classic::ink(Color32::GRAY)));
                egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                    for it in c.inventory.iter().filter(|i| i.equipped == 0) {
                        let r = ui.add(egui::Label::new(format!("{} x{}", name(c.db, it.id), it.amount)).sense(egui::Sense::click()));
                        if r.double_clicked() {
                            out.push(Command::Trade(TradeRequest::Add { index: it.index, amount: it.amount as i32 }));
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut t.zeny_input).hint_text("zeny").desired_width(100.0));
                    if ui.button("Add zeny").clicked() {
                        if let Ok(z) = t.zeny_input.trim().parse::<i32>() {
                            out.push(Command::Trade(TradeRequest::Add { index: 0, amount: z }));
                            t.zeny_ours = z;
                        }
                    }
                });
            }
            ui.horizontal(|ui| {
                if !t.we_locked && ui.button("OK (lock)").clicked() {
                    out.push(Command::Trade(TradeRequest::Ok));
                }
                if t.we_locked && t.they_locked && ui.button("Trade").clicked() {
                    out.push(Command::Trade(TradeRequest::Commit));
                }
                if ui.button("Cancel").clicked() {
                    out.push(Command::Trade(TradeRequest::Cancel));
                }
            });
        });
        if !open {
            out.push(Command::Trade(TradeRequest::Cancel));
        }
    }

    fn party_window(&mut self, ctx: &egui::Context, c: &Ctx, out: &mut Vec<Command>) {
        if !self.show_party {
            return;
        }
        let mut open = true;
        crate::classic::show(ctx, egui::Window::new("Party").open(&mut open).resizable(false).default_pos(egui::pos2(60.0, 260.0)), "Party", true, |ui| {
            match &self.party {
                None => {
                    ui.label("You are not in a party.");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.party_name).hint_text("party name").desired_width(140.0));
                        if ui.add_enabled(!self.party_name.trim().is_empty(), egui::Button::new("Create")).clicked() {
                            out.push(Command::Party(PartyRequest::Create { name: self.party_name.trim().to_string() }));
                        }
                    });
                }
                Some(p) => {
                    ui.label(RichText::new(&p.name).strong());
                    let leading = p.members.iter().any(|m| m.leader && m.name == c.my_name);
                    for m in &p.members {
                        ui.horizontal(|ui| {
                            let dead = self.party_dead.contains(&m.account_id);
                            let color = if dead { Color32::from_rgb(200, 90, 90) } else if m.online { Color32::WHITE } else { Color32::GRAY };
                            let crown = if m.leader { "[Leader] " } else { "" };
                            let tomb = if dead { " (dead)" } else { "" };
                            let label = ui.label(RichText::new(format!("{crown}{} (Lv {}){tomb}", m.name, m.level)).color(crate::classic::ink(color)));
                            // The leader's right-click menu on a member, as in the original.
                            if leading && m.name != c.my_name {
                                label.context_menu(|ui| {
                                    if ui.button("Delegate leadership").clicked() {
                                        out.push(Command::Party(PartyRequest::ChangeLeader { account_id: m.account_id }));
                                        ui.close();
                                    }
                                    if ui.button("Expel").clicked() {
                                        out.push(Command::Party(PartyRequest::Expel { account_id: m.account_id, name: m.name.clone() }));
                                        ui.close();
                                    }
                                });
                            }
                            ui.label(RichText::new(&m.map).size(11.0).color(crate::classic::ink(Color32::GRAY)));
                            if leading && m.name != c.my_name && ui.small_button("Expel").clicked() {
                                out.push(Command::Party(PartyRequest::Expel { account_id: m.account_id, name: m.name.clone() }));
                            }
                        });
                        if let Some(&(hp, max)) = p.hp.get(&m.account_id) {
                            ui.add(egui::ProgressBar::new(hp as f32 / max.max(1) as f32).desired_height(6.0).fill(Color32::from_rgb(70, 190, 90)));
                        }
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.invite_name).hint_text("character name").desired_width(140.0));
                        if ui.add_enabled(!self.invite_name.trim().is_empty(), egui::Button::new("Invite")).clicked() {
                            out.push(Command::Party(PartyRequest::Invite { name: self.invite_name.trim().to_string() }));
                            self.invite_name.clear();
                        }
                    });
                    if ui.button("Leave party").clicked() {
                        out.push(Command::Party(PartyRequest::Leave));
                    }
                }
            }
            let recruit = crate::msgstr::get(1763, &[]).unwrap_or_else(|| "Recruit".into());
            let tip = crate::msgstr::get(1764, &[]).unwrap_or_else(|| "Open party recruitment window.".into());
            if ui.button(recruit).on_hover_text(tip).clicked() {
                self.open_agency = true;
            }
            if let Some(g) = &self.guild {
                ui.separator();
                ui.label(format!("Guild: {g}"));
            }
            ui.label(RichText::new("Chat: /p message for party, /g message for guild").size(11.0).color(crate::classic::ink(Color32::GRAY)));
        });
        self.show_party = open;
    }

    fn friends_window(&mut self, ctx: &egui::Context, out: &mut Vec<Command>) {
        if !self.show_friends {
            return;
        }
        let mut open = true;
        crate::classic::show(ctx, egui::Window::new("Friends").open(&mut open).resizable(false).default_pos(egui::pos2(60.0, 460.0)), "Friends", true, |ui| {
            if self.friends.is_empty() {
                ui.label(RichText::new("No friends yet").color(crate::classic::ink(Color32::GRAY)));
            }
            let mut remove = None;
            for f in &self.friends {
                ui.horizontal(|ui| {
                    let (dot, color) = if f.online { ("online", Color32::from_rgb(90, 220, 90)) } else { ("offline", Color32::GRAY) };
                    ui.label(RichText::new(dot).color(crate::classic::ink(color)));
                    ui.label(&f.name);
                    if ui.small_button("Remove").clicked() {
                        remove = Some((f.account_id, f.char_id));
                    }
                });
            }
            if let Some((account_id, char_id)) = remove {
                out.push(Command::Friend(FriendRequest::Remove { account_id, char_id }));
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.friend_name).hint_text("character name").desired_width(140.0));
                if ui.add_enabled(!self.friend_name.trim().is_empty(), egui::Button::new("Add")).clicked() {
                    out.push(Command::Friend(FriendRequest::Add { name: self.friend_name.trim().to_string() }));
                    self.friend_name.clear();
                }
            });
        });
        self.show_friends = open;
    }
}

/// An item being dragged in the storage window: (index, amount) from storage or inventory.
#[derive(Clone, Debug, Hash, PartialEq)]
enum StorageDrag {
    Take(u16, u16),
    Put(u16, u16),
}

/// A column that takes dropped storage drags: outlined while one hovers it.
fn drop_column(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui)) -> Option<std::sync::Arc<StorageDrag>> {
    let r = ui.scope(body).response;
    if r.dnd_hover_payload::<StorageDrag>().is_some() {
        ui.painter().rect_stroke(r.rect.expand(2.0), 4.0, egui::Stroke::new(1.5, Color32::from_rgb(120, 200, 255)), egui::StrokeKind::Outside);
    }
    r.dnd_release_payload::<StorageDrag>()
}

fn storage_move(m: &StorageDrag, amount: u32) -> Command {
    Command::Storage(match m {
        StorageDrag::Take(index, _) => StorageRequest::Take { index: *index, amount },
        StorageDrag::Put(index, _) => StorageRequest::Put { index: *index, amount },
    })
}

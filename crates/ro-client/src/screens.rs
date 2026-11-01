//! The screens before the game, laid out like the original client: login (with account
//! registration), server select, character select (three slots per page, preview and
//! details, delete with the 24 hour timer) and character creation (name, gender, hair
//! style and colour with a live preview).

use crate::uitex::UiSize;
use std::collections::HashMap;

use egui::{Align2, Color32, RichText, Vec2};
use ro_net::{CharInfo, CharRequest, Command, PinRequest};

use crate::preview;

/// Hair choices offered when creating a character.
const HAIR_STYLES: std::ops::RangeInclusive<u16> = 1..=27;
const HAIR_COLORS: std::ops::RangeInclusive<u16> = 0..=8;
/// Doram have fewer hair styles.
const DORAM_HAIR: u16 = 10;
const SLOTS_PER_PAGE: u8 = 3;
/// The login form puts the cursor in the ID (or password) box, as the original. Not on
/// touch screens, where a focused box raises the on-screen keyboard over the form unasked.
const AUTO_FOCUS: bool = cfg!(not(target_os = "android"));

/// The PIN code prompt, answered with the mouse on a shuffled keypad as in the original.
struct PinPad {
    /// What the server asked (HC_SECOND_PASSWD_LOGIN): 1 enter, 2/4 make, 3 change.
    state: u16,
    /// Codes already typed in this prompt: make [new]; change [old] then [old, new].
    entries: Vec<String>,
    typed: String,
    keys: [u8; 10],
    note: Option<String>,
}

impl PinPad {
    fn new(state: u16, note: Option<String>) -> Self {
        let mut keys: [u8; 10] = std::array::from_fn(|i| i as u8);
        let mut s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.subsec_nanos() as u64 | 1);
        for i in (1..keys.len()).rev() {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            keys.swap(i, (s % (i as u64 + 1)) as usize);
        }
        Self { state, entries: vec![], typed: String::new(), keys, note }
    }

    fn prompt(&self) -> &'static str {
        match (self.state, self.entries.len()) {
            (1, _) => "Enter your PIN code.",
            (3, 0) => "Your PIN code has expired. Enter your current PIN code.",
            (3, 1) | (2 | 4, 0) => "Choose a new 4-digit PIN code.",
            _ => "Enter the new PIN code again.",
        }
    }

    /// OK with four digits typed: the request to send, once the prompt has all it needs.
    fn submit(&mut self) -> Option<PinRequest> {
        let typed = std::mem::take(&mut self.typed);
        self.note = None;
        let confirming = |entries: &Vec<String>, at: usize| entries.get(at).cloned();
        match self.state {
            1 => Some(PinRequest::Check(typed)),
            3 if self.entries.len() < 2 => {
                self.entries.push(typed);
                None
            }
            3 => match confirming(&self.entries, 1) {
                Some(new) if new == typed => Some(PinRequest::Change { old: self.entries[0].clone(), new }),
                _ => {
                    self.entries.truncate(1);
                    self.note = Some("The PIN codes did not match.".into());
                    None
                }
            },
            _ if self.entries.is_empty() => {
                self.entries.push(typed);
                None
            }
            _ => match confirming(&self.entries, 0) {
                Some(new) if new == typed => Some(PinRequest::Make(new)),
                _ => {
                    self.entries.clear();
                    self.note = Some("The PIN codes did not match.".into());
                    None
                }
            },
        }
    }
}

pub enum Screen {
    Login,
    Register,
    Servers(Vec<(String, u16)>),
    Select,
    Create { slot: u8 },
    /// Waiting for the map server after picking a character.
    Loading,
}

pub struct Front {
    pub screen: Screen,
    user: String,
    pass: String,
    pub remember: bool,
    reg_user: String,
    reg_pass: String,
    reg_pass2: String,
    reg_male: bool,
    message: Option<(String, bool)>,
    busy: bool,
    chars: Vec<CharInfo>,
    slots: u8,
    selected: u8,
    page: u8,
    /// Pending deletions: char id -> when it can be confirmed.
    deleting: HashMap<u32, std::time::Instant>,
    birthdate: String,
    /// Renaming: (char id, name typed, the name the server accepted).
    renaming: Option<(u32, String, Option<String>)>,
    /// Moving the character in this slot: the next slot clicked is where it goes.
    moving: Option<u8>,
    /// Characters the server blocks, and when they can play again.
    blocked: HashMap<u32, String>,
    /// The character server wants the account's PIN code before anything else.
    pin: Option<PinPad>,
    // Character creation.
    name: String,
    male: bool,
    hair: u16,
    hair_color: u16,
    summoner: bool,
    /// Preview textures by look.
    previews: HashMap<String, Option<egui::TextureHandle>>,
    /// The original login panel art (loaded once Classic is seen).
    login_art: Option<HashMap<&'static str, egui::TextureHandle>>,
}

const LOGIN_ART: [&str; 7] = ["win_login", "btn_connect", "btn_connect_a", "btn_exit", "btn_exit_a", "chk_saveon", "chk_saveoff"];

/// What the screens need from the client.
pub struct Ctx<'a> {
    pub vfs: Option<&'a ro_vfs::Vfs>,
    pub job_title: &'a dyn Fn(i16) -> &'static str,
    pub job_sprite: &'a dyn Fn(i16) -> &'static str,
    pub accessory: &'a dyn Fn(u16) -> Option<String>,
}

/// What the player chose.
pub enum Action {
    Send(Command),
    /// Back to the login screen (the session restarts).
    Logout,
    Quit,
}

impl Front {
    pub fn new(user: String, remember: bool) -> Self {
        Self {
            screen: Screen::Login,
            user,
            pass: String::new(),
            remember,
            reg_user: String::new(),
            reg_pass: String::new(),
            reg_pass2: String::new(),
            reg_male: true,
            message: None,
            busy: false,
            chars: vec![],
            slots: 9,
            selected: 0,
            page: 0,
            deleting: HashMap::new(),
            birthdate: String::new(),
            renaming: None,
            moving: None,
            blocked: HashMap::new(),
            pin: None,
            name: String::new(),
            male: true,
            hair: 1,
            hair_color: 0,
            summoner: false,
            previews: HashMap::new(),
            login_art: None,
        }
    }

    /// The original LogOn panel: the art at its own size, the two fields over its boxes,
    /// "save" check and Connect / Exit in the bottom strip. None without the art.
    fn classic_login(&mut self, ctx: &egui::Context, c: &Ctx, at: egui::Pos2) -> Option<Option<Action>> {
        if self.login_art.is_none() {
            let vfs = c.vfs?;
            let dir = r"data\texture\유저인터페이스\login_interface";
            self.login_art = Some(LOGIN_ART.iter().filter_map(|n| Some((*n, crate::load_texture(ctx, vfs, &format!(r"{dir}\{n}.bmp"))?))).collect());
        }
        let art = self.login_art.as_ref()?;
        let bg = art.get("win_login")?;
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        let mut action = None;
        egui::Area::new(egui::Id::unique("classic_login")).fixed_pos(at - bg.ui_size() / 2.0).show(ctx, |ui| {
            let (area, _) = ui.allocate_exact_size(bg.ui_size(), egui::Sense::hover());
            let p = ui.painter().clone();
            p.image(bg.id(), area, uv, Color32::WHITE);
            let field = |y: f32| egui::Rect::from_min_size(area.min + Vec2::new(93.0, y), Vec2::new(122.0, 15.0));
            fn edit(s: &mut String, pw: bool) -> egui::TextEdit<'_> {
                egui::TextEdit::singleline(s).password(pw).frame(egui::Frame::NONE).text_color(crate::classic::TEXT).font(egui::FontId::proportional(12.0)).margin(egui::Margin::symmetric(2, 1))
            }
            let id = ui.put(field(30.0), edit(&mut self.user, false));
            if AUTO_FOCUS && self.user.is_empty() && !ui.ctx().memory(|m| m.focused().is_some()) {
                id.request_focus();
            }
            let pw = ui.put(field(62.0), edit(&mut self.pass, true));
            if AUTO_FOCUS && !self.user.is_empty() && self.pass.is_empty() && !ui.ctx().memory(|m| m.focused().is_some()) {
                pw.request_focus();
            }
            let image_button = |ui: &mut egui::Ui, name: &str, hover: &str, at: egui::Pos2, salt: &str| {
                let t = art.get(name)?;
                let r = egui::Rect::from_min_size(at, t.ui_size());
                let resp = ui.interact(r, ui.scope_id().with(salt), egui::Sense::click());
                let t = if resp.hovered() { art.get(hover).unwrap_or(t) } else { t };
                p.image(t.id(), r, uv, Color32::WHITE);
                Some(resp.clicked())
            };
            let save = if self.remember { "chk_saveon" } else { "chk_saveoff" };
            if image_button(ui, save, save, area.min + Vec2::new(8.0, 101.0), "save") == Some(true) {
                self.remember = !self.remember;
            }
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            let ok = !self.busy && !self.user.trim().is_empty() && !self.pass.is_empty();
            if (image_button(ui, "btn_connect", "btn_connect_a", area.min + Vec2::new(186.0, 96.0), "connect") == Some(true) || enter) && ok {
                self.busy = true;
                self.message = Some(("Connecting...".into(), false));
                action = Some(Action::Send(Command::Login { user: self.user.trim().to_string(), pass: self.pass.clone() }));
            }
            if image_button(ui, "btn_exit", "btn_exit_a", area.min + Vec2::new(232.0, 96.0), "exit") == Some(true) {
                action = Some(Action::Quit);
            }
            // No art for registering: a small link under the panel, with any message.
            let below = egui::Rect::from_min_size(egui::pos2(area.min.x, area.max.y + 4.0), Vec2::new(area.width(), 60.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(below), |ui| egui::Frame::NONE.fill(Color32::from_black_alpha(170)).corner_radius(3).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
                ui.set_width(area.width() - 12.0);
                if ui.link(RichText::new("Create account").size(11.0).color(Color32::WHITE)).clicked() {
                    self.message = None;
                    self.screen = Screen::Register;
                }
                if let Some((text, err)) = &self.message {
                    ui.label(RichText::new(text).color(if *err { Color32::from_rgb(255, 120, 120) } else { Color32::WHITE }));
                }
            }));
        });
        Some(action)
    }

    /// Test hook: open character creation on the first empty slot.
    pub fn open_create(&mut self) {
        let slot = (0..self.slots).find(|s| !self.chars.iter().any(|c| c.slot == *s)).unwrap_or(0);
        (self.male, self.hair, self.hair_color) = (false, 9, 4);
        self.start_create(slot);
    }

    pub fn user(&self) -> &str {
        &self.user
    }

    /// Network events for these screens. Returns true if it was one of theirs.
    pub fn event(&mut self, e: &ro_net::Event) -> bool {
        use ro_net::Event::*;
        match e {
            LoginFailed(msg) => {
                self.busy = false;
                self.message = Some((msg.clone(), true));
                // As the original: the password box empties, so retyping it can't append to
                // the hidden old one (registering fills it in for you).
                self.pass.clear();
                if !matches!(self.screen, Screen::Register) {
                    self.screen = Screen::Login;
                }
            }
            Servers(list) => {
                self.busy = false;
                self.screen = Screen::Servers(list.clone());
            }
            Characters { chars, slots } => {
                // A list sent again after a rename or slot move keeps the selection and message.
                let refresh = matches!(self.screen, Screen::Select) && !self.chars.is_empty();
                self.busy = false;
                self.renaming = None;
                self.moving = None;
                if refresh {
                    self.chars = chars.clone();
                    return true;
                }
                self.message = None;
                self.chars = chars.clone();
                self.slots = (*slots).max(SLOTS_PER_PAGE);
                self.deleting = chars
                    .iter()
                    .filter(|c| c.delete_date > 0)
                    .map(|c| (c.char_id, std::time::Instant::now() + std::time::Duration::from_secs(c.delete_date as u64)))
                    .collect();
                self.selected = self.chars.first().map_or(0, |c| c.slot);
                self.page = self.selected / SLOTS_PER_PAGE;
                self.screen = Screen::Select;
            }
            // (Characters arrive above; a test hook may ask for the create screen.)
            CharCreated(c) => {
                self.busy = false;
                self.selected = c.slot;
                self.chars.retain(|x| x.slot != c.slot);
                self.chars.push(c.clone());
                self.message = Some((format!("{} was created.", c.name), false));
                self.screen = Screen::Select;
            }
            CharCreateFailed(code) => {
                self.busy = false;
                let why = match code {
                    0 => "That name is already taken.",
                    1 => "You are not old enough to create a character.",
                    2 => "Symbols are not allowed in the name.",
                    3 => "This slot cannot be used.",
                    11 => "This slot is for premium accounts.",
                    _ => "The character could not be created.",
                };
                self.message = Some((why.into(), true));
            }
            CharDeleteStarted { char_id, result, seconds } => {
                self.busy = false;
                if *result == 1 {
                    self.deleting.insert(*char_id, std::time::Instant::now() + std::time::Duration::from_secs(*seconds as u64));
                    self.message = Some(("Deletion reserved. Confirm it once the timer ends.".into(), false));
                } else {
                    let why = match result {
                        4 => "Leave your guild before deleting this character.",
                        5 => "Leave your party before deleting this character.",
                        _ => "The character could not be deleted.",
                    };
                    self.message = Some((why.into(), true));
                }
            }
            CharDeleted { char_id, result } => {
                self.busy = false;
                if *result == 1 {
                    self.chars.retain(|c| c.char_id != *char_id);
                    self.deleting.remove(char_id);
                    self.message = Some(("Character deleted.".into(), false));
                } else {
                    let why = match result {
                        5 => "Incorrect birthdate.",
                        4 => "The deletion time has not been reached yet.",
                        _ => "The character could not be deleted.",
                    };
                    self.message = Some((why.into(), true));
                }
            }
            CharNameOk(ok) => {
                self.busy = false;
                if let Some((_, typed, accepted)) = self.renaming.as_mut() {
                    if *ok {
                        *accepted = Some(typed.trim().to_string());
                        self.message = Some(("That name is available.".into(), false));
                    } else {
                        self.message = Some(("That name cannot be used.".into(), true));
                    }
                }
            }
            CharRenamed(result) => {
                self.busy = false;
                self.renaming = None;
                let why = match result {
                    0 => "The character's name has been changed.",
                    1 => "This character's name has already been changed.",
                    4 => "Another character is using that name.",
                    5 => "Leave your guild to change this character's name.",
                    6 => "Leave your party to change this character's name.",
                    7 => "That name is too long.",
                    8 => "That name contains characters that cannot be used.",
                    9 => "Name changes are not allowed.",
                    _ => "The name could not be changed.",
                };
                self.message = Some((why.into(), *result != 0));
            }
            CharMoved { ok, .. } => {
                self.busy = false;
                self.message = Some(if *ok { ("The character has been moved.".into(), false) } else { ("The character could not be moved there.".into(), true) });
            }
            CharsBlocked(list) => self.blocked = list.iter().cloned().collect(),
            Pin(state) => {
                self.busy = false;
                let mode = self.pin.as_ref().map(|p| p.state);
                self.pin = match (*state, mode) {
                    (0, _) => None,
                    (s @ 1..=4, _) => Some(PinPad::new(s, None)),
                    // Wrong or not allowed: ask again the way we were asking.
                    (8, m) => Some(PinPad::new(m.unwrap_or(1), Some("The PIN code is incorrect.".into()))),
                    (5, m) => Some(PinPad::new(m.unwrap_or(4), Some("That PIN code cannot be used: avoid repeated or sequential digits.".into()))),
                    (s, m) => Some(PinPad::new(m.unwrap_or(1), Some(format!("The PIN code was refused ({s}).")))),
                };
            }
            CharDeleteCanceled { char_id, result } => {
                self.busy = false;
                if *result == 1 {
                    self.deleting.remove(char_id);
                    self.message = Some(("Deletion cancelled.".into(), false));
                }
            }
            _ => return false,
        }
        true
    }

    fn preview(&mut self, ctx: &egui::Context, c: &Ctx, look: preview::Look) -> Option<egui::TextureHandle> {
        let key = format!("{look:?}");
        if let Some(t) = self.previews.get(&key) {
            return t.clone();
        }
        let tex = c.vfs.and_then(|v| preview::compose(v, &look)).map(|img| ctx.load_texture(format!("preview {key}"), img, egui::TextureOptions::NEAREST));
        self.previews.insert(key, tex.clone());
        tex
    }

    fn char_look(c: &Ctx, ch: &CharInfo) -> preview::Look {
        let mut headgear = vec![];
        for v in [ch.look.head_bottom, ch.look.head_mid, ch.look.head_top] {
            if let Some(n) = (v != 0).then(|| (c.accessory)(v)).flatten() {
                if !headgear.contains(&n) {
                    headgear.push(n);
                }
            }
        }
        preview::Look { job: (c.job_sprite)(ch.job).to_string(), male: ch.sex == 1, hair: ch.head.max(1) as u16, hair_color: ch.hair_color as u16, clothes_color: ch.clothes_color.max(0) as u16, headgear }
    }

    /// Draws the current screen.
    pub fn show(&mut self, ctx: &egui::Context, c: &Ctx) -> Option<Action> {
        let mut action = None;
        // Below the title art's logo, like the original.
        let below_logo = ctx.content_rect().height() * 0.2;
        let window = |title: &str| egui::Window::new(title).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::new(0.0, below_logo));
        match &self.screen {
            Screen::Login if crate::classic::skin(ctx).is_some() && c.vfs.is_some() => {
                let at = ctx.content_rect().center() + Vec2::new(0.0, below_logo);
                if let Some(a) = self.classic_login(ctx, c, at) {
                    action = a;
                }
            }
            Screen::Login => {
                crate::classic::show(ctx, window("Login"), "Login", false, |ui| {
                    ui.set_width(300.0);
                    egui::Grid::new("login").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                        ui.label("ID");
                        let id = ui.add(egui::TextEdit::singleline(&mut self.user).desired_width(200.0));
                        if AUTO_FOCUS && self.user.is_empty() {
                            id.request_focus();
                        }
                        ui.end_row();
                        ui.label("Password");
                        let pw = ui.add(egui::TextEdit::singleline(&mut self.pass).password(true).desired_width(200.0));
                        if AUTO_FOCUS && !self.user.is_empty() && self.pass.is_empty() && !ui.ctx().memory(|m| m.focused().is_some()) {
                            pw.request_focus();
                        }
                        ui.end_row();
                    });
                    ui.checkbox(&mut self.remember, "Remember ID");
                    self.message_line(ui);
                    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.horizontal(|ui| {
                        let ok = !self.busy && !self.user.trim().is_empty() && !self.pass.is_empty();
                        if ui.add_enabled(ok, egui::Button::new("Login")).clicked() || (ok && enter) {
                            self.busy = true;
                            self.message = Some(("Connecting...".into(), false));
                            action = Some(Action::Send(Command::Login { user: self.user.trim().to_string(), pass: self.pass.clone() }));
                        }
                        if ui.button("Register").clicked() {
                            self.message = None;
                            self.screen = Screen::Register;
                        }
                        if ui.button("Exit").clicked() {
                            action = Some(Action::Quit);
                        }
                    });
                });
            }
            Screen::Register => {
                crate::classic::show(ctx, window("Create Account"), "Create Account", false, |ui| {
                    ui.set_width(300.0);
                    egui::Grid::new("register").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                        ui.label("ID");
                        ui.add(egui::TextEdit::singleline(&mut self.reg_user).desired_width(200.0).char_limit(21));
                        ui.end_row();
                        ui.label("Password");
                        ui.add(egui::TextEdit::singleline(&mut self.reg_pass).password(true).desired_width(200.0));
                        ui.end_row();
                        ui.label("Confirm");
                        ui.add(egui::TextEdit::singleline(&mut self.reg_pass2).password(true).desired_width(200.0));
                        ui.end_row();
                        ui.label("Gender");
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut self.reg_male, true, "Male");
                            ui.radio_value(&mut self.reg_male, false, "Female");
                        });
                        ui.end_row();
                    });
                    let problem = if self.reg_user.trim().len() < 4 {
                        Some("The ID needs at least 4 characters.")
                    } else if self.reg_pass.len() < 4 {
                        Some("The password needs at least 4 characters.")
                    } else if self.reg_pass != self.reg_pass2 {
                        Some("The passwords do not match.")
                    } else {
                        None
                    };
                    if let Some(p) = problem {
                        ui.label(RichText::new(p).color(Color32::GRAY).size(12.0));
                    }
                    self.message_line(ui);
                    ui.horizontal(|ui| {
                        if ui.add_enabled(problem.is_none() && !self.busy, egui::Button::new("Create account")).clicked() {
                            // rAthena creates the account when an unknown ID logs in with
                            // a _M / _F suffix (login_athena.conf: new_account: yes).
                            let suffix = if self.reg_male { "_M" } else { "_F" };
                            self.user = self.reg_user.trim().to_string();
                            self.pass = self.reg_pass.clone();
                            self.busy = true;
                            self.message = Some(("Creating account...".into(), false));
                            action = Some(Action::Send(Command::Login { user: format!("{}{suffix}", self.user), pass: self.pass.clone() }));
                        }
                        if ui.button("Back").clicked() {
                            self.message = None;
                            self.screen = Screen::Login;
                        }
                    });
                });
            }
            Screen::Servers(list) => {
                let list = list.clone();
                crate::classic::show(ctx, window("Select Server"), "Select Server", false, |ui| {
                    ui.set_width(300.0);
                    for (i, (name, users)) in list.iter().enumerate() {
                        if ui.add(egui::Button::new(format!("{name}   ({users} online)")).min_size(Vec2::new(290.0, 28.0))).clicked() {
                            self.busy = true;
                            action = Some(Action::Send(Command::SelectServer(i)));
                        }
                    }
                    if ui.button("Back").clicked() {
                        action = Some(Action::Logout);
                    }
                });
            }
            Screen::Select if self.pin.is_some() => action = self.pin_pad(ctx),
            Screen::Select => action = self.select(ctx, c),
            Screen::Create { slot } => {
                let slot = *slot;
                action = self.create(ctx, c, slot);
            }
            Screen::Loading => {
                egui::Area::new(egui::Id::unique("loading")).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
                    ui.label(RichText::new("Loading...").size(22.0).color(Color32::WHITE));
                });
            }
        }
        action
    }

    fn message_line(&self, ui: &mut egui::Ui) {
        if let Some((msg, error)) = &self.message {
            let color = if *error { Color32::from_rgb(235, 120, 100) } else { Color32::from_rgb(170, 210, 255) };
            ui.label(RichText::new(msg).color(color));
        }
    }

    fn select(&mut self, ctx: &egui::Context, c: &Ctx) -> Option<Action> {
        let mut action = None;
        let pages = self.slots.div_ceil(SLOTS_PER_PAGE).max(1);
        crate::classic::show(ctx, egui::Window::new("Select Character").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::new(0.0, 30.0)), "Select Character", false, |ui| {
            ui.horizontal(|ui| {
                if ui.add_enabled(self.page > 0, egui::Button::new("<")).clicked() {
                    self.page -= 1;
                }
                for i in 0..SLOTS_PER_PAGE {
                    let slot = self.page * SLOTS_PER_PAGE + i;
                    let ch = self.chars.iter().find(|ch| ch.slot == slot).cloned();
                    let tex = ch.as_ref().and_then(|ch| self.preview(ctx, c, Self::char_look(c, ch)));
                    let selected = self.selected == slot;
                    let frame = egui::Frame::new()
                        .fill(if selected { Color32::from_rgba_unmultiplied(90, 140, 220, 60) } else { Color32::from_black_alpha(80) })
                        .stroke(egui::Stroke::new(1.0, if selected { Color32::from_rgb(140, 190, 255) } else { Color32::from_gray(70) }))
                        .corner_radius(6)
                        .inner_margin(6);
                    let resp = frame
                        .show(ui, |ui| {
                            ui.set_width(150.0);
                            ui.set_height(190.0);
                            ui.vertical_centered(|ui| match (&ch, &tex) {
                                (Some(ch), _) => {
                                    match &tex {
                                        Some(t) => {
                                            ui.image((t.id(), Vec2::splat(140.0)));
                                        }
                                        None => {
                                            ui.add_space(140.0);
                                        }
                                    }
                                    ui.label(RichText::new(&ch.name).strong());
                                    if self.deleting.contains_key(&ch.char_id) {
                                        ui.label(RichText::new("deleting").color(Color32::from_rgb(235, 120, 100)).size(11.0));
                                    } else if self.blocked.contains_key(&ch.char_id) {
                                        ui.label(RichText::new("blocked").color(Color32::from_rgb(235, 120, 100)).size(11.0));
                                    }
                                }
                                (None, _) => {
                                    ui.add_space(80.0);
                                    ui.label(RichText::new("Empty slot").color(Color32::GRAY));
                                }
                            });
                        })
                        .response
                        .interact(egui::Sense::click());
                    if resp.clicked() {
                        match self.moving.take() {
                            Some(from) if from != slot && !self.busy => {
                                self.busy = true;
                                action = Some(Action::Send(Command::Char(CharRequest::MoveSlot { from: from as u16, to: slot as u16 })));
                            }
                            _ => self.message = None,
                        }
                        self.selected = slot;
                    }
                    if resp.double_clicked() {
                        match &ch {
                            Some(ch) if !self.deleting.contains_key(&ch.char_id) => {
                                self.screen = Screen::Loading;
                                action = Some(Action::Send(Command::SelectChar(slot)));
                            }
                            Some(_) => {}
                            None => self.start_create(slot),
                        }
                    }
                }
                if ui.add_enabled(self.page + 1 < pages, egui::Button::new(">")).clicked() {
                    self.page += 1;
                }
            });
            ui.label(RichText::new(format!("Page {} / {}", self.page + 1, pages)).color(Color32::GRAY).size(11.5));
            ui.separator();
            // Details and actions for the selected slot.
            let ch = self.chars.iter().find(|ch| ch.slot == self.selected).cloned();
            match ch {
                Some(ch) => {
                    egui::Grid::new("char details").num_columns(4).spacing([16.0, 4.0]).show(ui, |ui| {
                        let k = |ui: &mut egui::Ui, s: &str| {
                            ui.label(RichText::new(s).color(Color32::GRAY));
                        };
                        k(ui, "Name");
                        ui.label(&ch.name);
                        k(ui, "Job");
                        ui.label((c.job_title)(ch.job));
                        ui.end_row();
                        k(ui, "Base Lv");
                        ui.label(ch.base_level.to_string());
                        k(ui, "Job Lv");
                        ui.label(ch.job_level.to_string());
                        ui.end_row();
                        k(ui, "HP");
                        ui.label(format!("{} / {}", ch.hp, ch.max_hp));
                        k(ui, "SP");
                        ui.label(format!("{} / {}", ch.sp, ch.max_sp));
                        ui.end_row();
                        k(ui, "Zeny");
                        ui.label(ch.zeny.to_string());
                        k(ui, "Map");
                        ui.label(ch.map.trim_end_matches(".gat"));
                        ui.end_row();
                    });
                    ui.add_space(4.0);
                    self.message_line(ui);
                    match self.deleting.get(&ch.char_id).copied() {
                        Some(when) => {
                            let left = when.saturating_duration_since(std::time::Instant::now()).as_secs();
                            if left > 0 {
                                ui.label(format!("Can be deleted in {}h {:02}m {:02}s.", left / 3600, left / 60 % 60, left % 60));
                                ctx.request_repaint_after(std::time::Duration::from_secs(1));
                            } else {
                                ui.horizontal(|ui| {
                                    ui.label("Birthdate (YYMMDD)");
                                    ui.add(egui::TextEdit::singleline(&mut self.birthdate).desired_width(80.0).char_limit(6));
                                    if ui.add_enabled(self.birthdate.len() == 6 && !self.busy, egui::Button::new("Delete now")).clicked() {
                                        self.busy = true;
                                        action = Some(Action::Send(Command::Char(CharRequest::Delete { char_id: ch.char_id, birthdate: self.birthdate.clone() })));
                                    }
                                });
                            }
                            if ui.button("Cancel deletion").clicked() {
                                action = Some(Action::Send(Command::Char(CharRequest::CancelDelete(ch.char_id))));
                            }
                        }
                        None => {
                            ui.horizontal(|ui| {
                                if ui.add(egui::Button::new(RichText::new("Start").strong()).min_size(Vec2::new(100.0, 28.0))).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    self.screen = Screen::Loading;
                                    action = Some(Action::Send(Command::SelectChar(ch.slot)));
                                }
                                if ui.button("Delete").clicked() && !self.busy {
                                    self.busy = true;
                                    action = Some(Action::Send(Command::Char(CharRequest::ReserveDelete(ch.char_id))));
                                }
                                // Shown only when the server allows them (rename count, char_move_enabled).
                                if ch.can_rename && ui.button("Change Name").clicked() {
                                    self.renaming = Some((ch.char_id, String::new(), None));
                                    self.message = None;
                                }
                                if ch.moves > 0 && ui.button("Move Slot").on_hover_text(format!("{} move(s) left", ch.moves)).clicked() {
                                    self.moving = Some(ch.slot);
                                    self.message = Some(("Click the slot to move this character to.".into(), false));
                                }
                                if ui.button("Logout").clicked() {
                                    action = Some(Action::Logout);
                                }
                            });
                            if let Some(until) = self.blocked.get(&ch.char_id) {
                                ui.label(RichText::new(format!("This character is blocked until {until}.")).color(Color32::from_rgb(235, 120, 100)));
                            }
                            if let Some((id, typed, accepted)) = self.renaming.as_mut().filter(|r| r.0 == ch.char_id) {
                                ui.horizontal(|ui| {
                                    ui.label("New name");
                                    if ui.add(egui::TextEdit::singleline(typed).desired_width(160.0).char_limit(23)).changed() {
                                        *accepted = None;
                                    }
                                    match accepted.clone() {
                                        None => {
                                            if ui.add_enabled(typed.trim().len() >= 4 && !self.busy, egui::Button::new("Check")).clicked() {
                                                self.busy = true;
                                                action = Some(Action::Send(Command::Char(CharRequest::CheckName { account_id: 0, char_id: *id, name: typed.trim().to_string() })));
                                            }
                                        }
                                        Some(name) => {
                                            if ui.add_enabled(!self.busy, egui::Button::new(format!("Rename to {name}"))).on_hover_text("A character can only be renamed once.").clicked() {
                                                self.busy = true;
                                                action = Some(Action::Send(Command::Char(CharRequest::Rename { char_id: *id, name })));
                                            }
                                        }
                                    }
                                });
                            }
                        }
                    }
                }
                None => {
                    self.message_line(ui);
                    ui.horizontal(|ui| {
                        if ui.add(egui::Button::new(RichText::new("Make Character").strong()).min_size(Vec2::new(140.0, 28.0))).clicked() {
                            self.start_create(self.selected);
                        }
                        if ui.button("Logout").clicked() {
                            action = Some(Action::Logout);
                        }
                    });
                }
            }
        });
        action
    }

    /// The PIN code window, shown over character select until the server lets us through.
    fn pin_pad(&mut self, ctx: &egui::Context) -> Option<Action> {
        let mut action = None;
        let busy = self.busy;
        let Some(pad) = self.pin.as_mut() else { return None };
        let mut send = None;
        crate::classic::show(ctx, egui::Window::new("PIN Code").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO), "PIN Code", false, |ui| {
            ui.label(pad.prompt());
            let dots: String = (0..4).map(|i| if i < pad.typed.len() { "* " } else { "_ " }).collect();
            ui.label(RichText::new(dots).size(22.0).color(Color32::from_rgb(170, 210, 255)));
            if let Some(note) = &pad.note {
                ui.label(RichText::new(note).color(Color32::from_rgb(235, 120, 100)));
            }
            let key = |ui: &mut egui::Ui, text: &str| ui.add_enabled(!busy, egui::Button::new(RichText::new(text).size(18.0)).min_size(Vec2::new(56.0, 40.0))).clicked();
            egui::Grid::new("pin keys").num_columns(3).spacing([6.0, 6.0]).show(ui, |ui| {
                for row in 0..3 {
                    for col in 0..3 {
                        let d = pad.keys[row * 3 + col];
                        if key(ui, &d.to_string()) && pad.typed.len() < 4 {
                            pad.typed.push(char::from(b'0' + d));
                        }
                    }
                    ui.end_row();
                }
                if key(ui, "Clear") {
                    pad.typed.clear();
                }
                let d = pad.keys[9];
                if key(ui, &d.to_string()) && pad.typed.len() < 4 {
                    pad.typed.push(char::from(b'0' + d));
                }
                if key(ui, "OK") && pad.typed.len() == 4 {
                    send = pad.submit();
                }
                ui.end_row();
            });
            ui.add_space(4.0);
            if ui.button("Logout").clicked() {
                action = Some(Action::Logout);
            }
        });
        if let Some(req) = send {
            self.busy = true;
            action = Some(Action::Send(Command::Pin(req)));
        }
        action
    }

    /// The PIN code is still wanted (scripted runs wait before picking a character).
    pub fn pin_pending(&self) -> bool {
        self.pin.is_some()
    }

    fn start_create(&mut self, slot: u8) {
        self.name.clear();
        self.message = None;
        self.screen = Screen::Create { slot };
    }

    fn create(&mut self, ctx: &egui::Context, c: &Ctx, slot: u8) -> Option<Action> {
        let mut action = None;
        let job = if self.summoner { "summoner" } else { "초보자" };
        let look = preview::Look { job: job.into(), male: self.male, hair: self.hair, hair_color: self.hair_color, clothes_color: 0, headgear: vec![] };
        let tex = self.preview(ctx, c, look);
        crate::classic::show(ctx, egui::Window::new("Make Character").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::new(0.0, 30.0)), "Make Character", false, |ui| {
            ui.horizontal(|ui| {
                egui::Frame::new().fill(Color32::from_black_alpha(80)).corner_radius(6).inner_margin(8).show(ui, |ui| {
                    ui.set_width(160.0);
                    match &tex {
                        Some(t) => {
                            ui.image((t.id(), Vec2::splat(160.0)));
                        }
                        None => {
                            ui.add_space(160.0);
                        }
                    }
                });
                ui.vertical(|ui| {
                    egui::Grid::new("make char").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
                        ui.label("Name");
                        ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(170.0).char_limit(23));
                        ui.end_row();
                        ui.label("Gender");
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut self.male, true, "Male");
                            ui.radio_value(&mut self.male, false, "Female");
                        });
                        ui.end_row();
                        ui.label("Race");
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut self.summoner, false, "Human");
                            ui.radio_value(&mut self.summoner, true, "Doram");
                            if self.summoner {
                                self.hair = self.hair.min(DORAM_HAIR);
                            }
                        });
                        ui.end_row();
                        ui.label("Hair style");
                        ui.horizontal(|ui| {
                            if ui.button("<").clicked() {
                                let max = if self.summoner { DORAM_HAIR } else { *HAIR_STYLES.end() };
                                self.hair = if self.hair <= *HAIR_STYLES.start() { max } else { self.hair - 1 };
                            }
                            ui.label(format!("{:>2}", self.hair));
                            if ui.button(">").clicked() {
                                let max = if self.summoner { DORAM_HAIR } else { *HAIR_STYLES.end() };
                                self.hair = if self.hair >= max { *HAIR_STYLES.start() } else { self.hair + 1 };
                            }
                        });
                        ui.end_row();
                        ui.label("Hair color");
                        ui.horizontal(|ui| {
                            if ui.button("<").clicked() {
                                self.hair_color = if self.hair_color <= *HAIR_COLORS.start() { *HAIR_COLORS.end() } else { self.hair_color - 1 };
                            }
                            ui.label(format!("{:>2}", self.hair_color));
                            if ui.button(">").clicked() {
                                self.hair_color = if self.hair_color >= *HAIR_COLORS.end() { *HAIR_COLORS.start() } else { self.hair_color + 1 };
                            }
                        });
                        ui.end_row();
                    });
                    ui.add_space(6.0);
                    self.message_line(ui);
                    ui.horizontal(|ui| {
                        let ok = self.name.trim().chars().count() >= 4 && !self.busy;
                        if ui.add_enabled(ok, egui::Button::new(RichText::new("Create").strong())).clicked() {
                            self.busy = true;
                            action = Some(Action::Send(Command::Char(CharRequest::Make {
                                name: self.name.trim().to_string(),
                                slot,
                                hair_color: self.hair_color,
                                hair_style: self.hair,
                                job: if self.summoner { 4218 } else { 0 },
                                sex: self.male as u8,
                            })));
                        }
                        if ui.button("Cancel").clicked() {
                            self.message = None;
                            self.screen = Screen::Select;
                        }
                    });
                    if self.name.trim().chars().count() < 4 {
                        ui.label(RichText::new("Names need at least 4 characters.").color(Color32::GRAY).size(11.5));
                    }
                });
            });
        });
        action
    }
}

//! Keyboard, mouse, touch and controller input. Part of `App` (app.rs).

use super::*;

impl App {
    /// Turns a window action into a server request.
    /// A key pressed over the game (not taken by the UI).
    pub(crate) fn key_pressed(&mut self, logical_key: winit::keyboard::Key) {
        use winit::keyboard::{Key, NamedKey};
        // The keys Shortcut Settings binds (shortcut bar, windows, macros, game toggles).
        if let Some(does) = keybinds::name(&logical_key, self.ctrl, self.alt, self.shift).and_then(|n| keybinds::lookup(&self.settings.keys, &n)) {
            self.run_key(does, logical_key);
            return;
        }
        match &logical_key {
            // Esc cancels aiming; otherwise it opens the game menu, as in the original.
            Key::Named(NamedKey::Escape) => {
                if self.aiming.take().is_none() && self.dialog.is_none() && !self.dead {
                    self.esc_menu = !self.esc_menu;
                }
            }
            // Unbound Alt and Ctrl combinations don't type.
            Key::Character(_) if self.alt || self.ctrl => {}
            // Battle mode: Space opens the chat, letters use the shortcut rows.
            Key::Named(NamedKey::Space) if self.settings.battle_mode && self.settings.battle_space_chat => self.hud.focus_chat(),
            Key::Named(NamedKey::Space) if self.settings.battle_mode => {
                if let Some(slot) = battle_slot(" ", self.hotkey_row, &self.settings.battle_keys) {
                    self.press_hotkey(slot, logical_key.clone());
                }
            }
            Key::Character(c) if self.settings.battle_mode => {
                if let Some(slot) = battle_slot(c, self.hotkey_row, &self.settings.battle_keys) {
                    self.press_hotkey(slot, logical_key.clone());
                }
            }
            // Otherwise typing goes to the chat, as in the original.
            Key::Character(c) => self.hud.type_into_chat(c),
            _ => {}
        }
    }

    /// What a bound key does (keybinds; the defaults are the original's keys: F1-F9 and F12
    /// for the shortcut bar, Alt+letters for windows, Alt+1..0 for macros, Insert to sit,
    /// Ctrl+Tab for the minimap, Scroll Lock for a screenshot; F10 and F11 are ours).
    pub(crate) fn run_key(&mut self, does: keybinds::Does, key: winit::keyboard::Key) {
        use keybinds::Does;
        match does {
            Does::Slot(col) => self.press_hotkey(self.hotkey_row as usize * HOTKEYS_PER_ROW + col, key),
            Does::BarRows => {
                self.settings.hotkey_rows = (self.settings.hotkey_rows + 1) % (HOTKEY_ROWS + 1);
                self.settings.save();
            }
            Does::Window(code) => self.alt_key(code),
            Does::Macro(digit) => self.alt_key(&digit.to_string()),
            Does::Sit => {
                let sitting = self.player.as_ref().is_some_and(|p| p.sitting);
                self.send(ro_net::Command::Sit(!sitting));
            }
            Does::Minimap => {
                self.settings.minimap = !self.settings.minimap;
                self.settings.save();
            }
            Does::Perf => {
                self.settings.perf_overlay = !self.settings.perf_overlay;
                self.settings.save();
            }
            Does::Fullscreen => {
                self.settings.fullscreen = !self.settings.fullscreen;
                self.settings.save();
            }
            Does::Screenshot => self.screenshot = true,
        }
    }

    /// A shortcut key pressed: uses its slot. Rolling Cutter repeats for as long as the key
    /// is held, as in the original.
    pub(crate) fn press_hotkey(&mut self, slot: usize, held: winit::keyboard::Key) {
        let key = self.hotkeys.get(slot).copied().filter(|h| h.skill && h.id as u16 == HOLD_SKILL);
        let learned = key.and_then(|h| self.skills.iter().find(|s| s.id == HOLD_SKILL).map(|s| (h.count.max(1) as u16).min(s.level.max(1))));
        match learned {
            Some(level) if self.held_skill.is_none() => {
                self.send(ro_net::Command::Stat(ro_net::StatRequest::HoldSkill { skill: HOLD_SKILL, level, target: self.account_id }));
                self.held_skill = Some((HOLD_SKILL, held));
            }
            Some(_) => {}
            None => self.queued_action = Some(windows::UiAction::UseHotkey(slot)),
        }
    }

    /// Lets go of a held skill (CZ_USE_SKILL_END).
    pub(crate) fn release_held_skill(&mut self) {
        if let Some((skill, _)) = self.held_skill.take() {
            self.send(ro_net::Command::Stat(ro_net::StatRequest::ReleaseSkill(skill)));
        }
    }

    /// Leaves the game, asking the map server first as the original does: it refuses within
    /// 10 seconds of combat (prevent_logout).
    pub(crate) fn request_exit(&mut self) {
        if self.net.is_some() {
            self.queued_commands.push(ro_net::Command::Quit);
            self.quit_asked = Some(std::time::Instant::now());
        } else {
            self.quit = true;
        }
    }

    /// The player or a unit by id.
    /// Left button held on the ground: re-aim the walk at the cursor every few frames, as
    /// the original client does when you hold the button and point.
    pub(crate) fn steer_to_cursor(&mut self, dt: f32) {
        if let Some((off, left)) = self.hold_walk.as_mut() {
            *left -= dt;
            let (off, done) = (*off, *left <= 0.0);
            if done {
                self.hold_walk = None;
                return;
            }
            self.mouse_walk_ms -= dt;
            if self.mouse_walk_ms <= 0.0 {
                self.mouse_walk_ms = 150.0;
                if let Some(c) = self.player.as_ref().map(|p| p.walker.cell) {
                    self.steer_toward(((c.0 + off.0).max(0) as u16, (c.1 + off.1).max(0) as u16));
                }
            }
            return;
        }
        if !self.mouse_walk {
            return;
        }
        if !self.left_down || self.dead || self.aiming.is_some() {
            self.mouse_walk = false;
            return;
        }
        self.mouse_walk_ms -= dt;
        if self.mouse_walk_ms > 0.0 {
            return;
        }
        self.mouse_walk_ms = 150.0;
        if self.egui_ctx.is_pointer_over_egui() {
            return;
        }
        if let Some(cell) = self.pick() {
            self.steer_toward(cell);
        }
    }

    /// Hold-to-walk toward a cell. Re-planning every time the aimed-at cell moves (it moves
    /// with us, the camera following) makes each new path start with the same step, so a
    /// heading between straight and diagonal drifts to the diagonal, and every re-plan risks
    /// a stutter. So the walk is re-aimed only when it is nearly done or when the aim turns
    /// (the mouse moves, the camera turns): paths run their length, straight and diagonal
    /// steps both, and the heading follows the cursor.
    pub(crate) fn steer_toward(&mut self, cell: (u16, u16)) {
        let Some(p) = self.player.as_ref() else { return };
        if self.walk_goal.as_ref().is_some_and(|g| g.cell == cell) {
            return;
        }
        let here = p.walker.cell;
        let goal = self.walk_goal.as_ref().map_or_else(|| p.walker.goal_or_cell(), |g| (g.cell.0 as i32, g.cell.1 as i32));
        let aim = glam::Vec2::new((cell.0 as i32 - here.0) as f32, (cell.1 as i32 - here.1) as f32);
        let going = glam::Vec2::new((goal.0 - here.0) as f32, (goal.1 - here.1) as f32);
        let turned = aim.length() > 0.5 && going.length() > 0.5 && aim.angle_to(going).abs() > 25f32.to_radians();
        if !p.walker.is_walking() || p.walker.remaining() <= 2 || turned {
            self.request_walk(cell);
        }
    }

    /// Indoors: recompute which room we are in when we step outside the current one (map
    /// entry, warps within the map) and hand the renderer its mask.
    pub(crate) fn update_room(&mut self) {
        let (Some(w), Some(p)) = (self.world.as_mut(), self.player.as_ref()) else { return };
        if !w.indoor() || (w.room.is_some() && w.shows_cell(p.walker.cell.0, p.walker.cell.1)) {
            return;
        }
        let Some(room) = w.room_of(p.walker.cell) else { return };
        if let (Some(r), Some(g)) = (self.renderer.as_mut(), w.gat.as_ref()) {
            r.set_room_mask(Some((g.width, g.height, &room)));
        }
        w.room = Some(room);
    }

    /// Touch screens (Android): one finger is the left mouse button on the world (tap to walk,
    /// talk or attack; hold to steer), two fingers turn the camera and pinch to zoom. Touches
    /// the UI takes never get here.
    pub(crate) fn touch_input(&mut self, el: &ActiveEventLoop, id: WindowId, t: winit::event::Touch) {
        use winit::event::TouchPhase;
        let pos = (t.location.x, t.location.y);
        let device_id = winit::event::DeviceId::dummy();
        let button = |pressed: bool| WindowEvent::MouseInput { device_id, state: if pressed { ElementState::Pressed } else { ElementState::Released }, button: MouseButton::Left };
        let moved = WindowEvent::CursorMoved { device_id, position: t.location };
        let before = self.fingers.clone();
        match t.phase {
            TouchPhase::Started => {
                self.fingers.push((t.id, pos));
                match self.fingers.len() {
                    1 => {
                        self.window_event(el, id, moved);
                        self.window_event(el, id, button(true));
                    }
                    // A second finger: a camera gesture, so the first stops steering.
                    2 => self.window_event(el, id, button(false)),
                    _ => {}
                }
            }
            TouchPhase::Moved => {
                if let Some(f) = self.fingers.iter_mut().find(|f| f.0 == t.id) {
                    f.1 = pos;
                }
                match (&before[..], &self.fingers[..]) {
                    ([one], _) if one.0 == t.id => self.window_event(el, id, moved),
                    ([a0, b0], [a1, b1]) => {
                        let mid = |a: (f64, f64), b: (f64, f64)| ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
                        let span = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).hypot(a.1 - b.1);
                        let (m0, m1) = (mid(a0.1, b0.1), mid(a1.1, b1.1));
                        // Arranging the interface: the pinch resizes the selected element.
                        if self.arrange.open {
                            let (s0, s1) = (span(a0.1, b0.1), span(a1.1, b1.1));
                            if s0 > 1.0 {
                                self.arrange.pinch *= (s1 / s0) as f32;
                            }
                        } else if let Some(r) = self.renderer.as_mut() {
                            let s = 0.005 / r.scale_factor as f32;
                            r.camera.rotate((m1.0 - m0.0) as f32 * s, (m1.1 - m0.1) as f32 * s);
                            r.camera.zoom(((span(a1.1, b1.1) - span(a0.1, b0.1)) / (60.0 * r.scale_factor)) as f32);
                        }
                    }
                    _ => {}
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                if !self.fingers.iter().any(|f| f.0 == t.id) {
                    return;
                }
                let last = self.fingers.len() == 1;
                self.fingers.retain(|f| f.0 != t.id);
                if last {
                    self.window_event(el, id, moved);
                    self.window_event(el, id, button(false));
                }
            }
        }
    }

    /// Right button: held and dragged it turns the camera; clicked on another player it
    /// opens their menu, as in the original.
    pub(crate) fn right_button(&mut self, pressed: bool) {
        let released = self.rotating && !pressed;
        self.rotating = pressed;
        if pressed {
            self.right_drag = 0.0;
        }
        if released && self.right_drag < 6.0 {
            if let Some(id) = self.pick_unit().filter(|id| self.units.get(id).is_some_and(|m| m.kind == 0)) {
                self.open_player_menu(id);
            }
        }
    }

    /// Opens the menu for another player (trade, party, friend; GM actions) at the cursor.
    pub(crate) fn open_player_menu(&mut self, id: u32) {
        let name = self.units.get(&id).map(|m| m.name.clone()).unwrap_or_default();
        let ppp = self.egui_ctx.pixels_per_point();
        let pos = self.cursor.map_or(egui::Pos2::ZERO, |(x, y)| egui::pos2(x as f32 / ppp, y as f32 / ppp));
        self.player_menu = Some((id, name, pos));
    }

    /// The cursor for what is under the mouse, as the original client shows it.
    pub(crate) fn hover_cursor(&self) -> usize {
        use hud::cursor::*;
        if self.rotating {
            return ROTATE;
        }
        if self.aiming.is_some() || self.capturing {
            return TARGET;
        }
        if self.cursor.is_none() || self.egui_ctx.is_pointer_over_egui() {
            return ARROW;
        }
        match self.pick_unit().and_then(|id| self.units.get(&id)).map(|m| m.kind) {
            Some(5) => ATTACK,
            Some(0) if self.pick_unit().is_some_and(|id| self.is_enemy_player(id)) => ATTACK,
            Some(k) if is_npc(k) => TALK,
            _ if self.pick_floor_item().is_some() => PICK_UP,
            _ => ARROW,
        }
    }

    /// Units the controller can target, nearest first: monsters and named NPCs in reach, or
    /// other players while aiming a support skill.
    pub(crate) fn pad_candidates(&self, players: bool) -> Vec<u32> {
        let Some(me) = self.player.as_ref().map(|p| p.walker.cell) else { return vec![] };
        let mut v: Vec<(i32, u32)> = self
            .units
            .iter()
            .filter(|(_, m)| m.dying.is_none() && m.actor.alpha > 0.01 && if players { m.kind == 0 } else { m.kind == 5 || (is_npc(m.kind) && !m.name.is_empty()) })
            .map(|(id, m)| ((m.walker.cell.0 - me.0).pow(2) + (m.walker.cell.1 - me.1).pow(2), *id))
            .filter(|(d, _)| *d <= PAD_REACH * PAD_REACH)
            .collect();
        v.sort();
        v.into_iter().map(|(_, id)| id).collect()
    }

    /// Plays a controller feedback cue, when a controller is being played with.
    pub(crate) fn feel(&mut self, c: haptics::Cue) {
        if self.pad.in_use() && self.settings.vibration > 0.0 {
            tracing::debug!(?c, "vibration cue");
            self.haptics.play(c);
        }
    }

    /// Feedback for an attack: a thud when we are hit, as hard as the share of our HP it
    /// took; a sharp tick when we land a critical.
    pub(crate) fn feel_act(&mut self, act: &ro_net::Act) {
        let me = self.account_id;
        if act.target == me && act.src != me && act.damage > 0 {
            let max = self.hud.status.max_hp.max(1) as f32;
            self.feel(haptics::Cue::Hit((act.damage as f32 + act.left_damage.max(0) as f32) / max));
        } else if act.src == me && act.target != me && act.kind == 10 && act.damage > 0 {
            self.feel(haptics::Cue::Crit);
        }
    }

    /// Feedback for a non-damaging skill: ours on ourselves or a player swells, on a monster
    /// zaps; another player's on us swells gently.
    pub(crate) fn feel_skill_used(&mut self, src: u32, target: u32) {
        let me = self.account_id;
        let kind = |app: &Self, id: u32| app.units.get(&id).map(|m| m.kind);
        if src == me {
            self.haptics.end_charge();
            let cue = if target == me || target == 0 || kind(self, target) == Some(0) { haptics::Cue::Support } else { haptics::Cue::Hex };
            self.feel(cue);
        } else if target == me && kind(self, src) == Some(0) {
            self.feel(haptics::Cue::Blessed);
        }
    }

    /// Controller feedback each frame: finds a Sony or Nintendo pad to open directly, beats
    /// the heart at low HP, and sends the mixed rumble (and the light bar's HP colour) to the
    /// pad: as audio to a USB DualSense's actuators, as HID reports to a Sony or Nintendo
    /// pad, else through gilrs.
    pub(crate) fn feedback_tick(&mut self, dt: f32) {
        // Follow the pad in use: another one picked up closes this one.
        let want = self.pad.active_ids();
        if self.hid.as_ref().is_some_and(|h| !h.alive() || want.is_some_and(|w| w != h.ids)) {
            self.hid = None;
            self.audio_haptics = None;
            self.hid_retry_ms = 0.0;
        }
        if self.hid.is_none() && self.pad.connected() {
            self.hid_retry_ms -= dt;
            if self.hid_retry_ms <= 0.0 {
                self.hid_retry_ms = 3000.0;
                self.hid = hidpad::HidPad::open(want, self.pad.detected);
                if self.settings.dualsense_hd && self.hid.as_ref().is_some_and(|h| h.kind == hidpad::Kind::DualSense) {
                    self.audio_haptics = haptics::AudioHaptics::open();
                }
            }
        }
        // The DualSense switched between HD haptics and classic rumble.
        if self.haptics_hd != self.settings.dualsense_hd {
            self.haptics_hd = self.settings.dualsense_hd;
            self.audio_haptics = None;
            if self.haptics_hd && self.hid.as_ref().is_some_and(|h| h.kind == hidpad::Kind::DualSense) {
                self.audio_haptics = haptics::AudioHaptics::open();
            }
        }
        let (hp, max) = (self.hud.status.hp, self.hud.status.max_hp);
        let share = if max > 0 { hp as f32 / max as f32 } else { 1.0 };
        if !self.dead && share < 0.25 {
            self.heartbeat_ms -= dt;
            if self.heartbeat_ms <= 0.0 {
                self.heartbeat_ms = if share < 0.1 { 650.0 } else { 1000.0 };
                self.feel(haptics::Cue::Heartbeat);
            }
        } else {
            self.heartbeat_ms = 0.0;
        }
        if std::mem::take(&mut self.settings.vibration_test) {
            tracing::info!(connected = self.pad.connected(), hid = ?self.hid.as_ref().map(|h| h.kind), audio = self.audio_haptics.is_some(), "vibration test");
            self.haptics.play(haptics::Cue::Hit(0.6));
            self.haptics.play(haptics::Cue::Crit);
            self.haptics.play(haptics::Cue::LevelUp);
            self.haptics_test_ms = 800.0;
        }
        self.haptics_test_ms -= dt;
        let (mut low, mut high) = self.haptics.frame(dt, self.settings.vibration.max(if self.haptics_test_ms > 0.0 { 0.5 } else { 0.0 }));
        if !self.pad.in_use() && self.haptics_test_ms <= 0.0 {
            (low.amp, high.amp) = (0.0, 0.0);
        }
        // The light bar shows HP: green, amber below half, red below a quarter, pulsing near death.
        let light = (self.settings.light_bar && max > 0).then(|| {
            let base: [f32; 3] = if self.dead {
                [0.35, 0.0, 0.0]
            } else if share > 0.5 {
                [0.15, 0.85, 0.3]
            } else if share > 0.25 {
                [0.95, 0.6, 0.1]
            } else {
                [0.95, 0.12, 0.12]
            };
            let secs = std::time::UNIX_EPOCH.elapsed().map_or(0.0, |d| (d.as_millis() % 100_000) as f32 / 1000.0);
            let pulse = if !self.dead && share < 0.1 { 0.35 + 0.65 * (secs * 3.0).sin().abs() } else { 1.0 };
            base.map(|c| (c * pulse * 255.0) as u8)
        });
        let route = match (&self.audio_haptics, &self.hid) {
            (Some(_), _) => "DualSense haptics (audio)",
            (None, Some(_)) => "controller HID rumble",
            (None, None) => "gilrs rumble",
        };
        if (low.amp > 0.0 || high.amp > 0.0) && route != self.haptics_route {
            tracing::info!(route, "vibration");
            self.haptics_route = route;
        }
        let audio_haptics = self.audio_haptics.is_some();
        if let Some(a) = &self.audio_haptics {
            a.set(low, high);
            (low.amp, high.amp) = (0.0, 0.0);
        }
        match self.hid.as_mut() {
            Some(h) => h.set(hidpad::Output { low, high, light, audio_haptics }),
            None => self.pad.set_rumble(low.amp, high.amp),
        }
    }

    /// The PlayStation touchpad: one finger moves the pointer, pressing clicks (with two
    /// fingers, right-clicks), a two-finger swipe across turns the shortcut row on show and a
    /// two-finger slide up or down zooms.
    pub(crate) fn touchpad_tick(&mut self, el: &ActiveEventLoop) {
        let Some(t) = self.hid.as_ref().filter(|_| self.settings.touchpad).map(|h| h.touch()) else { return };
        let prev = std::mem::replace(&mut self.touch_prev, t);
        let Some(window) = self.window.clone() else { return };
        let size = window.inner_size();
        let down: Vec<hidpad::Finger> = t.fingers.iter().flatten().copied().collect();
        let held = |f: &hidpad::Finger| prev.fingers.iter().flatten().find(|p| p.id == f.id).copied();
        match down.as_slice() {
            [f] => {
                self.touch_slide = glam::Vec2::ZERO;
                if let Some(p) = held(f) {
                    let d = glam::Vec2::new(f.x - p.x, f.y - p.y);
                    // A full stroke crosses the window; faster strokes go further.
                    let gain = (0.9 + 6.0 * d.length().min(0.05)) * self.settings.pad_tuning.touch_speed;
                    let (x, y) = self.touch_pos.or(self.cursor).unwrap_or((size.width as f64 / 2.0, size.height as f64 / 2.0));
                    let pos = ((x + (d.x * gain * size.width as f32) as f64).clamp(0.0, size.width as f64 - 1.0), (y + (d.y * gain * size.width as f32 * 0.6) as f64).clamp(0.0, size.height as f64 - 1.0));
                    if pos != (x, y) {
                        self.touch_pos = Some(pos);
                        if window.set_cursor_position(winit::dpi::PhysicalPosition::new(pos.0, pos.1)).is_err() {
                            // Where the pointer can't be moved (Wayland), move ours alone.
                            self.touch_feeding = true;
                            self.window_event(el, window.id(), WindowEvent::CursorMoved { device_id: winit::event::DeviceId::dummy(), position: winit::dpi::PhysicalPosition::new(pos.0, pos.1) });
                            self.touch_feeding = false;
                        }
                    }
                }
            }
            [a, b] => {
                if let (Some(pa), Some(pb)) = (held(a), held(b)) {
                    self.touch_slide += glam::Vec2::new((a.x + b.x - pa.x - pb.x) / 2.0, (a.y + b.y - pa.y - pb.y) / 2.0);
                    if self.touch_slide.x.abs() > 0.3 {
                        let n = HOTKEY_ROWS;
                        let row = if self.touch_slide.x > 0.0 { (self.hotkey_row + 1) % n } else { (self.hotkey_row + n - 1) % n };
                        self.queued_action = Some(windows::UiAction::HotkeyRow(row));
                        self.touch_slide = glam::Vec2::ZERO;
                    } else if self.touch_slide.y.abs() > 0.06 {
                        if let Some(r) = self.renderer.as_mut() {
                            r.camera.zoom(-self.touch_slide.y * 12.0);
                        }
                        self.touch_slide.y = 0.0;
                    }
                }
            }
            _ => {
                self.touch_slide = glam::Vec2::ZERO;
                self.touch_pos = None;
            }
        }
        let press = |app: &mut Self, button: MouseButton, state: ElementState| {
            app.touch_feeding = true;
            app.window_event(el, window.id(), WindowEvent::MouseInput { device_id: winit::event::DeviceId::dummy(), state, button });
            app.touch_feeding = false;
        };
        if t.click && !prev.click {
            let button = if down.len() >= 2 { MouseButton::Right } else { MouseButton::Left };
            self.touch_button = Some(button);
            press(self, button, ElementState::Pressed);
        } else if !t.click && prev.click {
            if let Some(button) = self.touch_button.take() {
                press(self, button, ElementState::Released);
            }
        }
    }

    /// Keys, clicks, the wheel or a real stroke of the mouse put the controller down: its button
    /// hints give way to the keyboard's until it is touched again. The touchpad steering the
    /// pointer is the controller's own input, and so is resting mouse jitter.
    pub(crate) fn note_keyboard_mouse(&mut self, event: &WindowEvent) {
        if !self.pad.in_use() || self.touch_feeding {
            self.mouse_travel = 0.0;
            return;
        }
        let put_down = match event {
            WindowEvent::KeyboardInput { event: winit::event::KeyEvent { state: ElementState::Pressed, .. }, .. }
            | WindowEvent::MouseInput { state: ElementState::Pressed, .. }
            | WindowEvent::MouseWheel { .. } => true,
            WindowEvent::CursorMoved { position, .. } if self.touch_pos.is_none() => {
                if let Some((x, y)) = self.cursor {
                    self.mouse_travel += (position.x - x).hypot(position.y - y);
                }
                self.mouse_travel > 48.0
            }
            _ => false,
        };
        if put_down {
            self.pad.put_down();
            self.mouse_travel = 0.0;
        }
    }

    /// Battle mode on a controller: a shortcut slot fires at once, without aiming. Attacks go
    /// on the target (the nearest monster when there is none), support skills on the targeted
    /// player or ourselves, ground skills under the target or at our feet.
    pub(crate) fn pad_quick_slot(&mut self, slot: usize) {
        let Some(key) = self.hotkeys.get(slot).copied().filter(|k| k.id != 0) else { return };
        let skill = key.id as u16;
        let Some(s) = self.skills.iter().find(|s| key.skill && s.id == skill) else {
            // Items, and skills we no longer have, as a click would.
            self.queued_action = Some(windows::UiAction::UseHotkey(slot));
            return;
        };
        let (flags, level) = (s.target, (key.count.max(1) as u16).clamp(1, s.level.max(1)));
        let target = self.pad_target.and_then(|t| self.units.get(&t).map(|m| (t, m.kind, m.walker.cell)));
        let me = self.player.as_ref().map(|p| p.walker.cell);
        if flags & 4 != 0 {
            self.use_skill_on(skill, level, self.account_id);
        } else if flags & 16 != 0 && flags & 1 == 0 {
            let ally = target.filter(|t| t.1 == 0).map_or(self.account_id, |t| t.0);
            self.use_skill_on(skill, level, ally);
        } else if flags & 2 != 0 {
            if let Some(c) = target.map(|t| t.2).or(me) {
                self.use_skill_at(skill, level, (c.0 as u16, c.1 as u16));
            }
        } else if flags & 1 != 0 {
            let foe = target
                .filter(|t| t.1 == 5 || (t.1 == 0 && self.is_enemy_player(t.0)))
                .map(|t| t.0)
                .or_else(|| self.pad_candidates(false).into_iter().find(|id| self.units.get(id).is_some_and(|m| m.kind == 5)));
            if let Some(foe) = foe {
                self.pad_target = Some(foe);
                self.use_skill_on(skill, level, foe);
            }
        } else {
            self.start_skill(skill, level);
        }
    }

    /// Controller play: left stick walks (camera-relative), right stick turns and tilts the
    /// camera, A acts on the highlighted target, LB/RB change target, X picks up, Y sits,
    /// B cancels. Holding RT reaches the shortcut row on show, LT the next and, in battle
    /// mode, both the one after; then a face button, the D-pad or R3 fires a slot (at once in
    /// battle mode, else aimed first).
    pub(crate) fn pad_tick(&mut self, dt: f32) {
        use gilrs::Button::*;
        self.pad.tuning = self.settings.pad_tuning;
        let mut f = self.pad.poll();
        self.pad_row = None;
        let mut testing = false;
        if let Some((v, left)) = self.test_stick.as_mut() {
            *left -= dt;
            if *left > 0.0 {
                f.left = *v;
                testing = true;
            } else {
                self.test_stick = None;
            }
        }
        if !self.pad.in_use() && !testing {
            if self.pad_dir.is_some() && self.test_stick.is_none() {
                // A test stick just let go: stop as a released stick does.
                f.left = glam::Vec2::ZERO;
            } else {
                return;
            }
        }
        self.hud.pad_used = true;
        let secs = dt / 1000.0;

        // Camera: right stick turns and tilts; D-pad left/right turn 45 degrees, up/down zoom.
        if let (Some(r), false) = (self.renderer.as_mut(), self.still) {
            if f.right != glam::Vec2::ZERO {
                r.camera.rotate(-f.right.x * secs * 2.2, -f.right.y * secs * 1.2);
            }
            // (With a trigger held, R3 is a shortcut slot.)
            if f.just(RightThumb) && !(f.lt || f.rt) {
                r.camera.yaw = 0.0;
            }
        }

        // Dead: A returns to the save point; nothing else acts.
        if self.dead {
            if f.just(South) {
                self.send(ro_net::Command::Restart(0));
            }
            return;
        }

        // NPC dialogs take the D-pad and face buttons while open.
        if self.dialog.is_some() {
            self.hud.pad_nav = if f.just(DPadUp) {
                Some(hud::PadNav::Up)
            } else if f.just(DPadDown) {
                Some(hud::PadNav::Down)
            } else if f.just(South) {
                Some(hud::PadNav::Confirm)
            } else if f.just(East) {
                Some(hud::PadNav::Cancel)
            } else {
                None
            };
            return;
        }

        // Shortcut bar: hold RT (the row on show), LT (the next) or, in battle mode, both (the
        // one after), then a face button, the D-pad or R3 (padglyph::SLOT_BUTTONS' order).
        if f.rt || f.lt {
            let offset = match (f.lt, f.rt) {
                (true, true) if self.settings.battle_mode => 2,
                (true, false) => 1,
                _ => 0,
            };
            self.pad_row = Some(offset);
            const KEYS: [gilrs::Button; 9] = [South, East, West, North, DPadUp, DPadRight, DPadDown, DPadLeft, RightThumb];
            if let Some(col) = KEYS.iter().position(|k| f.just(*k)) {
                let slot = ((self.hotkey_row + offset) % HOTKEY_ROWS) as usize * HOTKEYS_PER_ROW + col;
                if self.settings.battle_mode {
                    self.pad_quick_slot(slot);
                } else {
                    self.queued_action = Some(windows::UiAction::UseHotkey(slot));
                }
            }
        } else {
            // Target: keep the current one while it is in reach, else the nearest.
            let support_aim = self.aiming.and_then(|(s, _)| self.skills.iter().find(|k| k.id == s)).is_some_and(|k| k.target & 16 != 0 && k.target & 1 == 0);
            let list = self.pad_candidates(support_aim);
            if self.pad_target.is_none_or(|t| !list.contains(&t)) {
                self.pad_target = list.first().copied();
            }
            if f.just(RightTrigger) || f.just(LeftTrigger) {
                let at = self.pad_target.and_then(|t| list.iter().position(|&x| x == t)).unwrap_or(0);
                let n = list.len().max(1);
                let next = if f.just(RightTrigger) { (at + 1) % n } else { (at + n - 1) % n };
                self.pad_target = list.get(next).copied();
            }
            let target = self.pad_target.and_then(|t| self.units.get(&t).map(|m| (t, m.kind, m.walker.cell)));
            if let Some((skill, level)) = self.aiming {
                // Aiming a skill: A on the target (or its cell), Y on yourself, B cancels.
                let flags = self.skills.iter().find(|s| s.id == skill).map_or(0, |s| s.target);
                let ground = flags & 2 != 0;
                // Support skills (Heal, Detoxify, Blessing...) go on a targeted player, else on us.
                let support = flags & 16 != 0 && flags & 1 == 0;
                if f.just(South) && support {
                    self.aiming = None;
                    let ally = target.filter(|t| t.1 == 0).map_or(self.account_id, |t| t.0);
                    self.use_skill_on(skill, level, ally);
                } else if f.just(South) {
                    self.aiming = None;
                    match target {
                        Some((_, _, c)) if ground => self.use_skill_at(skill, level, (c.0 as u16, c.1 as u16)),
                        Some((id, _, _)) => self.use_skill_on(skill, level, id),
                        None if ground => {
                            if let Some(c) = self.player.as_ref().map(|p| p.walker.cell) {
                                self.use_skill_at(skill, level, (c.0 as u16, c.1 as u16));
                            }
                        }
                        None => {}
                    }
                } else if f.just(North) {
                    self.aiming = None;
                    self.use_skill_on(skill, level, self.account_id);
                } else if f.just(East) {
                    self.aiming = None;
                }
            } else {
                if f.just(South) {
                    match target {
                        Some((id, 5, _)) => self.attack_unit(id),
                        Some((id, _, _)) => {
                            self.walk_goal = None;
                            self.talk(id);
                        }
                        None => {}
                    }
                }
                if f.just(East) {
                    // CZ_CANCEL_LOCKON: the server stops its auto-attack too.
                    if self.attack.take().is_some() {
                        self.send(ro_net::Command::Bare("StopAttack"));
                    }
                    self.chasing = false;
                    self.pending_skill = None;
                    self.pending_pickup = None;
                }
                if f.just(West) {
                    // Nearest item on the ground.
                    let me = self.player.as_ref().map(|p| p.walker.cell);
                    let item = me.and_then(|me| {
                        self.floor
                            .iter()
                            .map(|(id, (fi, _))| ((fi.x as i32 - me.0).pow(2) + (fi.y as i32 - me.1).pow(2), *id))
                            .filter(|(d, _)| *d <= 100)
                            .min()
                            .map(|(_, id)| id)
                    });
                    if let Some(id) = item {
                        self.pick_up(id);
                    }
                }
                if f.just(North) {
                    let sitting = self.player.as_ref().is_some_and(|p| p.sitting);
                    self.send(ro_net::Command::Sit(!sitting));
                }
            }
            if let (Some(r), false) = (self.renderer.as_mut(), self.still) {
                if f.just(DPadLeft) {
                    r.camera.rotate(std::f32::consts::FRAC_PI_4, 0.0);
                }
                if f.just(DPadRight) {
                    r.camera.rotate(-std::f32::consts::FRAC_PI_4, 0.0);
                }
                if f.just(DPadUp) {
                    r.camera.zoom(1.0);
                }
                if f.just(DPadDown) {
                    r.camera.zoom(-1.0);
                }
            }
        }
        if f.just(Start) {
            self.windows.settings = !self.windows.settings;
        }
        if f.just(Select) {
            self.windows.toggle_inventory();
        }

        // Left stick: walk along the line the stick points, from where that heading was set,
        // toward a goal PAD_STEP cells further along it. Re-aimed only when the walk is
        // nearly done or the stick turns past ~25 degrees: re-planning on every wobble makes
        // each path start with the same step, drifting a heading between straight and
        // diagonal to the diagonal, and costs stutters. Measuring along the line from its
        // start (not from where we are) keeps the heading true over a long walk.
        let Some(yaw) = self.renderer.as_ref().map(|r| r.camera.yaw) else { return };
        self.pad_walk_ms -= dt;
        if f.left != glam::Vec2::ZERO {
            let (sy, cy) = yaw.sin_cos();
            let (fwd, right) = (glam::Vec2::new(-sy, cy), glam::Vec2::new(cy, sy));
            let dir = (right * f.left.x + fwd * f.left.y).normalize_or_zero();
            let Some(p) = self.player.as_ref() else { return };
            let me = p.walker.cell;
            let here = glam::Vec2::new(me.0 as f32, me.1 as f32);
            let turned = self.pad_dir.is_none_or(|d| d.dot(dir) < 0.9);
            if turned {
                self.pad_dir = Some(dir);
                self.pad_line = Some(here);
            }
            let nearly_done = !p.walker.is_walking() || p.walker.remaining() <= 2;
            if (turned || nearly_done) && self.pad_walk_ms <= 0.0 {
                self.attack = None;
                self.chasing = false;
                self.pending_npc = None;
                self.pending_skill = None;
                self.pending_pickup = None;
                if p.sitting {
                    self.send(ro_net::Command::Sit(false));
                }
                let (dir, origin) = (self.pad_dir.unwrap_or(dir), self.pad_line.unwrap_or(here));
                let along = (here - origin).dot(dir);
                let gat = self.world.as_ref().and_then(|w| w.gat.as_ref());
                let goal = (1..=PAD_STEP).rev().map(|k| origin + dir * (along + k as f32)).map(|v| (v.x.round() as i32, v.y.round() as i32)).find(|&(x, y)| (x, y) != me && gat.is_some_and(|g| g.walkable(x, y)));
                if let Some((x, y)) = goal {
                    self.request_walk((x as u16, y as u16));
                }
                self.pad_walk_ms = 100.0;
            }
        } else if self.pad_dir.take().is_some() {
            // Let go: stop on the cell being stepped into (stopping on the one we are leaving
            // would walk us back to it).
            self.pad_line = None;
            if let Some(p) = self.player.as_ref() {
                let c = p.walker.next().unwrap_or(p.walker.cell);
                self.request_walk((c.0 as u16, c.1 as u16));
            }
        }
    }

    /// A left click on the world: talk, attack, aim, pick up, turn or walk.
    pub(crate) fn left_click(&mut self) {
        let unit = self.pick_unit().and_then(|id| self.units.get(&id).map(|m| (id, m.kind)));
        match unit {
            // NPCs: talk (walking over first if needed).
            Some((id, k)) if is_npc(k) => {
                self.walk_goal = None;
                self.attack = None;
                self.talk(id);
            }
            // PvP / GvG maps: clicking an enemy player attacks (the original's attack cursor).
            Some((id, 0)) if self.aiming.is_none() && self.is_enemy_player(id) => self.attack_unit(id),
            // Other players: a menu (trade, party, friend).
            Some((id, 0)) if self.aiming.is_none() => self.open_player_menu(id),
            // Monsters: attack until it dies (or use the skill being aimed).
            Some((id, 5)) if std::mem::take(&mut self.capturing) => self.send(ro_net::Command::Companion(ro_net::CompanionRequest::Capture(id))),
            Some((id, 5)) => match self.aiming.take() {
                Some((skill, level)) => self.use_skill_on(skill, level, id),
                None => self.attack_unit(id),
            },
            // Aiming at another player (heal, blessing...).
            Some((id, 0)) => {
                if let Some((skill, level)) = self.aiming.take() {
                    self.use_skill_on(skill, level, id);
                }
            }
            _ => {
                self.attack = None;
                self.chasing = false;
                if let Some((skill, level)) = self.aiming.take() {
                    let ground = self.skills.iter().find(|s| s.id == skill).is_some_and(|s| s.target & 2 != 0);
                    let me = self.player.as_ref().map(|p| p.walker.cell);
                    match self.pick() {
                        Some(cell) if ground => self.use_skill_at(skill, level, cell),
                        // Clicking our own cell targets ourselves.
                        Some(cell) if Some((cell.0 as i32, cell.1 as i32)) == me => self.use_skill_on(skill, level, self.account_id),
                        _ => {}
                    }
                } else if self.shift || self.player.as_ref().is_some_and(|p| p.sitting) {
                    if let Some(cell) = self.pick() {
                        // Holding the button keeps turning as the mouse goes round.
                        self.turn_drag = self.turn_to((cell.0 as i32, cell.1 as i32));
                    }
                } else if let Some(item) = self.pick_floor_item() {
                    self.pick_up(item);
                } else if let Some(cell) = self.pick() {
                    self.pending_npc = None;
                    self.pending_pickup = None;
                    self.pending_skill = None;
                    // Holding the button keeps steering toward the cursor.
                    self.mouse_walk = true;
                    self.mouse_walk_ms = 150.0;
                    self.request_walk(cell);
                }
            }
        }
    }
    /// Turns in place to look at a cell (CZ_CHANGE_DIRECTION): Shift+click, a click while
    /// sitting, talking to an NPC. The server tells only the others, so we turn ourselves.
    /// Returns the direction looked toward.
    pub(crate) fn turn_to(&mut self, cell: (i32, i32)) -> Option<usize> {
        let p = self.player.as_mut().filter(|p| !p.walker.is_walking())?;
        let (dx, dy) = (cell.0 - p.walker.cell.0, cell.1 - p.walker.cell.1);
        if (dx, dy) == (0, 0) {
            return None;
        }
        let want = ro_game::dir_from_step(dx, dy);
        let (head, dir) = ro_game::turn_toward(p.walker.dir, p.actor.head_dir, want);
        if (head, dir) != (p.actor.head_dir, p.walker.dir) {
            (p.actor.head_dir, p.walker.dir) = (head, dir);
            self.send(ro_net::Command::Turn { head, dir: dir as u8 });
        }
        Some(want)
    }

    /// The mouse moved with the button held after a turn in place: look the new way, if the
    /// cursor is now in another direction.
    pub(crate) fn drag_turn(&mut self) {
        let (Some(cell), Some(p)) = (self.pick(), self.player.as_ref()) else { return };
        let cell = (cell.0 as i32, cell.1 as i32);
        let (dx, dy) = (cell.0 - p.walker.cell.0, cell.1 - p.walker.cell.1);
        if (dx, dy) != (0, 0) && Some(ro_game::dir_from_step(dx, dy)) != self.turn_drag {
            self.turn_drag = self.turn_to(cell);
        }
    }

    /// The NPC or monster under the cursor, nearest to the camera first.
    pub(crate) fn pick_unit(&self) -> Option<u32> {
        let (r, (cx, cy)) = (self.renderer.as_ref()?, self.cursor?);
        let size = r.size();
        let eye = r.camera.eye();
        self.units
            .iter()
            .filter_map(|(&id, m)| {
                if m.actor.alpha <= 0.01 {
                    return None;
                }
                let feet = r.project(m.actor.pos, size)?;
                // Rough sprite box: about two cells tall, a little over one wide.
                let top = r.project(m.actor.pos + glam::Vec3::Y * 2.2 * ro_render::CELL, size)?;
                let h = (feet.y - top.y).max(8.0);
                let w = h * 0.55;
                let inside = (cx as f32 - feet.x).abs() <= w / 2.0 && (cy as f32) <= feet.y + h * 0.1 && (cy as f32) >= top.y;
                inside.then(|| (id, m.actor.pos.distance_squared(eye)))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// Ground cell under the cursor, if any.
    pub(crate) fn pick(&self) -> Option<(u16, u16)> {
        let (r, w, (cx, cy)) = (self.renderer.as_ref()?, self.world.as_ref()?, self.cursor?);
        let (origin, dir) = r.ray(cx as f32, cy as f32);
        // March along the ray until it passes below what actors stand on (`floor_at`), not
        // the drawn ground: Herosria's platforms stand 30 units over it, so picking the ground
        // landed clicks cells away.
        let mut t = 0.0;
        while t < 5000.0 {
            let p = origin + dir * t;
            if p.y <= w.floor_at(p.x, p.z) {
                let (x, y) = ((p.x / ro_render::CELL).floor(), (p.z / ro_render::CELL).floor());
                return (x >= 0.0 && y >= 0.0).then_some((x as u16, y as u16));
            }
            t += 1.0;
        }
        None
    }
}

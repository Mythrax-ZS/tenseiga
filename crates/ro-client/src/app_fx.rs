//! Skill, script and map effects, and unit actions (hits, quakes). Part of `App` (app.rs).

use super::*;

impl App {
    pub(crate) fn skill_fx(&mut self, skill: u16, pos: Option<glam::Vec3>, damaging: bool) {
        if let Some(q) = fx::skill_quake(skill) {
            self.quake(q);
        }
        // The skill's own sound, when the client has one.
        if let (Some(a), Some(vfs), Some(name)) = (self.audio.as_ref(), self.vfs.as_deref(), self.skill_names.internal(skill)) {
            match a.skill_sound(name) {
                Some(file) => a.play(vfs, &file),
                None => tracing::debug!(skill, name, "no sound for skill"),
            }
        }
        let Some(pos) = pos else { return };
        let (Some(r), Some(vfs)) = (self.renderer.as_mut(), self.vfs.as_deref()) else { return };
        match fx::skill_visual(skill) {
            Some(fx::Visual::Str(name)) => {
                if let Some(asset) = self.fx.get(r, vfs, name) {
                    r.effects.push(ro_render::Fx { asset, pos, time_ms: 0.0, looping: false, tag: 0 });
                }
            }
            Some(fx::Visual::OneOf(names)) => {
                self.spark_seed = self.spark_seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let name = names[(self.spark_seed >> 16) as usize % names.len()];
                if let Some(asset) = self.fx.get(r, vfs, name) {
                    r.effects.push(ro_render::Fx { asset, pos, time_ms: 0.0, looping: false, tag: 0 });
                }
            }
            Some(fx::Visual::Column(tex, color)) => {
                if let Some(tex) = self.fx.texture(r, vfs, tex) {
                    let shape = ro_render::Shape::Column { radius: 0.9, height: 3.5 };
                    r.procs.push(ro_render::ProcFx { tex, shape, pos, color, time_ms: 0.0, duration_ms: 1200.0 });
                }
            }
            Some(fx::Visual::Sprite(name)) => {
                if let Some(body) = self.fx.sprite(r, vfs, name) {
                    let actor = ro_render::Actor {
                        pos,
                        body,
                        head: None,
                        headgear: vec![],
                        weapon: vec![],
                        shadow: None,
                        action: 0,
                        dir: ro_render::Dir::S,
                        head_dir: 0,
                        time_ms: 0.0,
                        once: true,
                        alpha: 1.0, scale: 1.0, tint: [1.0; 3], silhouette: None,
                    };
                    let ms = r.action_ms(body, 0).max(200.0);
                    self.sprite_fx.push((actor, ms));
                }
            }
            Some(fx::Visual::Code) => self.wizard_fx(skill, pos),
            Some(fx::Visual::Tenseiga) => {
                for &(part, texture, duration_ms, color) in fx::tenseiga_layers(skill) {
                    let Some(tex) = self.fx.fx_texture(r, vfs, texture) else { continue };
                    let shape = match part {
                        Some((part, solid)) => ro_render::Shape::Tenseiga { part, solid },
                        None => ro_render::Shape::Column { radius: 0.9, height: 3.5 },
                    };
                    r.procs.push(ro_render::ProcFx { tex, shape, pos, color, time_ms: 0.0, duration_ms });
                }
            }
            None => {
                let by_name = self.skill_names.internal(skill).map(|n| {
                    let short = n.split_once('_').map_or(n, |(_, rest)| rest).to_ascii_lowercase();
                    [short.clone(), short.replace('_', "")]
                });
                let folder = by_name.as_ref().and_then(|n| self.fx.find(&n[0])).map(str::to_string);
                let asset = by_name.iter().flatten().find_map(|n| self.fx.get(r, vfs, n))
                    .or_else(|| folder.and_then(|p| self.fx.get(r, vfs, &p))).or_else(|| damaging.then(|| self.fx.get(r, vfs, "hit")).flatten());
                if let Some(asset) = asset {
                    r.effects.push(ro_render::Fx { asset, pos, time_ms: 0.0, looping: false, tag: 0 });
                } else if let Some(tex) = self.fx.texture(r, vfs, "alpha_down.tga") {
                    // Buffs and other support skills without their own effect: a soft light.
                    let shape = ro_render::Shape::Column { radius: 0.8, height: 3.0 };
                    r.procs.push(ro_render::ProcFx { tex, shape, pos, color: [0.45, 0.45, 0.5], time_ms: 0.0, duration_ms: 900.0 });
                }
                tracing::debug!(skill, name = ?self.skill_names.internal(skill), "no mapped effect; used fallback");
            }
        }
    }

    /// A skill's effect on a target it hit: its own effect for single-target skills, a small
    /// hit for area skills (whose own effect played once, where they were cast).
    pub(crate) fn skill_hit_fx(&mut self, skill: u16, pos: Option<glam::Vec3>) {
        match fx::skill_hit(skill) {
            fx::HitVisual::Own => self.skill_fx(skill, pos, true),
            fx::HitVisual::Nothing => {}
            fx::HitVisual::OneOf(names) => {
                self.spark_seed = self.spark_seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let name = names[(self.spark_seed >> 16) as usize % names.len()];
                if let Some(pos) = pos {
                    self.play_fx(name, pos);
                }
            }
        }
    }

    /// The Wizard's effects the original client draws in code, at `pos` (the caster for Frost
    /// Nova and Sightrasher, the target for Earth Spike, the cell for Heaven's Drive).
    pub(crate) fn wizard_fx(&mut self, skill: u16, pos: glam::Vec3) {
        match skill {
            // Sightrasher: a ring of fire bursting out from the caster.
            81 => {
                self.proc_fx("fire_a.bmp", ro_render::Shape::Wave { radius: 3.5 }, pos, [1.0, 0.55, 0.25], 700.0);
                self.play_fx("firehit2", pos);
            }
            // Frost Nova: shards of ice bursting out from the caster's feet (EF_FROSTDIVER2).
            88 => self.horns_fx(ro_render::Horns::FrostNova, "ice.tga", pos, 700.0),
            // Earth Spike: a spike of rock thrust up under the target (EF_EARTHSPIKE), its
            // earth hit flashing there.
            90 => {
                self.horns_fx(ro_render::Horns::EarthSpike, "stone.bmp", pos, 3000.0);
                self.play_fx("earthhit", pos);
            }
            // Heaven's Drive: rock thrust up over its five cells square (EF_HEAVENDRIVE).
            91 => self.horns_fx(ro_render::Horns::HeavensDrive, "stone.bmp", pos, 2500.0),
            _ => {}
        }
    }

    /// Spikes thrust up out of the ground at `pos` (`ro_render::Shape::Horns`), lasting
    /// `duration_ms`.
    fn horns_fx(&mut self, kind: ro_render::Horns, tex: &'static str, pos: glam::Vec3, duration_ms: f32) {
        self.spark_seed = self.spark_seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let shape = ro_render::Shape::Horns { kind, seed: self.spark_seed };
        self.proc_fx(tex, shape, pos, [1.0; 3], duration_ms);
    }

    /// What shows while a spell is being cast (as the original's ZC_USESKILL_ACK handling):
    /// the caster's aura in the spell's element, and the lock-on ring under its target or the
    /// magic circle where an area spell will fall.
    pub(crate) fn cast_fx(&mut self, c: &ro_net::Cast) {
        let ms = c.ms as f32;
        if let Some(pos) = self.unit_pos(c.src) {
            let (tex, color, spin, rings) = fx::cast_aura(c.element);
            for &(top, height, grow, alpha) in rings {
                let shape = ro_render::Shape::Cylinder { bottom: fx::AURA_BASE, top, height, grow, alpha, spin, owner: c.src };
                self.proc_fx(tex, shape, pos, color, ms);
            }
        }
        if c.target != 0 && c.target != c.src {
            if let Some(pos) = self.unit_pos(c.target) {
                let shape = ro_render::Shape::LockOn { target: c.target, owner: c.src };
                self.proc_fx("lockon128.tga", shape, pos, [1.0; 3], ms);
            }
        } else if c.target == 0 {
            if let Some(pos) = self.cell_pos(c.cell) {
                let shape = ro_render::Shape::Rune { size: fx::cast_circle(c.skill), owner: c.src };
                self.proc_fx("magic_target.tga", shape, pos, [1.0; 3], ms);
            }
        }
    }

    /// Jupitel Thunder: a ball of lightning flying from the caster to its target.
    pub(crate) fn jupitel_fx(&mut self, from: glam::Vec3, to: glam::Vec3) {
        self.proc_fx("thunder_ball_a.bmp", ro_render::Shape::Orb { reach: to - from }, from, [0.75, 0.85, 1.0], 500.0);
    }

    /// Spikes of rock erupting in a wave from `from` toward `to` (Grimtooth, Earth Spike,
    /// Heaven's Drive), effect\stone.bmp's rock.
    pub(crate) fn spike_line(&mut self, from: glam::Vec3, to: glam::Vec3) {
        let (Some(r), Some(vfs)) = (self.renderer.as_mut(), self.vfs.as_deref()) else { return };
        let Some(tex) = self.fx.texture(r, vfs, "stone.bmp") else { return };
        let reach = to - from;
        let count = ((reach.length() / ro_render::CELL) as u32).clamp(3, 12);
        let shape = ro_render::Shape::Spikes { reach, count };
        r.procs.push(ro_render::ProcFx { tex, shape, pos: from, color: [0.95, 0.88, 0.78], time_ms: 0.0, duration_ms: 800.0 });
    }

    pub(crate) fn proc_fx(&mut self, tex: &'static str, shape: ro_render::Shape, pos: glam::Vec3, color: [f32; 3], duration_ms: f32) {
        let (Some(r), Some(vfs)) = (self.renderer.as_mut(), self.vfs.as_deref()) else { return };
        if let Some(tex) = self.fx.texture(r, vfs, tex) {
            r.procs.push(ro_render::ProcFx { tex, shape, pos, color, time_ms: 0.0, duration_ms });
        }
    }

    /// The wedding ceremony (ZC_CONGRATULATION, EF_COLORPAPER): coloured paper showering down
    /// on the newlyweds, to effect\wedding.wav. The original draws it in code.
    pub(crate) fn wedding_fx(&mut self, id: u32) {
        let Some(pos) = self.unit_pos(id) else { return };
        self.play_sound("effect\\wedding.wav");
        self.spark_seed = self.spark_seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let (Some(r), Some(vfs)) = (self.renderer.as_mut(), self.vfs.as_deref()) else { return };
        if let Some(tex) = self.fx.fx_texture(r, vfs, fx::FxTexture::Paper) {
            let shape = ro_render::Shape::Confetti { seed: self.spark_seed ^ id };
            r.procs.push(ro_render::ProcFx { tex, shape, pos, color: [1.0; 3], time_ms: 0.0, duration_ms: 6500.0 });
        }
    }

    /// Meteor Assault: eight violet slashes flying outward from the caster.
    /// The cast and every monster hit all announce it, so it plays once per cast.
    pub(crate) fn meteor_assault(&mut self, src: u32) {
        use ro_render::{Shape, CELL};
        let now = std::time::Instant::now();
        if self.burst_at.get(&src).is_some_and(|t| now.duration_since(*t).as_millis() < 400) {
            return;
        }
        let Some(at) = self.unit_pos(src) else { return };
        self.burst_at.insert(src, now);
        for i in 0..8 {
            let a = i as f32 / 8.0 * std::f32::consts::TAU;
            let dir = glam::Vec3::new(a.cos(), 0.0, a.sin()) * CELL;
            let shape = Shape::Slash { travel: dir * 4.0, size: (1.0, 2.0) };
            self.proc_fx("purpleslash.tga", shape, at + dir, [1.0, 1.0, 1.0], 500.0);
        }
    }

    /// Crescent Gale: a crescent of mortal flame flying from the caster past the target, the
    /// server's 11-cell path. Every enemy on the path reports a hit, so it plays once per cast.
    pub(crate) fn crescent_gale(&mut self, src: u32, from: glam::Vec3, to: glam::Vec3) {
        let now = std::time::Instant::now();
        if self.burst_at.get(&src).is_some_and(|t| now.duration_since(*t).as_millis() < 400) {
            return;
        }
        self.burst_at.insert(src, now);
        let travel = (to - from).normalize_or_zero() * ro_render::CELL * 11.0;
        let (Some(r), Some(vfs)) = (self.renderer.as_mut(), self.vfs.as_deref()) else { return };
        let Some(tex) = self.fx.fx_texture(r, vfs, fx::FxTexture::Streak) else { return };
        for solid in [true, false] {
            let shape = ro_render::Shape::Crescent { travel, solid };
            r.procs.push(ro_render::ProcFx { tex, shape, pos: from, color: [1.0; 3], time_ms: 0.0, duration_ms: 800.0 });
        }
    }

    pub(crate) fn play_fx(&mut self, name: &'static str, pos: glam::Vec3) {
        self.play_fx_tagged(name, pos, 0);
    }

    /// Plays an STR effect; a non-zero `tag` loops it until `stop_fx(tag)`.
    /// A trap's model standing at `pos` (loaded once per model).
    pub(crate) fn trap_draws(&mut self, path: &'static str, pos: glam::Vec3) -> Vec<ro_render::ModelDraw> {
        let (Some(r), Some(vfs)) = (self.renderer.as_mut(), self.vfs.as_deref()) else { return vec![] };
        let model = self
            .trap_models
            .entry(path)
            .or_insert_with(|| {
                let file = format!("data\\model\\{path}.rsm");
                let rsm = vfs.read(&file).ok().flatten().and_then(|d| ro_formats::rsm::Rsm::parse(&d).inspect_err(|e| tracing::warn!(%file, "{e:#}")).ok())?;
                let textures = rsm
                    .textures
                    .iter()
                    .map(|t| {
                        let p = format!("data\\texture\\{t}");
                        let img = vfs.read(&p).ok().flatten().and_then(|d| ro_formats::image::decode(&d, &p).ok())?;
                        Some(r.add_model_texture(&img))
                    })
                    .collect();
                Some(std::rc::Rc::new((rsm, textures)))
            })
            .clone();
        let Some(model) = model else { return vec![] };
        ro_render::rsm_at(&model.0, pos)
            .into_iter()
            .filter_map(|(t, vertices)| Some(ro_render::ModelDraw { texture: (*model.1.get(t)?)?, vertices }))
            .collect()
    }

    pub(crate) fn play_fx_tagged(&mut self, name: &'static str, pos: glam::Vec3, tag: u32) {
        let (Some(r), Some(vfs)) = (self.renderer.as_mut(), self.vfs.as_deref()) else { return };
        if let Some(asset) = self.fx.get(r, vfs, name) {
            r.effects.push(ro_render::Fx { asset, pos, time_ms: 0.0, looping: tag != 0, tag });
        }
    }

    pub(crate) fn stop_fx(&mut self, tag: u32) {
        if let Some(r) = self.renderer.as_mut() {
            r.effects.retain(|e| e.tag != tag);
        }
    }

    pub(crate) fn mob_mut(&mut self, id: u32) -> Option<&mut Mob> {
        if id == self.account_id { self.player.as_mut() } else { self.units.get_mut(&id) }
    }

    /// Someone acted on someone: animate both, and show what happened.
    /// Puts a hit in the combat log and meter (combat.rs) when it is ours (us, our
    /// homunculus, mercenary or elemental) or lands on us. `skill` 0 is a plain attack.
    pub(crate) fn log_combat(&mut self, skill: u16, act: &ro_net::Act) {
        // Sitting, standing and picking up come as acts too; skills that do no damage say -1.
        if matches!(act.kind, 1..=3) || act.src == act.target || act.damage < 0 {
            return;
        }
        let me = self.account_id;
        let c = &self.companions;
        let by = if act.src == me {
            Some(combat::Source::Me)
        } else if c.homun_id != 0 && act.src == c.homun_id {
            Some(combat::Source::Homunculus)
        } else if c.merc.as_ref().is_some_and(|m| m.id == act.src) {
            Some(combat::Source::Mercenary)
        } else if c.elem.as_ref().is_some_and(|e| e.id == act.src) {
            Some(combat::Source::Elemental)
        } else {
            None
        };
        if by.is_none() && act.target != me {
            return;
        }
        let name = |id: u32| self.units.get(&id).map(|m| m.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| "?".into());
        let other = if by.is_some() { name(act.target) } else { name(act.src) };
        let skill_name = if skill == 0 { "Attack".to_string() } else { self.skill_names.internal(skill).map_or_else(|| format!("Skill #{skill}"), |i| self.skill_names.display(i)) };
        let damage = if act.kind == 11 { 0 } else { act.damage + act.left_damage };
        let hit = combat::Hit { by, skill, skill_name, other, damage, hits: act.hits.max(1) as u16, crit: act.kind == 10, stamp: chrono::Local::now().format("%H:%M:%S").to_string() };
        self.combat.record(hit, std::time::Instant::now());
        // The target frame: the monster we hit, else one hitting us when we have none.
        let now = std::time::Instant::now();
        let monster = |id: u32| self.units.get(&id).is_some_and(|m| m.kind == 5);
        if by == Some(combat::Source::Me) && monster(act.target) {
            self.focus = Some((act.target, now));
        } else if by.is_none() && monster(act.src) {
            match self.focus {
                Some((id, _)) if id == act.src => self.focus = Some((id, now)),
                Some((id, last)) if targetframe::keeps(last, now) && self.units.contains_key(&id) => {}
                _ => self.focus = Some((act.src, now)),
            }
        }
    }

    pub(crate) fn act(&mut self, act: ro_net::Act) {
        let me = self.account_id;
        let cell_of = |app: &Self, id: u32| -> Option<(i32, i32)> {
            if id == me { app.player.as_ref().map(|p| p.walker.cell) } else { app.units.get(&id).map(|m| m.walker.cell) }
        };
        let (src_cell, target_cell) = (cell_of(self, act.src), cell_of(self, act.target));
        self.casts.remove(&act.src);
        match act.kind {
            2 | 3 => {
                if let Some(m) = self.mob_mut(act.src) {
                    m.sitting = act.kind == 2;
                }
                return;
            }
            1 => {
                tracing::debug!(src = act.src, "pick-up animation");
                // Picked something up: crouch (three frames, about 300 ms).
                if let Some(m) = self.mob_mut(act.src) {
                    m.set_pose(Pose::PickUp, 300.0);
                }
                return;
            }
            _ => {}
        }
        if let Some(m) = self.mob_mut(act.src) {
            if let Some(c) = target_cell {
                m.face(c);
            }
            m.set_pose(Pose::Attack, act.src_speed as f32);
        }
        let damage = act.damage + act.left_damage;
        // Players swing with their weapon's sound and land its hit sound; monsters' attacks
        // sound through their animation frames (see the unit update).
        if let Some(kind) = self.weapon_sound(act.src) {
            self.play_sound(&format!("_attack_{kind}.wav"));
            if damage > 0 {
                let hit = if kind == "fist" { format!("_hit_fist{}.wav", act.src % 4 + 1) } else { format!("_hit_{kind}.wav") };
                self.play_sound(&hit);
            }
        }
        let mut spark = None;
        if let Some(m) = self.mob_mut(act.target) {
            if damage > 0 && act.kind != 4 && act.kind != 9 {
                if let Some(c) = src_cell {
                    m.face(c);
                }
                m.set_pose(Pose::Hurt, act.dmg_speed as f32);
            }
            let pos = m.actor.pos;
            // The original's hit spark lands on monsters only (roBrowser's EF_HIT1 spawn).
            if damage > 0 && m.kind == 5 {
                spark = Some(pos);
            }
            let (text, color) = match act.kind {
                11 => ("Lucky".to_string(), egui::Color32::from_rgb(140, 220, 255)),
                _ if damage <= 0 => ("Miss".to_string(), egui::Color32::from_rgb(200, 200, 200)),
                10 => (format!("{damage}!"), egui::Color32::from_rgb(255, 210, 60)),
                _ if act.target == me => (damage.to_string(), egui::Color32::from_rgb(255, 90, 90)),
                _ => (damage.to_string(), egui::Color32::WHITE),
            };
            // Several hits in one (Triple Attack, Double Strafe, Sonic Blow, double attacks):
            // each hit's share in turn, then the total in yellow above, as the original shows
            // them. Criticals and Lucky dodges stay one number.
            let shares = if damage > 0 && !matches!(act.kind, 10 | 11) { hit_shares(damage, act.hits) } else { vec![] };
            if shares.len() > 1 {
                let last = (shares.len() - 1) as f32 * HIT_GAP_MS;
                for (i, share) in shares.iter().enumerate() {
                    self.floaters.push((pos, share.to_string(), color, -(i as f32) * HIT_GAP_MS));
                }
                self.floaters.push((pos + glam::Vec3::Y * ro_render::CELL, damage.to_string(), egui::Color32::from_rgb(255, 225, 80), -last));
            } else {
                self.floaters.push((pos, text, color, 0.0));
            }
        }
        if let Some(pos) = spark {
            self.hit_spark(pos);
        }
    }

    /// EF_HIT1, the spark where a hit lands: four flecks of effect\pok3.tga flying out from a
    /// cell above the target for 300 ms (roBrowser's EffectTable: alpha 0.8, ends up to 2 cells off).
    pub(crate) fn hit_spark(&mut self, pos: glam::Vec3) {
        let (Some(r), Some(vfs)) = (self.renderer.as_mut(), self.vfs.as_deref()) else { return };
        let Some(tex) = self.fx.texture(r, vfs, "pok3.tga") else { return };
        for _ in 0..4 {
            self.spark_seed = self.spark_seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let f = |shift: u32| ((self.spark_seed >> shift) & 0xff) as f32 / 255.0 * 2.0 - 1.0;
            let drift = glam::Vec3::new(f(0), f(8) * 0.5 + 0.5, f(16)) * 2.0 * ro_render::CELL;
            let shape = ro_render::Shape::Spark { drift, size: 1.0 + (f(24) + 1.0) * 0.5 };
            r.procs.push(ro_render::ProcFx { tex, shape, pos, color: [0.8; 3], time_ms: 0.0, duration_ms: 300.0 });
        }
    }

    /// Shakes the screen for an effect (after its delay), unless Settings turns that off.
    pub(crate) fn quake(&mut self, (delay, duration): fx::Quake) {
        if self.settings.screen_shake {
            let start = std::time::Instant::now() + std::time::Duration::from_millis(delay as u64);
            self.quakes.push((start, start + std::time::Duration::from_millis(duration as u64)));
        }
    }
}

/// Milliseconds between the numbers of a multi-hit attack.
const HIT_GAP_MS: f32 = 200.0;

/// A multi-hit attack's damage split per hit, the remainder on the last. One share when the
/// server counts a single hit (or a negative count: drawn as one, e.g. plant mode).
fn hit_shares(damage: i32, hits: i16) -> Vec<i32> {
    let n = i32::from(hits.max(1)).min(damage.max(1));
    let mut shares = vec![damage / n; n as usize];
    if let Some(last) = shares.last_mut() {
        *last += damage % n;
    }
    shares
}

#[cfg(test)]
mod hit_share_tests {
    #[test]
    fn damage_splits_across_the_hits() {
        assert_eq!(super::hit_shares(900, 3), [300, 300, 300]);
        assert_eq!(super::hit_shares(1000, 3), [333, 333, 334], "the remainder lands last");
        assert_eq!(super::hit_shares(500, 1), [500]);
        assert_eq!(super::hit_shares(500, -3), [500], "a negative count is drawn as one");
        assert_eq!(super::hit_shares(2, 5), [1, 1], "never more numbers than damage");
    }
}

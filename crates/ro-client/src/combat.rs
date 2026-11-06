//! The combat log and damage meter (Alt+D, or More… > Combat Log). Every hit we, our
//! homunculus, mercenary or elemental land, and every hit we take, from the server's attack
//! and skill packets. The meter shows the fight in progress (or the last one): damage, damage
//! per second and damage taken; then the session's totals, and damage by skill. A fight ends
//! after `FIGHT_GAP` with no hits either way. Reset clears it all.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use egui::{Color32, RichText};

/// A fight is over once this long passes without a hit either way.
pub const FIGHT_GAP: Duration = Duration::from_secs(6);
/// Lines the log keeps.
const LOG_LINES: usize = 1000;

/// Whose hit it was, of ours.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Source {
    Me,
    Homunculus,
    Mercenary,
    Elemental,
}

impl Source {
    fn name(self) -> &'static str {
        match self {
            Source::Me => "You",
            Source::Homunculus => "Homunculus",
            Source::Mercenary => "Mercenary",
            Source::Elemental => "Elemental",
        }
    }
}

/// One hit, as the client saw it.
#[derive(Clone, Debug)]
pub struct Hit {
    /// Ours that landed it, or None when it landed on us.
    pub by: Option<Source>,
    /// 0 for a plain attack.
    pub skill: u16,
    pub skill_name: String,
    /// The target (our hits) or the attacker (hits on us).
    pub other: String,
    /// All of it (both hands, all hits); 0 or less is a miss.
    pub damage: i32,
    pub hits: u16,
    pub crit: bool,
    /// The local time, for the log.
    pub stamp: String,
}

#[derive(Default, Clone, Copy, Debug, PartialEq)]
struct Row {
    hits: u32,
    damage: i64,
    biggest: i32,
    crits: u32,
    misses: u32,
}

#[derive(Clone, Copy, Debug)]
struct Fight {
    start: Instant,
    last: Instant,
    dealt: i64,
    taken: i64,
}

impl Fight {
    /// From its first hit to its last (at least a second, so one hit doesn't read as a huge
    /// rate); steady between hits rather than sagging while we wait for the next.
    fn secs(&self) -> f64 {
        (self.last - self.start).as_secs_f64().max(1.0)
    }
}

#[derive(Default, PartialEq, Clone, Copy)]
enum Tab {
    #[default]
    Meter,
    Log,
}

#[derive(Default)]
pub struct Combat {
    pub open: bool,
    tab: Tab,
    log: VecDeque<Hit>,
    since: Option<(Instant, String)>,
    dealt: i64,
    taken: i64,
    rows: HashMap<(Source, u16, String), Row>,
    fight: Option<Fight>,
}

impl Combat {
    pub fn record(&mut self, hit: Hit, now: Instant) {
        if self.since.is_none() {
            self.since = Some((now, hit.stamp.clone()));
        }
        let damage = hit.damage.max(0) as i64;
        // A new fight after a quiet spell.
        let fight = match self.fight {
            Some(f) if now.saturating_duration_since(f.last) <= FIGHT_GAP => self.fight.as_mut().unwrap(),
            _ => self.fight.insert(Fight { start: now, last: now, dealt: 0, taken: 0 }),
        };
        fight.last = now;
        match hit.by {
            Some(by) => {
                fight.dealt += damage;
                self.dealt += damage;
                let row = self.rows.entry((by, hit.skill, hit.skill_name.clone())).or_default();
                row.hits += 1;
                row.damage += damage;
                row.biggest = row.biggest.max(hit.damage);
                row.crits += hit.crit as u32;
                row.misses += (hit.damage <= 0) as u32;
            }
            None => {
                fight.taken += damage;
                self.taken += damage;
            }
        }
        self.log.push_back(hit);
        while self.log.len() > LOG_LINES {
            self.log.pop_front();
        }
    }

    pub fn reset(&mut self) {
        *self = Combat { open: self.open, tab: self.tab, ..Default::default() };
    }

    /// The fight in progress or the last one: (going on, seconds, dealt, per second, taken).
    fn fight_summary(&self, now: Instant) -> Option<(bool, f64, i64, f64, i64)> {
        let f = self.fight?;
        let secs = f.secs();
        Some((now.saturating_duration_since(f.last) <= FIGHT_GAP, secs, f.dealt, f.dealt as f64 / secs, f.taken))
    }

    /// Damage by who and skill, most first: (who, skill name, row).
    fn table(&self) -> Vec<(Source, &str, Row)> {
        let mut rows: Vec<_> = self.rows.iter().map(|((by, _, name), row)| (*by, name.as_str(), *row)).collect();
        rows.sort_by(|a, b| b.2.damage.cmp(&a.2.damage).then(a.1.cmp(b.1)));
        rows
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let now = Instant::now();
        let mut open = true;
        let mut reset = false;
        let gray = crate::classic::ink(Color32::GRAY);
        crate::classic::show(ctx, egui::Window::new("Combat").open(&mut open).default_width(380.0).default_height(320.0), "Combat", true, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Meter, "Damage meter");
                ui.selectable_value(&mut self.tab, Tab::Log, "Combat log");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    reset = ui.button("Reset").on_hover_text("Clear the meter and the log").clicked();
                });
            });
            ui.separator();
            match self.tab {
                Tab::Meter => self.meter_ui(ui, now, gray),
                Tab::Log => self.log_ui(ui, gray),
            }
        });
        if reset {
            self.reset();
        }
        self.open = open;
        // Live damage per second while a fight goes on.
        if self.fight_summary(now).is_some_and(|f| f.0) {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn meter_ui(&self, ui: &mut egui::Ui, now: Instant, gray: Color32) {
        let Some((going, secs, dealt, dps, taken)) = self.fight_summary(now) else {
            ui.label(RichText::new("No fighting yet. Hits you land and take show here.").color(gray));
            return;
        };
        ui.label(RichText::new(format!("{} · {}", if going { "This fight" } else { "Last fight" }, clock(secs))).strong());
        ui.label(format!("Dealt {} · {} per second · Taken {}", group(dealt), group(dps.round() as i64), group(taken)));
        if let Some((_, at)) = &self.since {
            ui.label(RichText::new(format!("Since {at}: dealt {}, taken {}", group(self.dealt), group(self.taken))).color(gray));
        }
        ui.add_space(4.0);
        let rows = self.table();
        if rows.is_empty() {
            return;
        }
        let total = rows.iter().map(|r| r.2.damage).sum::<i64>().max(1);
        egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
            egui::Grid::new("combat_meter").num_columns(5).striped(true).spacing([12.0, 3.0]).show(ui, |ui| {
                for h in ["Skill", "Hits", "Damage", "Share", "Biggest"] {
                    ui.label(RichText::new(h).color(gray));
                }
                ui.end_row();
                for (by, name, row) in rows {
                    let label = if by == Source::Me { name.to_string() } else { format!("{} · {name}", by.name()) };
                    ui.label(label);
                    let mut hits = group(row.hits as i64);
                    if row.misses > 0 {
                        hits = format!("{hits} ({} missed)", row.misses);
                    }
                    ui.label(hits);
                    ui.label(group(row.damage));
                    let share = row.damage as f32 / total as f32;
                    ui.add(egui::ProgressBar::new(share).desired_width(70.0).text(format!("{:.0}%", share * 100.0)));
                    ui.label(if row.crits > 0 { format!("{} ({} crit)", group(row.biggest.max(0) as i64), row.crits) } else { group(row.biggest.max(0) as i64) });
                    ui.end_row();
                }
            });
        });
    }

    fn log_ui(&self, ui: &mut egui::Ui, gray: Color32) {
        if self.log.is_empty() {
            ui.label(RichText::new("Nothing yet.").color(gray));
            return;
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
            for hit in &self.log {
                let (text, color) = line(hit);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&hit.stamp).color(gray).monospace().size(11.0));
                    ui.label(RichText::new(text).color(crate::classic::ink(color)));
                });
            }
        });
    }
}

/// A log line and its colour: ours white (gold for criticals), hits on us red, misses grey.
fn line(hit: &Hit) -> (String, Color32) {
    let what = if hit.skill == 0 { "Attack".to_string() } else { hit.skill_name.clone() };
    let amount = if hit.damage <= 0 {
        "Miss".to_string()
    } else if hit.hits > 1 {
        format!("{} ({} hits)", group(hit.damage as i64), hit.hits)
    } else {
        group(hit.damage as i64)
    };
    let crit = if hit.crit { " critical" } else { "" };
    match hit.by {
        Some(by) => {
            let who = if by == Source::Me { String::new() } else { format!("{}: ", by.name()) };
            let color = if hit.damage <= 0 {
                Color32::from_rgb(160, 160, 160)
            } else if hit.crit {
                Color32::from_rgb(255, 200, 60)
            } else {
                Color32::from_rgb(235, 235, 235)
            };
            (format!("{who}{what} → {}: {amount}{crit}", hit.other), color)
        }
        None => {
            let color = if hit.damage <= 0 { Color32::from_rgb(160, 160, 160) } else { Color32::from_rgb(255, 120, 110) };
            (format!("{} ({what}) → you: {amount}{crit}", hit.other), color)
        }
    }
}

/// 1234567 -> "1,234,567".
fn group(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

/// Seconds as "m:ss".
fn clock(secs: f64) -> String {
    let s = secs.round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(by: Option<Source>, skill: u16, damage: i32) -> Hit {
        Hit { by, skill, skill_name: if skill == 0 { "Attack".into() } else { format!("Skill {skill}") }, other: "Poring".into(), damage, hits: 1, crit: false, stamp: "12:00:00".into() }
    }

    #[test]
    fn a_fight_counts_damage_per_second_and_ends_after_a_quiet_spell() {
        let mut c = Combat::default();
        let t0 = Instant::now();
        c.record(hit(Some(Source::Me), 0, 100), t0);
        c.record(hit(Some(Source::Me), 19, 300), t0 + Duration::from_secs(2));
        c.record(hit(None, 0, 25), t0 + Duration::from_secs(4));
        let (going, secs, dealt, dps, taken) = c.fight_summary(t0 + Duration::from_secs(4)).unwrap();
        assert!(going);
        assert_eq!((secs, dealt, dps, taken), (4.0, 400, 100.0, 25));
        // Quiet: the fight ends where its last hit was.
        let (going, secs, ..) = c.fight_summary(t0 + Duration::from_secs(30)).unwrap();
        assert!(!going);
        assert_eq!(secs, 4.0);
        // The next hit starts a new fight; the session keeps counting.
        c.record(hit(Some(Source::Me), 0, 50), t0 + Duration::from_secs(40));
        let (_, _, dealt, _, taken) = c.fight_summary(t0 + Duration::from_secs(40)).unwrap();
        assert_eq!((dealt, taken), (50, 0));
        assert_eq!((c.dealt, c.taken), (450, 25));
    }

    #[test]
    fn the_table_sums_by_skill_most_first_and_counts_misses() {
        let mut c = Combat::default();
        let t = Instant::now();
        c.record(hit(Some(Source::Me), 0, 100), t);
        c.record(hit(Some(Source::Me), 0, 0), t);
        c.record(hit(Some(Source::Me), 19, 900), t);
        c.record(hit(Some(Source::Homunculus), 8013, 50), t);
        let rows = c.table();
        assert_eq!(rows[0].1, "Skill 19");
        assert_eq!((rows[1].1, rows[1].2.hits, rows[1].2.damage, rows[1].2.misses), ("Attack", 2, 100, 1));
        assert_eq!(rows[2].0, Source::Homunculus);
        c.reset();
        assert!(c.table().is_empty() && c.fight.is_none() && c.log.is_empty());
    }

    #[test]
    fn numbers_read_well() {
        assert_eq!(group(0), "0");
        assert_eq!(group(999), "999");
        assert_eq!(group(1000), "1,000");
        assert_eq!(group(-1234567), "-1,234,567");
        assert_eq!(clock(83.4), "1:23");
        let (text, _) = line(&Hit { hits: 3, crit: false, ..hit(Some(Source::Me), 0, 1500) });
        assert_eq!(text, "Attack → Poring: 1,500 (3 hits)");
        let (text, _) = line(&hit(None, 0, 0));
        assert_eq!(text, "Poring (Attack) → you: Miss");
    }
}

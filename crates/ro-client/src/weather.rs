//! Weather: what the server turns on (its snow, sakura, leaves, clouds and fog mapflags, and
//! GM @snow and the like, sent as special effects on entering a map) and, if Settings asks,
//! ambient weather by climate: snow on snowy maps, petals where cherry trees grow, leaves in
//! forests, fog in Niflheim, and now and then a shower of rain. Whenever a map's weather
//! changes the server sends all that is still on, one effect straight after another: each
//! such run is the whole of it. Turning the last kind off sends nothing (the original keeps
//! it until the map changes); the GM who did it is told so, and that clears it for them.

use std::collections::HashMap;

use ro_render::{WeatherKind, WeatherLayer};
use serde::{Deserialize, Serialize};

/// Settings > Graphics > Weather.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum WeatherMode {
    Off,
    /// The server's weather alone, as the original.
    Server,
    /// The server's, and the client's own by climate.
    #[default]
    Ambient,
}

impl WeatherMode {
    pub const ALL: [WeatherMode; 3] = [WeatherMode::Off, WeatherMode::Server, WeatherMode::Ambient];

    pub fn name(self) -> &'static str {
        match self {
            WeatherMode::Off => "Off",
            WeatherMode::Server => "Server's only",
            WeatherMode::Ambient => "Server's and ambient",
        }
    }
}

/// What the server can turn on: weather the renderer draws in layers, or fireworks (set off
/// here, one after another, while they are on).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sky {
    Layer(WeatherKind),
    Fireworks,
}

/// The weather a special effect turns on (rAthena's clif_weather_check, plus EF_RAIN).
pub fn from_effect(effect: u32) -> Option<Sky> {
    use Sky::Layer;
    Some(match effect {
        161 => Layer(WeatherKind::Rain),         // EF_RAIN
        162 => Layer(WeatherKind::Snow),         // EF_SNOW (snow)
        163 => Layer(WeatherKind::Petals),       // EF_SAKURA (sakura)
        333 => Layer(WeatherKind::Leaves),       // EF_MAPLE (leaves)
        233 | 516 => Layer(WeatherKind::Clouds), // EF_CLOUD3 (clouds), EF_CLOUD5 (clouds2)
        515 => Layer(WeatherKind::Fog),          // EF_CLOUD4 (fog)
        // EF_POKJUK, EF_THROWITEM2, EF_POKJUK_SOUND (fireworks)
        297 | 299 | 301 => Sky::Fireworks,
        _ => return None,
    })
}

/// The weather the server's message says has stopped (rAthena's msg 1201..1215), or Some(None)
/// for all of it (@clearweather, msg 291).
pub fn stopped(text: &str) -> Option<Option<Sky>> {
    use Sky::Layer;
    Some(Some(match text.trim() {
        "Snow has stopped falling." => Layer(WeatherKind::Snow),
        "Cherry tree leaves no longer fall." => Layer(WeatherKind::Petals),
        "The clouds has disappear." | "The alternative clouds disappear." => Layer(WeatherKind::Clouds),
        "The fog has gone." => Layer(WeatherKind::Fog),
        "Leaves no longer fall." => Layer(WeatherKind::Leaves),
        "The rain has stopped." => Layer(WeatherKind::Rain),
        "Fireworks have ended." => Sky::Fireworks,
        "Weather effects will dispell on warp/refresh" => return Some(None),
        _ => return None,
    }))
}

/// Ambient weather for a map's climate (its sky profile's name) at `unix_secs`: what is on
/// and how strongly.
pub fn ambient(climate: &str, map: &str, unix_secs: u64) -> Vec<(WeatherKind, f32)> {
    let mut out = match climate {
        "snow" => vec![(WeatherKind::Snow, 0.55)],
        "blossom" => vec![(WeatherKind::Petals, 0.6)],
        "forest" => vec![(WeatherKind::Leaves, 0.35)],
        "gothic" => vec![(WeatherKind::Fog, 0.7)],
        _ => vec![],
    };
    if matches!(climate, "temperate" | "coastal" | "forest" | "highland") && showering(map, unix_secs) {
        out.push((WeatherKind::Rain, 0.85));
    }
    out
}

/// Showers come and go in twenty-minute spells, about one spell in six, each map on its own
/// schedule (the same for everyone, as it follows the clock).
fn showering(map: &str, unix_secs: u64) -> bool {
    const SPELL_SECS: u64 = 20 * 60;
    let spell = unix_secs / SPELL_SECS;
    // FNV-1a over the map name and the spell.
    let h = map.bytes().chain(spell.to_le_bytes()).fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3));
    h % 6 == 0
}

#[derive(Default)]
pub struct Weather {
    /// The server's weather on this map.
    server: Vec<Sky>,
    /// The last thing the server sent was one of its weather effects: a run is going on (the
    /// next effect joins it, and a message straight after it was already answered by it).
    in_run: bool,
    /// `--weather snow,fog`: shown on every map (captures).
    forced: Vec<Sky>,
    /// Fireworks: until the next goes up (ms), the bangs still to come (ms until each), and
    /// the dice for when and where.
    next_launch_ms: f32,
    bangs: Vec<f32>,
    dice: u32,
    /// Thunderstorms: until the next strike (ms), how long since the last (ms; its flash
    /// flickers out within a second), and the thunder still to come (ms until each).
    next_strike_ms: f32,
    since_strike_ms: Option<f32>,
    /// Fireflies: until the next swarm comes out (ms).
    next_swarm_ms: f32,
    flicker: [f32; 3],
    thunder: Vec<f32>,
    /// Each kind's strength as shown, eased toward what is wanted.
    shown: HashMap<WeatherKind, f32>,
    /// Each kind's texture once loaded (and how many pictures it holds side by side).
    textures: HashMap<WeatherKind, (ro_render::TexId, u32)>,
}

impl Weather {
    /// With `forced` weather on every map (`--weather`).
    pub fn with_forced(forced: Vec<Sky>) -> Self {
        Self { forced, ..Default::default() }
    }

    /// A new map: the server says again what its weather is.
    pub fn clear_server(&mut self) {
        self.server.clear();
        self.in_run = false;
        self.shown.clear();
    }

    /// Anything else the server sent: a run of weather effects is over.
    pub fn server_other(&mut self) {
        self.in_run = false;
    }

    /// One of the server's weather effects: part of the set still on.
    pub fn server_on(&mut self, kind: Sky) {
        if !self.in_run && !self.server.is_empty() {
            tracing::info!(was = ?self.server, "weather set changes");
            self.server.clear();
        }
        self.in_run = true;
        if !self.server.contains(&kind) {
            tracing::info!(?kind, "weather");
            self.server.push(kind);
        }
    }

    /// A server message: clears what it says has stopped, unless the run of effects right
    /// before it said what is still on (clouds and clouds2 are both clouds here).
    pub fn server_said(&mut self, text: &str) {
        let after_run = std::mem::take(&mut self.in_run);
        let Some(gone) = stopped(text) else { return };
        if after_run {
            return;
        }
        tracing::info!(?gone, "weather stopped");
        self.server.retain(|k| gone.is_some_and(|g| g != *k));
    }

    /// Eases the weather toward what is wanted and hands it to the renderer. `climate` is the
    /// map's sky profile (None indoors and underground, where no ambient weather falls).
    pub fn update(&mut self, dt_ms: f32, mode: WeatherMode, map: &str, climate: Option<&str>, r: &mut ro_render::Renderer, vfs: &ro_vfs::Vfs) {
        let mut want: Vec<(WeatherKind, f32)> = vec![];
        if mode != WeatherMode::Off {
            want.extend(self.server.iter().chain(&self.forced).filter_map(|s| match s {
                Sky::Layer(k) => Some((*k, 1.0)),
                Sky::Fireworks => None,
            }));
        }
        if let (WeatherMode::Ambient, Some(climate)) = (mode, climate) {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            for (kind, s) in ambient(climate, map, now) {
                if !want.iter().any(|(k, _)| *k == kind) {
                    want.push((kind, s));
                }
            }
        }
        // About eight seconds to come in or clear.
        let step = dt_ms / 8000.0;
        for (kind, target) in &want {
            let s = self.shown.entry(*kind).or_insert(0.0);
            *s = if *s < *target { (*s + step).min(*target) } else { (*s - step).max(*target) };
        }
        for (kind, s) in self.shown.iter_mut() {
            if !want.iter().any(|(k, _)| k == kind) {
                *s -= step;
            }
        }
        self.shown.retain(|_, s| *s > 0.0);
        r.weather.clear();
        // A steady order, so layers don't swap places from frame to frame.
        let mut kinds: Vec<(WeatherKind, f32)> = self.shown.iter().map(|(k, s)| (*k, *s)).collect();
        kinds.sort_by_key(|(k, _)| *k as u8);
        for (kind, strength) in kinds {
            let (tex, frames) = *self.textures.entry(kind).or_insert_with(|| texture(kind, r, vfs));
            r.weather.push(WeatherLayer { kind, tex, frames, strength });
        }
    }
}

impl Weather {
    /// A steady random number in 0..1.
    fn roll(&mut self) -> f32 {
        if self.dice == 0 {
            self.dice = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.subsec_nanos() | 1);
        }
        self.dice = self.dice.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.dice >> 8) as f32 / 16_777_216.0
    }

    /// Fireworks over `dt_ms`: how many go up now, and how many bang (each as its rocket
    /// bursts). While they are on one goes up every half second to two seconds, now and then
    /// a quick volley.
    pub fn fireworks(&mut self, dt_ms: f32, mode: WeatherMode) -> (Vec<Launch>, u32) {
        for b in &mut self.bangs {
            *b -= dt_ms;
        }
        let bangs = self.bangs.iter().filter(|b| **b <= 0.0).count() as u32;
        self.bangs.retain(|b| *b > 0.0);
        let on = mode != WeatherMode::Off && self.server.iter().chain(&self.forced).any(|s| *s == Sky::Fireworks);
        if !on {
            self.next_launch_ms = 0.0;
            return (vec![], bangs);
        }
        // Back from a long pause (a minimised window): no pile of rockets at once.
        self.next_launch_ms = (self.next_launch_ms - dt_ms).max(-500.0);
        let mut launches = vec![];
        while self.next_launch_ms <= 0.0 {
            let launch = Launch { across: self.roll() * 2.0 - 1.0, depth: self.roll() * 2.0 - 1.0, rise: 9.0 + self.roll() * 5.0, colour: self.roll(), seed: self.dice };
            launches.push(launch);
            self.bangs.push(ro_render::FIREWORK_BURST_MS);
            self.next_launch_ms += if self.roll() < 0.2 { 120.0 + self.roll() * 200.0 } else { 500.0 + self.roll() * 1500.0 };
        }
        (launches, bangs)
    }
}

impl Weather {
    /// A thunderstorm over `dt_ms`: heavy rain (shown at 0.6 or more, past its easing in)
    /// brings a strike every 12 to 45 seconds. The flash's brightness now (0 none .. about
    /// 1, two or three quick flickers), and how many thunderclaps sound now (each 0.4 to 2.5
    /// seconds after its flash, as far off as that).
    pub fn storm(&mut self, dt_ms: f32) -> (f32, u32) {
        for t in &mut self.thunder {
            *t -= dt_ms;
        }
        let claps = self.thunder.iter().filter(|t| **t <= 0.0).count() as u32;
        self.thunder.retain(|t| *t > 0.0);
        let rain = self.shown.get(&WeatherKind::Rain).copied().unwrap_or(0.0);
        if rain < 0.6 {
            // Not storming: the first strike comes a little while after it gets heavy.
            self.next_strike_ms = 6000.0 + self.roll() * 10000.0;
        } else {
            self.next_strike_ms -= dt_ms;
            if self.next_strike_ms <= 0.0 {
                self.since_strike_ms = Some(0.0);
                let (second, third) = (0.25 + self.roll() * 0.5, self.roll() * 0.9);
                self.flicker = [1.0, second, third];
                let far = 400.0 + self.roll() * 2100.0;
                self.thunder.push(far);
                self.next_strike_ms = 12_000.0 + self.roll() * 33_000.0;
                tracing::debug!("lightning");
            }
        }
        let flash = match self.since_strike_ms.as_mut() {
            Some(t) => {
                let at = *t;
                *t += dt_ms;
                if at > 1000.0 {
                    self.since_strike_ms = None;
                }
                flash_at(at, self.flicker)
            }
            None => 0.0,
        };
        (flash, claps)
    }
}

impl Weather {
    /// Fireflies over `dt_ms`: on green maps (forests, fields, coasts, highlands, blossom)
    /// once night has fallen (`night` 0..1), unless it rains: a swarm every few seconds,
    /// somewhere within view. Where to set one loose (cells from the focus), if now.
    pub fn fireflies(&mut self, dt_ms: f32, climate: Option<&str>, night: f32) -> Option<glam::Vec2> {
        let green = matches!(climate, Some("forest" | "temperate" | "blossom" | "highland" | "coastal"));
        let rain = self.shown.get(&WeatherKind::Rain).copied().unwrap_or(0.0);
        if !green || night < 0.6 || rain > 0.3 {
            self.next_swarm_ms = 0.0;
            return None;
        }
        self.next_swarm_ms -= dt_ms;
        if self.next_swarm_ms > 0.0 {
            return None;
        }
        self.next_swarm_ms = 1800.0 + self.roll() * 2400.0;
        Some(glam::Vec2::new(self.roll() * 2.0 - 1.0, self.roll() * 2.0 - 1.0) * glam::Vec2::new(16.0, 9.0))
    }
}

/// A lightning flash `ms` after the strike: flickers at 0, 90 and 230 ms of the given
/// strengths, each dying away in about a tenth of a second.
fn flash_at(ms: f32, flicker: [f32; 3]) -> f32 {
    [0.0, 90.0, 230.0].iter().zip(flicker).map(|(at, k)| if ms >= *at { k * (-(ms - at) / 70.0).exp() } else { 0.0 }).sum()
}

/// One firework going up: where across the screen (-1..1) and how far into it (-1..1, from
/// near to far), how high it bursts (cells), its colour (0..1 picks one) and a seed.
#[derive(Clone, Copy, Debug)]
pub struct Launch {
    pub across: f32,
    pub depth: f32,
    pub rise: f32,
    pub colour: f32,
    pub seed: u32,
}

/// The fireworks' colours.
const FIREWORK_COLOURS: [[f32; 3]; 7] = [[1.0, 0.3, 0.3], [1.0, 0.78, 0.3], [0.4, 1.0, 0.45], [0.4, 0.65, 1.0], [0.85, 0.45, 1.0], [1.0, 0.5, 0.85], [0.95, 0.95, 1.0]];

/// Sets a firework off around the camera's focus: it bursts over a spot within the view
/// (`launch.across` and `depth`), so its rocket goes up from nearer the camera by as much as
/// its height lifts it up the screen.
pub fn set_off(launch: Launch, fx: &mut crate::fx::FxLib, r: &mut ro_render::Renderer, vfs: &ro_vfs::Vfs) {
    use ro_render::{FireworkPart, Shape, CELL};
    if !r.show_effects {
        return;
    }
    let cam = r.camera;
    let (sy, cy) = cam.yaw.sin_cos();
    // Toward the camera along the ground, and across the screen.
    let toward = glam::Vec3::new(sy, 0.0, -cy);
    let across = glam::Vec3::new(cy, 0.0, sy);
    let spot = cam.target + across * launch.across * 14.0 * CELL - toward * launch.depth * 4.0 * CELL;
    let pos = spot + toward * launch.rise * CELL / cam.pitch.tan().max(0.3);
    let colour = FIREWORK_COLOURS[(launch.colour * FIREWORK_COLOURS.len() as f32) as usize % FIREWORK_COLOURS.len()];
    let parts = [
        (FireworkPart::Rocket, fx.fx_texture(r, vfs, crate::fx::FxTexture::Glow)),
        (FireworkPart::Glitter, fx.texture(r, vfs, "pokjuk_b.bmp")),
        (FireworkPart::Twinkle, fx.texture_white(r, vfs, "pokjuk_c.bmp")),
        (FireworkPart::Bloom, fx.texture_white(r, vfs, "pokjuk_a.bmp")),
        (FireworkPart::Spray, fx.texture_white(r, vfs, "pokjuk_d.bmp")),
    ];
    for (part, tex) in parts {
        let Some(tex) = tex else { continue };
        let shape = Shape::Firework { part, seed: launch.seed, rise: launch.rise };
        r.procs.push(ro_render::ProcFx { tex, shape, pos, color: colour, time_ms: 0.0, duration_ms: ro_render::FIREWORK_MS });
    }
}

/// A kind's look: the original's art where the data has it (petals, maple leaves, fog and
/// cloud puffs), else drawn here; snow and rain are always drawn here (the data's snow is
/// sheets of flakes for scrolling, not single flakes).
fn texture(kind: WeatherKind, r: &mut ro_render::Renderer, vfs: &ro_vfs::Vfs) -> (ro_render::TexId, u32) {
    let art = |paths: &[&str]| -> Option<ro_render::Rgba> {
        let imgs: Vec<ro_render::Rgba> = paths
            .iter()
            .filter_map(|p| vfs.read(&format!("data\\texture\\effect\\{p}")).ok().flatten().and_then(|d| ro_formats::image::decode(&d, p).ok()))
            .collect();
        side_by_side(imgs)
    };
    let (img, frames) = match kind {
        WeatherKind::Snow => (flake(), 1),
        WeatherKind::Rain => (streak(), 1),
        WeatherKind::Petals => art(&["efst_blossom_fluttering\\sakura1.bmp"]).map(black_to_alpha).map_or((flake(), 1), |i| (i, 1)),
        WeatherKind::Leaves => art(&["efst_autumn_full_moon\\maple1.tga", "efst_autumn_full_moon\\maple2.tga"]).map_or((flake(), 1), frames),
        WeatherKind::Fog => art(&["fog1.tga", "fog2.tga"]).map_or((puff(), 1), frames),
        WeatherKind::Clouds => art(&["cloud3.tga", "cloud5.tga"]).map_or((puff(), 1), frames),
    };
    (r.add_effect_texture(&img), frames)
}

/// Square pictures side by side: the picture and how many it holds.
fn frames(img: ro_render::Rgba) -> (ro_render::Rgba, u32) {
    let n = (img.width / img.height.max(1)).max(1);
    (img, n)
}

/// Pictures of one size, left to right (the shader picks one per particle). None if none.
fn side_by_side(imgs: Vec<ro_render::Rgba>) -> Option<ro_render::Rgba> {
    let first = imgs.first()?;
    let (w, h) = (first.width, first.height);
    let imgs: Vec<_> = imgs.iter().filter(|i| (i.width, i.height) == (w, h)).collect();
    let n = imgs.len() as u32;
    let mut pixels = vec![0u8; (w * n * h * 4) as usize];
    for (k, img) in imgs.iter().enumerate() {
        for y in 0..h {
            let src = &img.pixels[(y * w * 4) as usize..((y + 1) * w * 4) as usize];
            let at = ((y * w * n + k as u32 * w) * 4) as usize;
            pixels[at..at + src.len()].copy_from_slice(src);
        }
    }
    Some(ro_render::Rgba { width: w * n, height: h, pixels })
}

/// Art on black with no alpha (the sakura petal): black is see-through, the rest solid.
fn black_to_alpha(mut img: ro_render::Rgba) -> ro_render::Rgba {
    for p in img.pixels.chunks_exact_mut(4) {
        let v = p[0].max(p[1]).max(p[2]);
        p[3] = if v < 12 { 0 } else { p[3].min(((v as u32 - 12) * 255 / 40).min(255) as u8) };
    }
    img
}

/// A round flake, soft at its edge.
fn flake() -> ro_render::Rgba {
    soft_disc(32, 2.2)
}

/// A puff for fog and clouds when the data has none: very soft.
fn puff() -> ro_render::Rgba {
    soft_disc(64, 1.2)
}

fn soft_disc(n: u32, sharp: f32) -> ro_render::Rgba {
    let mut pixels = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let (dx, dy) = ((x as f32 + 0.5) / n as f32 * 2.0 - 1.0, (y as f32 + 0.5) / n as f32 * 2.0 - 1.0);
            let a = (1.0 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0).powf(1.0 / sharp);
            pixels.extend([255, 255, 255, (a * 255.0) as u8]);
        }
    }
    ro_render::Rgba { width: n, height: n, pixels }
}

/// A raindrop's streak: brightest in the middle, fading to both ends and its sides.
fn streak() -> ro_render::Rgba {
    let (w, h) = (8u32, 64u32);
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let across = 1.0 - ((x as f32 + 0.5) / w as f32 * 2.0 - 1.0).abs();
            let along = (std::f32::consts::PI * (y as f32 + 0.5) / h as f32).sin();
            pixels.extend([255, 255, 255, (across * along * 255.0) as u8]);
        }
    }
    ro_render::Rgba { width: w, height: h, pixels }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_servers_weather_effects() {
        let layer = |k| Some(Sky::Layer(k));
        assert_eq!(from_effect(162), layer(WeatherKind::Snow));
        assert_eq!(from_effect(163), layer(WeatherKind::Petals));
        assert_eq!(from_effect(333), layer(WeatherKind::Leaves));
        assert_eq!(from_effect(233), layer(WeatherKind::Clouds));
        assert_eq!(from_effect(516), layer(WeatherKind::Clouds));
        assert_eq!(from_effect(515), layer(WeatherKind::Fog));
        assert_eq!(from_effect(297), Some(Sky::Fireworks));
        assert_eq!(from_effect(301), Some(Sky::Fireworks));
        assert_eq!(from_effect(298), None);
    }

    #[test]
    fn fireworks_go_up_while_on_and_bang_as_they_burst() {
        let mut w = Weather::default();
        assert!(w.fireworks(16.0, WeatherMode::Server).0.is_empty());
        w.server_on(Sky::Fireworks);
        // On: one goes up at once, more over the next ten seconds; each bangs as it bursts.
        let (mut up, mut bangs) = (0, 0);
        for _ in 0..625 {
            let (l, b) = w.fireworks(16.0, WeatherMode::Server);
            up += l.len();
            bangs += b;
        }
        assert!((5..=30).contains(&up), "{up} in 10 s");
        assert!(bangs as usize >= up - 2 && bangs as usize <= up, "{bangs} bangs for {up}");
        // Weather off in Settings, or the server's "Fireworks have ended.": none.
        assert!((0..100).all(|_| w.fireworks(16.0, WeatherMode::Off).0.is_empty()));
        w.server_other();
        w.server_said("Fireworks have ended.");
        assert!((0..100).all(|_| w.fireworks(16.0, WeatherMode::Ambient).0.is_empty()));
        // A long pause: a rocket or two, not one for every second missed.
        w.server_on(Sky::Fireworks);
        assert!(w.fireworks(60_000.0, WeatherMode::Server).0.len() <= 2);
    }

    #[test]
    fn each_burst_is_all_the_servers_weather() {
        let mut w = Weather::default();
        // @snow, then @sakura: the second run holds both.
        w.server_on(Sky::Layer(WeatherKind::Snow));
        w.server_said("It has started to snow.");
        w.server_on(Sky::Layer(WeatherKind::Snow));
        w.server_on(Sky::Layer(WeatherKind::Petals));
        w.server_said("Cherry tree leaves have begun to fall.");
        assert_eq!(w.server, [Sky::Layer(WeatherKind::Snow), Sky::Layer(WeatherKind::Petals)]);
        // @sakura off: snow alone is sent again, then the message (which the run overrules).
        w.server_other();
        w.server_on(Sky::Layer(WeatherKind::Snow));
        w.server_said("Cherry tree leaves no longer fall.");
        assert_eq!(w.server, [Sky::Layer(WeatherKind::Snow)]);
        // @snow off: nothing is sent; the message clears it.
        w.server_said("Snow has stopped falling.");
        assert!(w.server.is_empty());
        // Entering a map (its weather, then more), and turning it off straight away.
        w.clear_server();
        w.server_on(Sky::Layer(WeatherKind::Rain));
        w.server_other();
        w.server_said("The rain has stopped.");
        assert!(w.server.is_empty());
        w.server_on(Sky::Layer(WeatherKind::Fog));
        w.server_on(Sky::Layer(WeatherKind::Leaves));
        w.server_other();
        w.server_said("Weather effects will dispell on warp/refresh");
        assert!(w.server.is_empty());
        // Chat that merely says so (with a speaker) is no message from the server.
        assert_eq!(stopped("Hayate : Snow has stopped falling."), None);
    }

    #[test]
    fn heavy_rain_brings_lightning_and_thunder_after_it() {
        let mut w = Weather::default();
        // Light rain: no storm.
        w.shown.insert(WeatherKind::Rain, 0.4);
        assert!((0..2000).all(|_| w.storm(16.0) == (0.0, 0)));
        // Heavy: strikes within the first minute, each flash then its thunder.
        w.shown.insert(WeatherKind::Rain, 1.0);
        let (mut flashes, mut claps, mut bright) = (0, 0, 0.0f32);
        let mut lit = false;
        for _ in 0..(120_000 / 16) {
            let (f, c) = w.storm(16.0);
            if f > 0.0 && !lit {
                flashes += 1;
            }
            lit = f > 0.0;
            bright = bright.max(f);
            claps += c;
        }
        assert!((3..=10).contains(&flashes), "{flashes} strikes in two minutes");
        assert!(claps + 1 >= flashes && claps <= flashes, "{claps} claps for {flashes}");
        assert!(bright > 0.9 && bright < 2.5);
        // The flash is gone within a second.
        assert!(flash_at(1000.0, [1.0, 0.8, 0.9]) < 0.01);
    }

    #[test]
    fn ambient_weather_follows_the_climate() {
        assert_eq!(ambient("snow", "xmas", 0)[0].0, WeatherKind::Snow);
        assert_eq!(ambient("blossom", "amatsu", 0)[0].0, WeatherKind::Petals);
        assert!(ambient("desert", "morocc", 0).is_empty());
        assert!(ambient("volcanic", "thor_v01", 0).is_empty());
        // Showers: some spells but not most, steady through a spell, each map its own.
        let spells: Vec<bool> = (0..600u64).map(|s| showering("prt_fild08", s * 1200)).collect();
        let wet = spells.iter().filter(|w| **w).count();
        assert!((60..150).contains(&wet), "{wet} of 600 spells rain");
        assert_eq!(showering("prt_fild08", 1200 * 7), showering("prt_fild08", 1200 * 7 + 1199));
        assert_ne!((0..600u64).map(|s| showering("payon", s * 1200)).collect::<Vec<_>>(), spells);
    }

    #[test]
    fn pictures_go_side_by_side() {
        let img = |v: u8| ro_render::Rgba { width: 2, height: 1, pixels: vec![v; 8] };
        let both = side_by_side(vec![img(1), img(2)]).unwrap();
        assert_eq!((both.width, both.height), (4, 1));
        assert_eq!(both.pixels, [1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2]);
        assert!(side_by_side(vec![]).is_none());
    }
}

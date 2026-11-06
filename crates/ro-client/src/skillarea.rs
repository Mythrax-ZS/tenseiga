//! What a ground skill will cover, shown on the ground while aiming it (the original shows
//! only the cell). Square areas come from the server's skill_db (skillarea_db.rs, generated
//! by tools/skillareas); the layouts rAthena builds in code are copied from its skill.cpp
//! (skill_init_unit_layout), turned by the direction from caster to target as the server does.

use crate::skillarea_db::{SPECIAL, SQUARES};

/// Cells a skill covers, around the target cell.
#[derive(Debug, PartialEq)]
pub enum Footprint {
    /// Every cell within `radius` (a square `2r + 1` across).
    Square(i32),
    /// These cells (absolute), for layouts that aren't squares.
    Cells(Vec<(i32, i32)>),
}

/// The area `skill` at `level` covers aimed at `target` by a caster on `caster`; None for
/// skills the tables don't know.
/// How far out a skill aimed by a player reaches, in cells from the caster's cell: rAthena
/// measures a player's skills as a circle (battle_check_range -> check_distance_client:
/// in range while floor(sqrt(dx² + dy²) - 0.1) <= range, so straight out it is one cell
/// longer than the range), and never past the 14 cells a unit can see (AREA_SIZE).
pub fn reach(range: u16) -> f32 {
    (range as f32 + 1.1).min(14.0 * std::f32::consts::SQRT_2)
}

/// Whether a cell `d` cells away (dx, dy) is within a skill's `range`, as the server
/// decides it for a player (see `reach`).
pub fn in_reach(range: u16, d: (i32, i32)) -> bool {
    let dist = ((d.0 * d.0 + d.1 * d.1) as f64).sqrt();
    d.0.abs().max(d.1.abs()) <= 14 && ((dist - 0.1).max(0.0) as i64) <= range as i64
}

/// The texture of the reach ring (`Shape::Ring`): a bright line on its outer edge fading
/// inward, constant around it.
pub fn reach_texture() -> ro_render::Rgba {
    const H: u32 = 32;
    let mut pixels = Vec::with_capacity((4 * H * 4) as usize);
    for y in 0..H {
        let v = y as f32 / (H - 1) as f32;
        let k = if v > 0.82 { 255.0 } else { 120.0 * (v / 0.82).powi(3) };
        for _ in 0..4 {
            pixels.extend([k as u8, k as u8, k as u8, 255]);
        }
    }
    ro_render::Rgba { width: 4, height: H, pixels }
}

pub fn footprint(skill: u16, level: u16, caster: (i32, i32), target: (i32, i32)) -> Option<Footprint> {
    if let Some(r) = lookup(SQUARES, skill, level) {
        return Some(Footprint::Square(r as i32));
    }
    let range = lookup(SPECIAL, skill, level)? as i32;
    // skill_get_unit_layout: the server's default (east) when cast on our own cell.
    let dir = if caster == target { 6 } else { direction(target.0 - caster.0, target.1 - caster.1) };
    let shape: Vec<(i32, i32)> = match skill {
        18 => firewall(dir),   // MG_FIREWALL
        87 => icewall(dir),    // WZ_ICEWALL
        70 => SANCTUARY.into(), // PR_SANCTUARY
        79 => MAGNUS.into(),   // PR_MAGNUS
        140 => VENOM_DUST.into(), // AS_VENOMDUST
        _ => return None,
    };
    // Each unit hits what is within its range (skill_unit_timer_sub).
    let mut cells: Vec<(i32, i32)> = shape
        .iter()
        .flat_map(|&(dx, dy)| (-range..=range).flat_map(move |ex| (-range..=range).map(move |ey| (target.0 + dx + ex, target.1 + dy + ey))))
        .collect();
    cells.sort_unstable();
    cells.dedup();
    Some(Footprint::Cells(cells))
}

/// A level's value: the list holds levels 1.., its last repeating.
fn lookup(table: &[(u16, &[u8])], skill: u16, level: u16) -> Option<u8> {
    let (_, levels) = table.iter().find(|(id, _)| *id == skill)?;
    levels.get(level.max(1) as usize - 1).or(levels.last()).copied()
}

/// rAthena's map_calc_dir_xy: 0 north, then anticlockwise (1 north-west .. 7 north-east).
fn direction(dx: i32, dy: i32) -> u8 {
    if dx >= 0 && dy >= 0 {
        if dx >= dy * 3 {
            6
        } else if dx * 3 < dy {
            0
        } else {
            7
        }
    } else if dx >= 0 {
        if dx >= -dy * 3 {
            6
        } else if dx * 3 < -dy {
            4
        } else {
            5
        }
    } else if dy <= 0 {
        if dx * 3 >= dy {
            4
        } else if dx < dy * 3 {
            2
        } else {
            3
        }
    } else if -dx * 3 <= dy {
        0
    } else if -dx > dy * 3 {
        2
    } else {
        1
    }
}

/// Fire Wall: three cells across the line of fire, five on the diagonals.
fn firewall(dir: u8) -> Vec<(i32, i32)> {
    let (dx, dy): (&[i32], &[i32]) = if dir & 1 != 0 {
        if dir & 2 != 0 { (&[-1, -1, 0, 0, 1], &[1, 0, 0, -1, -1]) } else { (&[1, 1, 0, 0, -1], &[1, 0, 0, -1, -1]) }
    } else if dir % 4 == 0 {
        (&[-1, 0, 1], &[0, 0, 0])
    } else {
        (&[0, 0, 0], &[-1, 0, 1])
    };
    dx.iter().copied().zip(dy.iter().copied()).collect()
}

/// Ice Wall: five cells across the line of fire.
fn icewall(dir: u8) -> Vec<(i32, i32)> {
    let (dx, dy): ([i32; 5], [i32; 5]) = if dir & 1 != 0 {
        if dir & 2 != 0 { ([-2, -1, 0, 1, 2], [2, 1, 0, -1, -2]) } else { ([2, 1, 0, -1, -2], [2, 1, 0, -1, -2]) }
    } else if dir % 4 == 0 {
        ([-2, -1, 0, 1, 2], [0; 5])
    } else {
        ([0; 5], [-2, -1, 0, 1, 2])
    };
    dx.into_iter().zip(dy).collect()
}

/// Sanctuary: 5x5 without its corners.
const SANCTUARY: [(i32, i32); 21] = [
    (-1, -2), (0, -2), (1, -2),
    (-2, -1), (-1, -1), (0, -1), (1, -1), (2, -1),
    (-2, 0), (-1, 0), (0, 0), (1, 0), (2, 0),
    (-2, 1), (-1, 1), (0, 1), (1, 1), (2, 1),
    (-1, 2), (0, 2), (1, 2),
];

/// Magnus Exorcismus: a cross, three cells thick, seven across.
const MAGNUS: [(i32, i32); 33] = [
    (-1, -3), (0, -3), (1, -3),
    (-1, -2), (0, -2), (1, -2),
    (-3, -1), (-2, -1), (-1, -1), (0, -1), (1, -1), (2, -1), (3, -1),
    (-3, 0), (-2, 0), (-1, 0), (0, 0), (1, 0), (2, 0), (3, 0),
    (-3, 1), (-2, 1), (-1, 1), (0, 1), (1, 1), (2, 1), (3, 1),
    (-1, 2), (0, 2), (1, 2),
    (-1, 3), (0, 3), (1, 3),
];

/// Venom Dust: a plus.
const VENOM_DUST: [(i32, i32); 5] = [(-1, 0), (0, -1), (0, 0), (0, 1), (1, 0)];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reach_is_the_servers_circle() {
        // Range 9 (Fire Bolt): ten cells straight out, (7, 7) on the diagonal, not (10, 2).
        assert!(in_reach(9, (10, 0)) && in_reach(9, (0, -10)) && in_reach(9, (10, 1)));
        assert!(!in_reach(9, (11, 0)) && !in_reach(9, (10, 2)) && in_reach(9, (7, 7)) && !in_reach(9, (8, 7)));
        // Every cell inside the ring is in reach and every one outside is not.
        for range in [1u16, 3, 9, 14] {
            for dx in -16i32..=16 {
                for dy in -16i32..=16 {
                    let inside = (((dx * dx + dy * dy) as f32).sqrt()) < reach(range);
                    if dx.abs().max(dy.abs()) <= 14 {
                        assert_eq!(inside, in_reach(range, (dx, dy)), "range {range} at ({dx}, {dy})");
                    }
                }
            }
        }
    }

    #[test]
    fn square_areas_follow_the_server_per_level() {
        // Storm Gust: 9x9 units reaching a cell further; Land Protector grows with its level.
        assert_eq!(footprint(89, 10, (0, 0), (5, 5)), Some(Footprint::Square(5)));
        assert_eq!(footprint(288, 1, (0, 0), (5, 5)), Some(Footprint::Square(3)));
        assert_eq!(footprint(288, 5, (0, 0), (5, 5)), Some(Footprint::Square(5)));
        // Levels past the table keep the last; unknown skills have none.
        assert_eq!(footprint(288, 9, (0, 0), (5, 5)), Some(Footprint::Square(5)));
        assert_eq!(footprint(12, 1, (0, 0), (5, 5)), Some(Footprint::Square(0)));
        assert_eq!(footprint(5, 10, (0, 0), (5, 5)), None);
    }

    #[test]
    fn directions_match_the_server() {
        assert_eq!(direction(0, 5), 0);
        assert_eq!(direction(-3, 3), 1);
        assert_eq!(direction(-5, 0), 2);
        assert_eq!(direction(-3, -3), 3);
        assert_eq!(direction(0, -5), 4);
        assert_eq!(direction(3, -3), 5);
        assert_eq!(direction(5, 0), 6);
        assert_eq!(direction(3, 3), 7);
        // Nearly straight still counts as straight (within a third).
        assert_eq!(direction(1, 4), 0);
        assert_eq!(direction(6, 2), 6);
    }

    #[test]
    fn walls_lie_across_the_line_of_fire() {
        // Fire Wall cast northward lies east-west; eastward, north-south.
        assert_eq!(footprint(18, 10, (10, 0), (10, 5)), Some(Footprint::Cells(vec![(9, 5), (10, 5), (11, 5)])));
        assert_eq!(footprint(18, 10, (0, 5), (5, 5)), Some(Footprint::Cells(vec![(5, 4), (5, 5), (5, 6)])));
        // Ice Wall cast north-east runs north-west to south-east.
        let Some(Footprint::Cells(ice)) = footprint(87, 10, (0, 0), (5, 5)) else { panic!() };
        assert_eq!(ice, vec![(3, 7), (4, 6), (5, 5), (6, 4), (7, 3)]);
    }

    #[test]
    fn shaped_areas_reach_their_units_range() {
        // Sanctuary's 21 units, each healing a cell around it: 7x7 without its corner cells.
        let Some(Footprint::Cells(cells)) = footprint(70, 10, (0, 0), (10, 10)) else { panic!() };
        assert_eq!(cells.len(), 7 * 7 - 4);
        assert!(!cells.contains(&(7, 7)) && cells.contains(&(8, 7)));
    }
}

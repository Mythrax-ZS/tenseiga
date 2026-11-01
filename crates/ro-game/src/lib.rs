//! Client-side game state: walking along server-confirmed paths.

pub mod items;
pub mod looks;
pub mod path;
pub mod skills;

/// RO facing, counter-clockwise from south (ACT action order).
pub fn dir_from_step(dx: i32, dy: i32) -> usize {
    match (dx.signum(), dy.signum()) {
        (0, -1) => 0,  // S
        (-1, -1) => 1, // SW
        (-1, 0) => 2,  // W
        (-1, 1) => 3,  // NW
        (0, 1) => 4,   // N
        (1, 1) => 5,   // NE
        (1, 0) => 6,   // E
        (1, -1) => 7,  // SE
        _ => 0,
    }
}

/// Turning in place to look toward `want` (Shift+click, talking to an NPC): the new (head,
/// body), head 0 straight, 1 turned right, 2 left. One step round turns only the head, and
/// a second look the same way brings the body round; two or three steps turn the body to one
/// step short with the head looking the rest; straight behind turns the body all the way
/// (or keeps a turned head turned). roBrowserLegacy's Entity.lookTo.
pub fn turn_toward(body: usize, head: u8, want: usize) -> (u8, usize) {
    // Body placings that leave the head turned left (2) or right (1) onto `want`.
    let (left, right) = ((want + 1) % 8, (want + 7) % 8);
    match (body + 8 - want) % 8 {
        0 => (0, body),
        1 if head == 2 => (0, want),
        7 if head == 1 => (0, want),
        1..=3 => (2, left),
        5..=7 => (1, right),
        _ => match head {
            2 => (2, left),
            1 => (1, right),
            _ => (0, want),
        },
    }
}

/// An actor walking cell to cell. Positions are fractional cells.
#[derive(Debug, Clone)]
pub struct Walker {
    pub cell: (i32, i32),
    pub dir: usize,
    /// Milliseconds per straight cell (the char's speed stat; 150 by default).
    pub speed: f32,
    path: Vec<(i32, i32)>,
    /// Time spent along the current path.
    elapsed: f32,
}

impl Walker {
    pub fn new(cell: (i32, i32), dir: usize, speed: f32) -> Self {
        Self { cell, dir, speed, path: vec![], elapsed: 0.0 }
    }

    pub fn is_walking(&self) -> bool {
        !self.path.is_empty()
    }

    /// The cell being stepped into, if walking.
    pub fn next(&self) -> Option<(i32, i32)> {
        self.path.first().copied()
    }

    /// Steps left on the path (the one in progress included).
    pub fn remaining(&self) -> usize {
        self.path.len()
    }

    /// Keeps the step in progress, then follows `path`, which starts from the cell that step
    /// ends on (standing: from our cell).
    pub fn continue_with(&mut self, path: Vec<(i32, i32)>) {
        match self.path.first().copied() {
            Some(next) => self.path = std::iter::once(next).chain(path).collect(),
            None => self.walk(path),
        }
    }

    /// Where the walk ends, or the current cell when standing.
    pub fn goal_or_cell(&self) -> (i32, i32) {
        self.path.last().copied().unwrap_or(self.cell)
    }

    /// Starts walking along `path` (cells after the current one).
    pub fn walk(&mut self, path: Vec<(i32, i32)>) {
        self.path = path;
        self.elapsed = 0.0;
    }

    /// Follows a new path that starts at `from` without losing the current step: if `from`
    /// is the cell we are leaving or the one we are heading to, the step in progress is
    /// kept and the new path continues from it (no snap back when re-aiming a walk).
    /// Returns false when `from` is elsewhere (the caller should place us there).
    pub fn redirect(&mut self, from: (i32, i32), path: Vec<(i32, i32)>) -> bool {
        match self.path.first().copied() {
            Some(next) if next == from => {
                self.path = std::iter::once(next).chain(path).collect();
                true
            }
            Some(next) if self.cell == from && path.first() == Some(&next) => {
                self.path = path;
                true
            }
            _ if self.cell == from => {
                self.walk(path);
                true
            }
            _ => false,
        }
    }

    /// Stops at a cell (server correction).
    pub fn stop_at(&mut self, cell: (i32, i32)) {
        self.cell = cell;
        self.path.clear();
    }

    fn step_cost(&self, from: (i32, i32), to: (i32, i32)) -> f32 {
        if from.0 != to.0 && from.1 != to.1 { self.speed * 1.4 } else { self.speed }
    }

    /// Advances by `dt_ms`; returns the fractional position.
    pub fn update(&mut self, dt_ms: f32) -> (f32, f32) {
        self.elapsed += dt_ms;
        loop {
            let Some(&next) = self.path.first() else {
                return (self.cell.0 as f32, self.cell.1 as f32);
            };
            let cost = self.step_cost(self.cell, next);
            self.dir = dir_from_step(next.0 - self.cell.0, next.1 - self.cell.1);
            if self.elapsed < cost {
                let t = self.elapsed / cost;
                return (
                    self.cell.0 as f32 + (next.0 - self.cell.0) as f32 * t,
                    self.cell.1 as f32 + (next.1 - self.cell.1) as f32 * t,
                );
            }
            self.elapsed -= cost;
            self.cell = next;
            self.path.remove(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continues_without_losing_the_step() {
        let mut w = Walker::new((0, 0), 0, 100.0);
        w.walk(vec![(1, 1), (2, 2)]);
        // Half way through the first (diagonal, 140 ms) step.
        assert_eq!(w.update(70.0), (0.5, 0.5));
        // Re-aimed from the cell the step ends on: no snap back.
        w.continue_with(vec![(1, 2), (1, 3)]);
        assert_eq!(w.update(0.0), (0.5, 0.5));
        // The step ends on (1, 1); the new path goes on from there.
        let (x, y) = w.update(70.0);
        assert!((x - 1.0).abs() < 1e-3 && (y - 1.0).abs() < 1e-3);
        assert_eq!(w.remaining(), 2);
    }

    #[test]
    fn walks_at_speed() {
        let mut w = Walker::new((0, 0), 0, 100.0);
        w.walk(vec![(1, 0), (2, 0)]);
        assert_eq!(w.update(50.0), (0.5, 0.0));
        assert_eq!(w.dir, 6);
        assert_eq!(w.update(100.0), (1.5, 0.0));
        assert_eq!(w.update(100.0), (2.0, 0.0));
        assert!(!w.is_walking());
    }

    #[test]
    fn turning_looks_then_turns() {
        // Facing south (0): south-west (1) is a look to the right, south-east (7) to the left.
        assert_eq!(turn_toward(0, 0, 1), (1, 0));
        assert_eq!(turn_toward(0, 0, 7), (2, 0));
        // Looking right already, the same look again brings the body round.
        assert_eq!(turn_toward(0, 1, 1), (0, 1));
        // West (two steps): the body goes one step, the head the other.
        assert_eq!(turn_toward(0, 0, 2), (1, 1));
        assert_eq!(turn_toward(0, 0, 6), (2, 7));
        // Straight behind: a full turn, unless the head is turned.
        assert_eq!(turn_toward(0, 0, 4), (0, 4));
        assert_eq!(turn_toward(0, 2, 4), (2, 5));
        // Looking where the body faces straightens the head.
        assert_eq!(turn_toward(3, 1, 3), (0, 3));
    }
}

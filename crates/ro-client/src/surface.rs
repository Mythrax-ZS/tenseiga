//! What the ground is made of, cell by cell, read from the names of its GND textures (RO
//! names them for what they show: 초원 grassland, 흙 soil, 맨땅 bare sand, 눈바닥 snow,
//! 도시 town paving...): where footsteps leave prints and raise dust.

/// A cell's ground.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Surface {
    /// Paving, stone, wood, cliffs: nothing shows.
    #[default]
    Hard,
    Grass,
    /// Soil and dirt paths: dust when dry, muddy prints when wet.
    Dirt,
    /// Sand and desert: dust and prints.
    Sand,
    Snow,
}

/// A ground texture's surface, by its name.
pub fn classify(texture: &str) -> Surface {
    let name = texture.rsplit(['\\', '/']).next().unwrap_or(texture).to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
    if has(&["눈바닥", "snow", "설원"]) {
        Surface::Snow
    } else if has(&["맨땅", "모래", "사막", "sand", "desert"]) {
        Surface::Sand
    } else if has(&["흙", "dirt", "soil", "진흙"]) {
        Surface::Dirt
    } else if has(&["초원", "풀바닥", "숲속바닥", "필드", "grass", "잔디"]) {
        Surface::Grass
    } else {
        Surface::Hard
    }
}

/// The ground of a whole map, per GND cube (two GAT cells across).
#[derive(Default)]
pub struct SurfaceMap {
    width: u32,
    height: u32,
    cubes: Vec<Surface>,
}

impl SurfaceMap {
    pub fn new(gnd: &ro_formats::gnd::Gnd) -> Self {
        let kinds: Vec<Surface> = gnd.textures.iter().map(|t| classify(t)).collect();
        let cubes = gnd
            .cubes
            .iter()
            .map(|c| {
                usize::try_from(c.top)
                    .ok()
                    .and_then(|s| gnd.surfaces.get(s))
                    .and_then(|s| usize::try_from(s.texture).ok())
                    .and_then(|t| kinds.get(t).copied())
                    .unwrap_or_default()
            })
            .collect();
        Self { width: gnd.width, height: gnd.height, cubes }
    }

    /// The ground at a GAT cell.
    pub fn at(&self, x: i32, y: i32) -> Surface {
        let (cx, cy) = (x.div_euclid(2), y.div_euclid(2));
        if cx < 0 || cy < 0 || cx as u32 >= self.width || cy as u32 >= self.height {
            return Surface::Hard;
        }
        self.cubes.get((cy as u32 * self.width + cx as u32) as usize).copied().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ground_textures_say_what_they_are() {
        assert_eq!(classify("필드바닥\\prt_초원01.bmp"), Surface::Grass);
        assert_eq!(classify("필드바닥\\prt_흙03.bmp"), Surface::Dirt);
        assert_eq!(classify("필드바닥\\prt_도시08.bmp"), Surface::Hard);
        assert_eq!(classify("모로코\\moc_맨땅08.bmp"), Surface::Sand);
        assert_eq!(classify("moc_성내02.bmp"), Surface::Hard);
        assert_eq!(classify("크리스마스마을\\xmas_눈바닥01.bmp"), Surface::Snow);
        assert_eq!(classify("xmas_블럭01.bmp"), Surface::Hard);
        assert_eq!(classify("숲속바닥-01.bmp"), Surface::Grass);
        assert_eq!(classify("슈발츠절벽01.bmp"), Surface::Hard);
        assert_eq!(classify("BACKSIDE.BMP"), Surface::Hard);
    }
}

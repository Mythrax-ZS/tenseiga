//! Player job ids (rAthena / pcidentity.lub) -> body sprite name (data\sprite\인간족 or
//! 도람족\몸통) and English title. The original client has this table built in.

/// (job id, sprite name, title)
const JOBS: &[(i16, &str, &str)] = &[
    (0, "초보자", "Novice"),
    (1, "검사", "Swordman"),
    (2, "마법사", "Mage"),
    (3, "궁수", "Archer"),
    (4, "성직자", "Acolyte"),
    (5, "상인", "Merchant"),
    (6, "도둑", "Thief"),
    (7, "기사", "Knight"),
    (8, "프리스트", "Priest"),
    (9, "위저드", "Wizard"),
    (10, "제철공", "Blacksmith"),
    (11, "헌터", "Hunter"),
    (12, "어세신", "Assassin"),
    (13, "페코페코_기사", "Knight"),
    (14, "크루세이더", "Crusader"),
    (15, "몽크", "Monk"),
    (16, "세이지", "Sage"),
    (17, "로그", "Rogue"),
    (18, "연금술사", "Alchemist"),
    (19, "바드", "Bard"),
    (20, "무희", "Dancer"),
    (21, "신페코크루세이더", "Crusader"),
    (22, "결혼", "Wedding"),
    (23, "슈퍼노비스", "Super Novice"),
    (24, "건너", "Gunslinger"),
    (25, "닌자", "Ninja"),
    (26, "산타", "Xmas"),
    (27, "여름", "Summer"),
    (28, "한복", "Hanbok"),
    (29, "옥토버패스트", "Oktoberfest"),
    (30, "여름2", "Summer"),
    (4001, "초보자", "High Novice"),
    (4002, "검사", "High Swordman"),
    (4003, "마법사", "High Mage"),
    (4004, "궁수", "High Archer"),
    (4005, "성직자", "High Acolyte"),
    (4006, "상인", "High Merchant"),
    (4007, "도둑", "High Thief"),
    (4008, "로드나이트", "Lord Knight"),
    (4009, "하이프리", "High Priest"),
    (4010, "하이위저드", "High Wizard"),
    (4011, "화이트스미스", "Whitesmith"),
    (4012, "스나이퍼", "Sniper"),
    (4013, "어쌔신크로스", "Assassin Cross"),
    (4014, "로드페코", "Lord Knight"),
    (4015, "팔라딘", "Paladin"),
    (4016, "챔피온", "Champion"),
    (4017, "프로페서", "Professor"),
    (4018, "스토커", "Stalker"),
    (4019, "크리에이터", "Creator"),
    (4020, "클라운", "Clown"),
    (4021, "집시", "Gypsy"),
    (4022, "페코팔라딘", "Paladin"),
    (4023, "초보자", "Baby Novice"),
    (4024, "검사", "Baby Swordman"),
    (4025, "마법사", "Baby Mage"),
    (4026, "궁수", "Baby Archer"),
    (4027, "성직자", "Baby Acolyte"),
    (4028, "상인", "Baby Merchant"),
    (4029, "도둑", "Baby Thief"),
    (4030, "기사", "Baby Knight"),
    (4031, "프리스트", "Baby Priest"),
    (4032, "위저드", "Baby Wizard"),
    (4033, "제철공", "Baby Blacksmith"),
    (4034, "헌터", "Baby Hunter"),
    (4035, "어세신", "Baby Assassin"),
    (4036, "페코페코_기사", "Baby Knight"),
    (4037, "크루세이더", "Baby Crusader"),
    (4038, "몽크", "Baby Monk"),
    (4039, "세이지", "Baby Sage"),
    (4040, "로그", "Baby Rogue"),
    (4041, "연금술사", "Baby Alchemist"),
    (4042, "바드", "Baby Bard"),
    (4043, "무희", "Baby Dancer"),
    (4044, "신페코크루세이더", "Baby Crusader"),
    (4045, "슈퍼노비스", "Super Baby"),
    (4046, "태권소년", "Taekwon"),
    (4047, "권성", "Star Gladiator"),
    (4048, "권성융합", "Star Gladiator"),
    (4049, "소울링커", "Soul Linker"),
    (4054, "룬나이트", "Rune Knight"),
    (4055, "워록", "Warlock"),
    (4056, "레인져", "Ranger"),
    (4057, "아크비숍", "Arch Bishop"),
    (4058, "미케닉", "Mechanic"),
    (4059, "길로틴크로스", "Guillotine Cross"),
    (4060, "룬나이트", "Rune Knight"),
    (4061, "워록", "Warlock"),
    (4062, "레인져", "Ranger"),
    (4063, "아크비숍", "Arch Bishop"),
    (4064, "미케닉", "Mechanic"),
    (4065, "길로틴크로스", "Guillotine Cross"),
    (4066, "가드", "Royal Guard"),
    (4067, "소서러", "Sorcerer"),
    (4068, "민스트럴", "Minstrel"),
    (4069, "원더러", "Wanderer"),
    (4070, "슈라", "Sura"),
    (4071, "제네릭", "Genetic"),
    (4072, "쉐도우체이서", "Shadow Chaser"),
    (4073, "가드", "Royal Guard"),
    (4074, "소서러", "Sorcerer"),
    (4075, "민스트럴", "Minstrel"),
    (4076, "원더러", "Wanderer"),
    (4077, "슈라", "Sura"),
    (4078, "제네릭", "Genetic"),
    (4079, "쉐도우체이서", "Shadow Chaser"),
    (4080, "룬나이트쁘띠", "Rune Knight"),
    (4081, "룬나이트쁘띠", "Rune Knight"),
    (4082, "그리폰가드", "Royal Guard"),
    (4083, "그리폰가드", "Royal Guard"),
    (4084, "레인져늑대", "Ranger"),
    (4085, "레인져늑대", "Ranger"),
    (4086, "마도기어", "Mechanic"),
    (4087, "마도기어", "Mechanic"),
    (4096, "룬나이트", "Baby Rune Knight"),
    (4097, "워록", "Baby Warlock"),
    (4098, "레인져", "Baby Ranger"),
    (4099, "아크비숍", "Baby Arch Bishop"),
    (4100, "미케닉", "Baby Mechanic"),
    (4101, "길로틴크로스", "Baby Guillotine Cross"),
    (4102, "가드", "Baby Royal Guard"),
    (4103, "소서러", "Baby Sorcerer"),
    (4104, "민스트럴", "Baby Minstrel"),
    (4105, "원더러", "Baby Wanderer"),
    (4106, "슈라", "Baby Sura"),
    (4107, "제네릭", "Baby Genetic"),
    (4108, "쉐도우체이서", "Baby Shadow Chaser"),
    (4190, "슈퍼노비스", "Expanded Super Novice"),
    (4191, "슈퍼노비스", "Expanded Super Baby"),
    (4211, "kagerou", "Kagerou"),
    (4212, "oboro", "Oboro"),
    (4215, "rebellion", "Rebellion"),
    (4218, "summoner", "Summoner"),
    (4220, "summoner", "Baby Summoner"),
    (4222, "닌자", "Baby Ninja"),
    (4223, "kagerou", "Baby Kagerou"),
    (4224, "oboro", "Baby Oboro"),
    (4225, "태권소년", "Baby Taekwon"),
    (4226, "권성", "Baby Star Gladiator"),
    (4227, "소울링커", "Baby Soul Linker"),
    (4228, "건너", "Baby Gunslinger"),
    (4229, "rebellion", "Baby Rebellion"),
    (4239, "성제", "Star Emperor"),
    (4240, "소울리퍼", "Soul Reaper"),
    (4241, "성제", "Baby Star Emperor"),
    (4242, "소울리퍼", "Baby Soul Reaper"),
    (4252, "dragon_knight", "Dragon Knight"),
    (4253, "meister", "Meister"),
    (4254, "shadow_cross", "Shadow Cross"),
    (4255, "arch_mage", "Arch Mage"),
    (4256, "cardinal", "Cardinal"),
    (4257, "windhawk", "Windhawk"),
    (4258, "imperial_guard", "Imperial Guard"),
    (4259, "biolo", "Biolo"),
    (4260, "abyss_chaser", "Abyss Chaser"),
    (4261, "elemetal_master", "Elemental Master"),
    (4262, "inquisitor", "Inquisitor"),
    (4263, "troubadour", "Troubadour"),
    (4264, "trouvere", "Trouvere"),
    (4302, "sky_emperor", "Sky Emperor"),
    (4303, "soul_ascetic", "Soul Ascetic"),
    (4304, "shinkiro", "Shinkiro"),
    (4305, "shiranui", "Shiranui"),
    (4306, "night_watch", "Night Watch"),
    (4307, "hyper_novice", "Hyper Novice"),
    (4308, "spirit_handler", "Spirit Handler"),
    (crate::samurai::JOB, crate::samurai::SPRITE, "Samurai"),
];

/// Body selection and attachment rules shared by world actors and CPU portraits.
pub struct Appearance {
    pub body: String,
    pub attachments: bool,
}

/// The Game Master body (data\sprite\인간족\몸통\{sex}\운영자_{sex}), drawn for GMs in place of their
/// job's (see `App::gm_sprite`).
pub const GM_SPRITE: &str = "운영자";

pub fn appearance(sprite: &str, male: bool) -> Option<Appearance> {
    if sprite == crate::samurai::SPRITE {
        // A body without its head, like official ones: the player's head and headgear attach at
        // its anchors, and clothes dyes are data\palette\몸\samurai_남_N.pal. The Samurai's
        // own tied-back hair is hairstyle 43 (samurai::HAIRSTYLE). This package contains the
        // male artwork only; the job-change NPC enforces it.
        return male.then(|| Appearance { body: crate::samurai::BODY.into(), attachments: true });
    }
    let sex = if male { "남" } else { "여" };
    let race = race_dir(sprite);
    Some(Appearance { body: format!("data\\sprite\\{race}\\몸통\\{sex}\\{sprite}_{sex}"), attachments: true })
}

/// 4th jobs and their mounted forms (Dragon Knight 4252 .. Sky Emperor riding 4316): the jobs
/// with trait stats and AP.
pub fn is_fourth(job: i16) -> bool {
    (4252..=4316).contains(&job)
}

/// Job ids reserved for custom jobs (rAthena's JOB_CUSTOM_START..JOB_CUSTOM_END): the
/// Samurai, then whatever a server lists in System\customjobs.lua.
pub const CUSTOM: std::ops::RangeInclusive<i16> = 4500..=4599;

/// The server's custom jobs (id, sprite name, display name), read once the game data is open.
static CUSTOM_JOBS: std::sync::OnceLock<Vec<(i16, &'static str, &'static str)>> = std::sync::OnceLock::new();

/// Takes the custom jobs a server ships (customdata::custom_jobs). Built-in jobs keep theirs.
pub fn set_custom(jobs: Vec<(i16, String, String)>) {
    let leaked = jobs.into_iter().map(|(id, sprite, name)| (id, &*sprite.leak(), &*name.leak())).collect();
    let _ = CUSTOM_JOBS.set(leaked);
}

fn find(job: i16) -> Option<&'static (i16, &'static str, &'static str)> {
    JOBS.iter().chain(CUSTOM_JOBS.get().into_iter().flatten()).find(|j| j.0 == job)
}

pub fn is_player(job: i16) -> bool {
    job < 45 || (4001..4400).contains(&job) || CUSTOM.contains(&job)
}

pub fn sprite(job: i16) -> &'static str {
    find(job).map_or("초보자", |j| j.1)
}

pub fn title(job: i16) -> &'static str {
    find(job).map_or("Adventurer", |j| j.2)
}

/// A job's body dye: clothes colour `n` (0: the sprite's own colours) is
/// data\palette\몸\{sprite}_{sex}_{n}.pal, Doram's in palette\도람족\body. Servers add palettes
/// by number for any job, custom ones included.
pub fn body_palette(sprite: &str, male: bool, n: u16) -> Option<String> {
    let sex = if male { "남" } else { "여" };
    let dir = if race_dir(sprite) == "도람족" { r"palette\도람족\body" } else { r"palette\몸" };
    (n > 0).then(|| format!(r"data\{dir}\{sprite}_{sex}_{n}.pal"))
}

/// Folder holding a job's weapon sprites. Transcended and mounted variants have their own
/// body but reuse the weapons drawn for the class they came from.
pub fn weapon_dir(sprite: &str) -> &str {
    match sprite {
        "로드나이트" => "기사",
        "로드페코" => "페코페코_기사",
        "하이프리" => "프리스트",
        "하이위저드" => "위저드",
        "화이트스미스" => "제철공",
        "스나이퍼" => "헌터",
        "어쌔신크로스" => "어세신",
        "팔라딘" => "크루세이더",
        "페코팔라딘" => "신페코크루세이더",
        "챔피온" => "몽크",
        "프로페서" => "세이지",
        "스토커" => "로그",
        "크리에이터" => "연금술사",
        "클라운" => "바드",
        "집시" => "무희",
        "권성융합" => "권성",
        "가드" => "로얄가드",
        "그리폰가드" => "신페코로얄가드",
        "룬나이트쁘띠" => "페코페코_룬나이트",
        s => s,
    }
}

/// Doram jobs keep their sprites under 도람족 instead of 인간족.
pub fn race_dir(sprite: &str) -> &'static str {
    if matches!(sprite, "summoner" | "spirit_handler" | "cart_summoner") { "도람족" } else { "인간족" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_palettes_are_named_after_the_job_sprite() {
        assert_eq!(body_palette("어세신", true, 3).as_deref(), Some(r"data\palette\몸\어세신_남_3.pal"));
        assert_eq!(body_palette("summoner", false, 120).as_deref(), Some(r"data\palette\도람족\body\summoner_여_120.pal"));
        assert_eq!(body_palette("samurai", true, 0), None);
    }

    #[test]
    fn the_custom_range_is_players() {
        assert!(is_player(crate::samurai::JOB) && is_player(4599) && !is_player(4600));
        assert_eq!(title(crate::samurai::JOB), "Samurai");
    }
}

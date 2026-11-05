//! Chat time stamps and the chat log (Settings > Game > Chat). Each chat line keeps the local
//! time it arrived; with the log on, lines are also appended to `Chat\<character> <date>.txt`
//! in the working folder (beside ScreenShot\), one file per character per day.

use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, NaiveDate};
use ro_net::ChatKind;

pub struct ChatLog {
    dir: PathBuf,
    character: String,
    day: NaiveDate,
    file: std::fs::File,
    path: PathBuf,
    /// A write failed (said once in the game's log, not every line).
    failed: bool,
}

impl ChatLog {
    /// Today's log for `character` in `dir` (made if missing), appended to.
    pub fn open(dir: &Path, character: &str) -> std::io::Result<Self> {
        Self::open_on(dir, character, Local::now().date_naive())
    }

    fn open_on(dir: &Path, character: &str, day: NaiveDate) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(file_name(character, day));
        let file = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self { dir: dir.to_path_buf(), character: character.to_string(), day, file, path, failed: false })
    }

    pub fn character(&self) -> &str {
        &self.character
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends a line (plain text: item links already named). After midnight it moves on to
    /// the new day's file.
    pub fn write(&mut self, at: DateTime<Local>, kind: ChatKind, text: &str) {
        if at.date_naive() != self.day {
            match Self::open_on(&self.dir, &self.character, at.date_naive()) {
                Ok(next) => *self = next,
                Err(e) => self.fail(&e),
            }
        }
        if let Err(e) = self.file.write_all(line(at, kind, text).as_bytes()) {
            self.fail(&e);
        }
    }

    fn fail(&mut self, e: &std::io::Error) {
        if !std::mem::replace(&mut self.failed, true) {
            tracing::warn!(path = %self.path.display(), "chat log: {e}");
        }
    }
}

/// `<character> <date>.txt`, with what Windows can't have in a file name replaced.
fn file_name(character: &str, day: NaiveDate) -> String {
    let safe: String = character.chars().map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).collect();
    let safe = safe.trim().trim_end_matches('.');
    format!("{} {}.txt", if safe.is_empty() { "chat" } else { safe }, day.format("%Y-%m-%d"))
}

/// One line of the log: the time, the channel (but for ordinary chat), the text.
fn line(at: DateTime<Local>, kind: ChatKind, text: &str) -> String {
    let channel = match kind {
        ChatKind::Public => "",
        ChatKind::Npc => "[NPC] ",
        ChatKind::Whisper => "[Whisper] ",
        ChatKind::Broadcast => "[Broadcast] ",
        ChatKind::System => "[System] ",
        ChatKind::Party => "[Party] ",
        ChatKind::Guild => "[Guild] ",
        ChatKind::Clan => "[Clan] ",
        ChatKind::Battle => "[Battle] ",
    };
    format!("[{}] {channel}{}\n", at.format("%H:%M:%S"), text.replace(['\r', '\n'], " "))
}

/// The time stamp the chat window shows.
pub fn stamp(at: DateTime<Local>) -> String {
    at.format("%H:%M").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(day: u32, h: u32, m: u32, s: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 11, day, h, m, s).unwrap()
    }

    #[test]
    fn lines_have_the_time_and_the_channel() {
        assert_eq!(line(at(5, 9, 7, 3), ChatKind::Public, "Hayate : hi"), "[09:07:03] Hayate : hi\n");
        assert_eq!(line(at(5, 21, 0, 0), ChatKind::Party, "Doofy : on my way\r\nnow"), "[21:00:00] [Party] Doofy : on my way  now\n");
        assert_eq!(stamp(at(5, 21, 4, 59)), "21:04");
    }

    #[test]
    fn file_names_are_safe_on_windows() {
        let day = at(5, 0, 0, 0).date_naive();
        assert_eq!(file_name("Hayate", day), "Hayate 2026-11-05.txt");
        assert_eq!(file_name("a:b/c?", day), "a_b_c_ 2026-11-05.txt");
        assert_eq!(file_name(" .. ", day), "chat 2026-11-05.txt");
    }

    #[test]
    fn the_log_appends_and_moves_on_at_midnight() {
        let dir = std::env::temp_dir().join(format!("tenseiga-chatlog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut log = ChatLog::open_on(&dir, "Hayate", at(5, 0, 0, 0).date_naive()).unwrap();
        log.write(at(5, 23, 59, 58), ChatKind::Public, "one");
        log.write(at(6, 0, 0, 1), ChatKind::Guild, "two");
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap();
        assert_eq!(read("Hayate 2026-11-05.txt"), "[23:59:58] one\n");
        assert_eq!(read("Hayate 2026-11-06.txt"), "[00:00:01] [Guild] two\n");
        // Opened again the same day: appended, not replaced.
        let mut again = ChatLog::open_on(&dir, "Hayate", at(6, 0, 0, 0).date_naive()).unwrap();
        again.write(at(6, 0, 5, 0), ChatKind::Public, "three");
        assert_eq!(read("Hayate 2026-11-06.txt"), "[00:00:01] [Guild] two\n[00:05:00] three\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

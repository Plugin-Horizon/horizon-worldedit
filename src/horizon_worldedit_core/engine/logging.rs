//! `logging.log-commands`, `logging.file` and `logging.format`: one line per WorldEdit command in a file, formatted with a
//! Java `Formatter` pattern as WorldEdit's `LogFormat` does (`%1$tY-%1$tm-%1$td %1$tH:%1$tM:%1$tS %4$s`, `%5$s`, `%6$s`, `%n`).

use super::*;
use std::io::Write;

/// The default pattern (`LogFormat.DEFAULT_FORMAT`).
pub(super) const DEFAULT_FORMAT: &str = "[%1$tY-%1$tm-%1$td %1$tH:%1$tM:%1$tS %4$s]: %5$s%6$s%n";

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

/// One `%t` conversion of the time `ms` (UTC): `None` for a letter this does not know.
fn time_part(c: char, ms: i64) -> Option<String> {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let in_day = secs.rem_euclid(86_400);
    let (y, mo, d) = civil(days);
    let (h, mi, s) = (in_day / 3600, in_day % 3600 / 60, in_day % 60);
    let weekday = (days + 4).rem_euclid(7) as usize;
    let jan1 = civil_days(y, 1, 1);
    Some(match c {
        'Y' => format!("{y:04}"),
        'y' => format!("{:02}", y.rem_euclid(100)),
        'C' => format!("{:02}", y / 100),
        'm' => format!("{mo:02}"),
        'd' => format!("{d:02}"),
        'e' => d.to_string(),
        'B' => MONTHS[mo as usize - 1].to_owned(),
        'b' | 'h' => MONTHS[mo as usize - 1][..3].to_owned(),
        'A' => DAYS[weekday].to_owned(),
        'a' => DAYS[weekday][..3].to_owned(),
        'j' => format!("{:03}", days - jan1 + 1),
        'H' => format!("{h:02}"),
        'k' => h.to_string(),
        'I' => format!("{:02}", if h % 12 == 0 { 12 } else { h % 12 }),
        'l' => (if h % 12 == 0 { 12 } else { h % 12 }).to_string(),
        'M' => format!("{mi:02}"),
        'S' => format!("{s:02}"),
        'L' => format!("{:03}", ms.rem_euclid(1000)),
        'N' => format!("{:09}", ms.rem_euclid(1000) * 1_000_000),
        'p' => (if h < 12 { "am" } else { "pm" }).to_owned(),
        'Z' => "UTC".to_owned(),
        'z' => "+0000".to_owned(),
        's' => secs.to_string(),
        'Q' => ms.to_string(),
        'R' => format!("{h:02}:{mi:02}"),
        'T' => format!("{h:02}:{mi:02}:{s:02}"),
        'r' => format!("{:02}:{mi:02}:{s:02} {}", if h % 12 == 0 { 12 } else { h % 12 }, if h < 12 { "AM" } else { "PM" }),
        'D' => format!("{mo:02}/{d:02}/{:02}", y.rem_euclid(100)),
        'F' => format!("{y:04}-{mo:02}-{d:02}"),
        'c' => format!("{} {} {d:02} {h:02}:{mi:02}:{s:02} UTC {y:04}", &DAYS[weekday][..3], &MONTHS[mo as usize - 1][..3]),
        _ => return None,
    })
}

fn civil_days(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = i64::from(if m > 2 { m - 3 } else { m + 9 });
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `String.format(format, date, source, logger, level, message, thrown)`. `None` for a pattern this does not read (the caller
/// then uses the default one, as `LogFormat` does for a pattern Java rejects).
pub(super) fn java_format(format: &str, ms: i64, args: [&str; 5]) -> Option<String> {
    let chars: Vec<char> = format.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '%' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        i += 1;
        // `%N$`, flags, width.
        let mut index = String::new();
        let mut j = i;
        while j < chars.len() && chars[j].is_ascii_digit() {
            index.push(chars[j]);
            j += 1;
        }
        let arg = if chars.get(j) == Some(&'$') {
            i = j + 1;
            index.parse::<usize>().ok()?
        } else {
            0
        };
        let mut left = false;
        while chars.get(i).is_some_and(|c| matches!(c, '-' | '#' | '+' | ' ' | '0' | ',' | '(')) {
            left |= chars[i] == '-';
            i += 1;
        }
        let mut width = String::new();
        while chars.get(i).is_some_and(char::is_ascii_digit) {
            width.push(chars[i]);
            i += 1;
        }
        let width = width.parse::<usize>().unwrap_or(0);
        let conv = *chars.get(i)?;
        i += 1;
        let text = match conv {
            'n' => "\n".to_owned(),
            '%' => "%".to_owned(),
            't' | 'T' => {
                let part = *chars.get(i)?;
                i += 1;
                if arg != 1 {
                    return None;
                }
                let t = time_part(part, ms)?;
                if conv == 'T' { t.to_uppercase() } else { t }
            }
            's' | 'S' => {
                // Argument 1 is the date: `%1$s` prints it as Java's `LocalDateTime` does.
                let t = match arg {
                    1 => format!("{}T{}", time_part('F', ms)?, time_part('T', ms)?),
                    2..=6 => args[arg - 2].to_owned(),
                    _ => return None,
                };
                if conv == 'S' { t.to_uppercase() } else { t }
            }
            _ => return None,
        };
        let pad = width.saturating_sub(text.chars().count());
        if left {
            out.push_str(&text);
            out.push_str(&" ".repeat(pad));
        } else {
            out.push_str(&" ".repeat(pad));
            out.push_str(&text);
        }
    }
    Some(out)
}

impl Engine {
    /// Appends the line for `command` (the typed text) to `logging.file`, in `logging.format`. Nothing happens unless
    /// `logging.log-commands` is on and the file is named.
    pub(super) fn log_command(&self, world: &dyn World, actor: &Actor, command: &str, env: &Env) {
        if !self.config.log_commands || self.config.log_file.is_empty() {
            return;
        }
        let text = command.trim().strip_prefix('/').unwrap_or(command.trim());
        let called = text.split_whitespace().next().unwrap_or("");
        let message = format!("WorldEdit: {} (in \"{}\"): {called}: {text}", actor.name, world.name());
        let args = ["com.sk89q.worldedit.CommandLog", "com.sk89q.worldedit.CommandLog", "INFO", &message, ""];
        let Some(line) = java_format(&self.config.log_format, env.now_ms, args).or_else(|| java_format(DEFAULT_FORMAT, env.now_ms, args)) else { return };
        let file = Path::new(&self.config.log_file);
        // The file lives in the config folder; a name that climbs out of it is not written.
        if file.is_absolute() || file.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return;
        }
        let Some(dir) = env.config_file.parent() else { return };
        let path = dir.join(file);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_format_writes_a_dated_line() {
        // 2026-11-25 19:52:31 UTC.
        let ms = 1_795_636_351_000;
        let line = java_format(DEFAULT_FORMAT, ms, ["a", "b", "INFO", "WorldEdit: x", ""]).unwrap();
        assert_eq!(line, "[2026-11-25 19:52:31 INFO]: WorldEdit: x\n");
    }

    #[test]
    fn time_conversions_follow_javas_letters() {
        let ms = 1_795_636_351_042;
        assert_eq!(time_part('B', ms).as_deref(), Some("November"));
        assert_eq!(time_part('a', ms).as_deref(), Some("Wed"));
        assert_eq!(time_part('L', ms).as_deref(), Some("042"));
        assert_eq!(time_part('j', ms).as_deref(), Some("329"));
        assert_eq!(time_part('D', ms).as_deref(), Some("11/25/26"));
        assert!(java_format("%q", ms, ["", "", "", "", ""]).is_none());
    }
}

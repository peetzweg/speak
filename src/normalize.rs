//! English text normalisation for things misaki doesn't read naturally on its own: versions,
//! dates, clock times, `#12`, and a few units glued to numbers. Misaki already handles plain
//! numbers, "2,000", money, percentages, ordinals and years, so the output keeps digits for it.

use std::sync::OnceLock;

use regex::{Captures, Regex};

const MONTHS: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).unwrap())
}

fn ordinal(n: u32) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

fn date(year: &str, month: u32, day: u32) -> Option<String> {
    ((1..=12).contains(&month) && (1..=31).contains(&day)).then(|| format!("{} {}, {year}", MONTHS[month as usize - 1], ordinal(day)))
}

/// Normalise `text` for misaki. `british` picks day/month order for ambiguous `1/2/2025` dates.
pub fn english(text: &str, british: bool) -> String {
    static ISO: OnceLock<Regex> = OnceLock::new();
    static SLASH: OnceLock<Regex> = OnceLock::new();
    static DOTTED: OnceLock<Regex> = OnceLock::new();
    static VERSION: OnceLock<Regex> = OnceLock::new();
    static LETTERS: OnceLock<Regex> = OnceLock::new();
    static NUMBER_SIGN: OnceLock<Regex> = OnceLock::new();
    static UNIT: OnceLock<Regex> = OnceLock::new();

    // Letters spelled out with spaces ("crew A P I" in a lexicon) read as one acronym ("API"),
    // so misaki spells them instead of reading "A" as the article and "I" as the pronoun.
    let t = re(&LETTERS, r"\b[A-Z](?: [A-Z]\b)+").replace_all(text, |c: &Captures| c[0].replace(' ', ""));

    // 2025-03-03 → March 3rd, 2025
    let t = re(&ISO, r"\b(\d{4})-(\d{2})-(\d{2})\b").replace_all(&t, |c: &Captures| {
        date(&c[1], c[2].parse().unwrap_or(0), c[3].parse().unwrap_or(0)).unwrap_or_else(|| c[0].to_string())
    });

    // 12/25/2024 (US) or 25/12/2024 (GB; also whenever the first number can't be a month)
    let t = re(&SLASH, r"\b(\d{1,2})/(\d{1,2})/(\d{4})\b").replace_all(&t, |c: &Captures| {
        let (a, b): (u32, u32) = (c[1].parse().unwrap_or(0), c[2].parse().unwrap_or(0));
        let (month, day) = if british || a > 12 { (b, a) } else { (a, b) };
        date(&c[3], month, day).unwrap_or_else(|| c[0].to_string())
    });

    // Dotted triples: 25.12.2024 is a date; 1.2.3 and v0.3.5 are versions; 4 bytes are an address.
    let t = re(&DOTTED, r"\b(v)?(\d+(?:\.\d+)+)\b").replace_all(&t, |c: &Captures| {
        let parts: Vec<&str> = c[2].split('.').collect();
        let v = c.get(1).is_some();
        if !v && parts.len() == 2 {
            return c[0].to_string(); // a decimal number: misaki says "three point one four"
        }
        if !v
            && parts.len() == 3
            && parts[2].len() == 4
            && parts[0].len() <= 2
            && let Some(d) = date(parts[2], parts[1].parse().unwrap_or(0), parts[0].parse().unwrap_or(0))
        {
            return d;
        }
        let is_address = !v && parts.len() == 4 && parts.iter().all(|p| p.parse::<u32>().is_ok_and(|n| n <= 255));
        let joined = parts.join(if is_address { " dot " } else { " point " });
        if v { format!("version {joined}") } else { joined }
    });
    // v2, v10 (a version without dots)
    let t = re(&VERSION, r"\bv(\d+)\b").replace_all(&t, "version $1");

    // 10:30am, 7:05 PM, 14:00, 1:30:15
    let t = replace_times(&t);

    // #12 → number 12
    let t = re(&NUMBER_SIGN, r"(^|[\s(\[])#(\d+)\b").replace_all(&t, "${1}number $2");

    // 3.5s, 120ms, 5 min, 1.2GB, 2x, 10k
    re(&UNIT, r"\b(\d+(?:\.\d+)?)( ?)(ms|s|secs?|mins?|hrs?|h|[KMGT]i?B|kB|x|k)\b")
        .replace_all(&t, |c: &Captures| {
            let (n, space, unit) = (&c[1], &c[2], &c[3]);
            let one = n == "1";
            let word = match unit {
                // Bare "s" also ends decades ("the 1990s", "80s"): only read it as seconds after a
                // decimal or a number that can't be a decade.
                "s" if !space.is_empty() || (!n.contains('.') && n.ends_with('0')) => return c[0].to_string(),
                "h" | "x" | "k" if !space.is_empty() => return c[0].to_string(),
                "ms" => "millisecond",
                "s" | "sec" | "secs" => "second",
                "min" | "mins" => "minute",
                "h" | "hr" | "hrs" => "hour",
                "kB" | "KB" => "kilobyte",
                "MB" => "megabyte",
                "GB" => "gigabyte",
                "TB" => "terabyte",
                "KiB" => "kibibyte",
                "MiB" => "mebibyte",
                "GiB" => "gibibyte",
                "TiB" => "tebibyte",
                "x" => return format!("{n} times"),
                "k" => return format!("{n} thousand"),
                _ => return c[0].to_string(),
            };
            format!("{n} {word}{}", if one { "" } else { "s" })
        })
        .into_owned()
}

/// Clock times: "10:30am" → "10 30 AM", "7:05" → "7 oh 5", "14:00" → "14 hundred".
fn replace_times(text: &str) -> String {
    static TIME: OnceLock<Regex> = OnceLock::new();
    let re = re(&TIME, r"\b(\d{1,2}):(\d{2})(?::(\d{2}))?(?:\s?([AaPp])(\.?)[Mm]\b(\.?))?");
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for c in re.captures_iter(text) {
        let m = c.get(0).unwrap();
        let (h, min): (u32, u32) = (c[1].parse().unwrap_or(99), c[2].parse().unwrap_or(99));
        let secs: u32 = c.get(3).map_or(0, |s| s.as_str().parse().unwrap_or(99));
        let meridiem = c.get(4).map(|x| if x.as_str().eq_ignore_ascii_case("a") { "AM" } else { "PM" });
        if h > 23 || min > 59 || secs > 59 || (meridiem.is_some() && h > 12) {
            continue;
        }
        let mut spoken = match (min, meridiem) {
            (0, Some(_)) => h.to_string(),
            (0, None) if h > 12 => format!("{h} hundred"),
            (0, None) => format!("{h} o'clock"),
            (1..=9, _) => format!("{h} oh {min}"),
            _ => format!("{h} {min}"),
        };
        if secs > 0 {
            spoken.push_str(&format!(" and {secs} second{}", if secs == 1 { "" } else { "s" }));
        }
        if let Some(mer) = meridiem {
            spoken.push(' ');
            spoken.push_str(mer);
        }
        // The final dot of "a.m." also ends the sentence, unless the sentence carries on.
        let dotted = c.get(5).is_some_and(|d| !d.is_empty());
        let trailing = c.get(6).is_some_and(|d| !d.is_empty());
        let rest = &text[m.end()..];
        let carries_on = rest.starts_with(|ch: char| ch.is_whitespace()) && rest.trim_start().starts_with(|ch: char| ch.is_lowercase());
        if trailing && !(dotted && carries_on) {
            spoken.push('.');
        }
        out.push_str(&text[last..m.start()]);
        out.push_str(&spoken);
        last = m.end();
    }
    out.push_str(&text[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn us(t: &str) -> String {
        english(t, false)
    }

    #[test]
    fn versions() {
        assert_eq!(us("KruAPI shipped v0.3.5 on Tuesday."), "KruAPI shipped version 0 point 3 point 5 on Tuesday.");
        assert_eq!(us("node 20.11.0, v2 and 1.2.3.4.5"), "node 20 point 11 point 0, version 2 and 1 point 2 point 3 point 4 point 5");
        assert_eq!(us("pi is 3.14159"), "pi is 3.14159");
        assert_eq!(us("ping 192.168.1.10"), "ping 192 dot 168 dot 1 dot 10");
        assert_eq!(us("dev2 and V8"), "dev2 and V8");
    }

    #[test]
    fn dates() {
        assert_eq!(us("on 2025-03-03."), "on March 3rd, 2025.");
        assert_eq!(us("on 12/25/2024 and 2/1/2025"), "on December 25th, 2024 and February 1st, 2025");
        assert_eq!(english("on 2/1/2025", true), "on January 2nd, 2025");
        assert_eq!(us("on 25/12/2024 or 25.12.2024"), "on December 25th, 2024 or December 25th, 2024");
        assert_eq!(us("not 2025-13-45 or 13/13/2025"), "not 2025-13-45 or 13/13/2025");
        assert_eq!(ordinal(11), "11th");
        assert_eq!(ordinal(22), "22nd");
        assert_eq!(ordinal(103), "103rd");
    }

    #[test]
    fn times() {
        assert_eq!(us("at 10:30am"), "at 10 30 AM");
        assert_eq!(us("at 7:05 PM or 9:00 a.m."), "at 7 oh 5 PM or 9 AM.");
        assert_eq!(us("at 10:30am. Then 9 a.m. sharp, 8:15 p.m. tomorrow."), "at 10 30 AM. Then 9 a.m. sharp, 8 15 PM tomorrow.");
        assert_eq!(us("14:00, 10:00 and 23:45"), "14 hundred, 10 o'clock and 23 45");
        assert_eq!(us("took 1:30:15"), "took 1 30 and 15 seconds");
        assert_eq!(us("ratio 16:9, John 3:16, 25:00"), "ratio 16:9, John 3 16, 25:00");
    }

    #[test]
    fn spaced_letters_become_acronyms() {
        assert_eq!(us("crew A P I host, dot J S"), "crew API host, dot JS");
        assert_eq!(us("I said A or B"), "I said A or B");
        assert_eq!(us("Plan B C"), "Plan BC");
    }

    #[test]
    fn number_sign_and_units() {
        assert_eq!(us("PR #123 (#4) C#5"), "PR number 123 (number 4) C#5");
        assert_eq!(us("passed in 3.5s, 120ms, 1s and 12s"), "passed in 3.5 seconds, 120 milliseconds, 1 second and 12 seconds");
        assert_eq!(us("the 1990s and 80s"), "the 1990s and 80s");
        assert_eq!(
            us("5 min, 1.2GB, 512 MB, 2x faster, 10k users"),
            "5 minutes, 1.2 gigabytes, 512 megabytes, 2 times faster, 10 thousand users"
        );
        assert_eq!(us("box 2 x 3, 5 s"), "box 2 x 3, 5 s");
    }
}

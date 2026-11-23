//! Dynamic variables: `{{$name}}` placeholders that produce a fresh value every time they
//! are resolved, with Postman's names as the compatibility target (`$guid`, `$timestamp`,
//! `$randomInt`, ...). They are a fallback only: a user variable with the same name wins
//! (see [`crate::Variables::resolve`]). Values come from small built-in word lists and the
//! OS random source behind `uuid`, so there is no faker dependency.

use std::time::{SystemTime, UNIX_EPOCH};

/// Every supported dynamic name with a one-line description of its format.
pub const DYNAMIC_VARIABLES: &[(&str, &str)] = &[
    ("$guid", "a v4 UUID"),
    ("$uuid", "a v4 UUID"),
    ("$randomUUID", "a v4 UUID"),
    ("$timestamp", "Unix time in seconds"),
    ("$isoTimestamp", "current UTC time, RFC 3339"),
    ("$randomInt", "an integer from 0 to 1000"),
    ("$randomAlphaNumeric", "one random letter or digit"),
    ("$randomBoolean", "true or false"),
    ("$randomFirstName", "a first name"),
    ("$randomLastName", "a last name"),
    ("$randomEmail", "an example.com email address"),
    ("$randomColor", "a colour name"),
    ("$randomIP", "an IPv4 address"),
];

const FIRST_NAMES: &[&str] = &[
    "Ada",
    "Alan",
    "Grace",
    "Linus",
    "Margaret",
    "Dennis",
    "Barbara",
    "Ken",
    "Frances",
    "Edsger",
    "Radia",
    "Tim",
    "Hedy",
    "John",
    "Katherine",
    "Niklaus",
    "Sophie",
    "Guido",
    "Joan",
    "Bjarne",
];

const LAST_NAMES: &[&str] = &[
    "Lovelace",
    "Turing",
    "Hopper",
    "Torvalds",
    "Hamilton",
    "Ritchie",
    "Liskov",
    "Thompson",
    "Allen",
    "Dijkstra",
    "Perlman",
    "Berners-Lee",
    "Lamarr",
    "Backus",
    "Johnson",
    "Wirth",
    "Wilson",
    "Rossum",
    "Clarke",
    "Stroustrup",
];

const COLORS: &[&str] = &[
    "red", "orange", "yellow", "green", "blue", "indigo", "violet", "purple", "pink", "brown",
    "black", "white", "gray", "cyan", "magenta", "teal", "lime", "maroon", "navy", "olive",
];

const ALNUM: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

/// Whether `name` (without braces) is a known dynamic variable.
pub fn is_dynamic(name: &str) -> bool {
    DYNAMIC_VARIABLES.iter().any(|(n, _)| *n == name)
}

/// 64 random bits from the OS source (via `uuid`'s v4 generator).
fn random_u64() -> u64 {
    uuid::Uuid::new_v4().as_u128() as u64
}

/// A uniform-enough pick in `0..n` (`n` is tiny, so modulo bias is negligible).
fn below(n: usize) -> usize {
    (random_u64() % n as u64) as usize
}

fn pick<'a>(list: &[&'a str]) -> &'a str {
    list[below(list.len())]
}

/// A fresh value for the dynamic variable `name`, or `None` if it is not one.
pub fn dynamic_value(name: &str) -> Option<String> {
    let v = match name {
        "$guid" | "$uuid" | "$randomUUID" => uuid::Uuid::new_v4().to_string(),
        "$timestamp" => unix_now().to_string(),
        "$isoTimestamp" => rfc3339_utc(SystemTime::now()),
        "$randomInt" => (random_u64() % 1001).to_string(),
        "$randomAlphaNumeric" => (ALNUM[below(ALNUM.len())] as char).to_string(),
        "$randomBoolean" => (random_u64() & 1 == 1).to_string(),
        "$randomFirstName" => pick(FIRST_NAMES).to_string(),
        "$randomLastName" => pick(LAST_NAMES).to_string(),
        "$randomEmail" => format!(
            "{}.{}{}@example.com",
            pick(FIRST_NAMES).to_ascii_lowercase(),
            pick(LAST_NAMES).to_ascii_lowercase(),
            below(100)
        ),
        "$randomColor" => pick(COLORS).to_string(),
        "$randomIP" => {
            let r = random_u64();
            // First octet 1..=254, so the address is never 0.x or 255.x.
            format!(
                "{}.{}.{}.{}",
                1 + (r & 0xff) % 254,
                (r >> 8) & 0xff,
                (r >> 16) & 0xff,
                (r >> 24) & 0xff
            )
        }
        _ => return None,
    };
    Some(v)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `t` as RFC 3339 UTC with milliseconds, e.g. `2026-11-23T09:41:07.123Z` (Postman's format).
pub fn rfc3339_utc(t: SystemTime) -> String {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs();
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (y, m, day) = civil_from_days(days as i64);
    format!(
        "{y:04}-{m:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60,
        d.subsec_millis()
    )
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian (H. Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
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

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;
    use std::time::Duration;

    fn check(name: &str, pattern: &str) {
        let re = Regex::new(pattern).unwrap();
        for _ in 0..50 {
            let v = dynamic_value(name).unwrap();
            assert!(
                re.is_match(&v),
                "{name} produced {v:?}, expected /{pattern}/"
            );
        }
    }

    const UUID: &str = r"^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$";

    #[test]
    fn formats() {
        check("$guid", UUID);
        check("$uuid", UUID);
        check("$randomUUID", UUID);
        check("$timestamp", r"^\d{10}$");
        check(
            "$isoTimestamp",
            r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$",
        );
        check("$randomInt", r"^\d{1,4}$");
        check("$randomAlphaNumeric", r"^[a-z0-9]$");
        check("$randomBoolean", r"^(true|false)$");
        check("$randomFirstName", r"^[A-Z][a-z]+$");
        check("$randomLastName", r"^[A-Z][A-Za-z-]+$");
        check("$randomEmail", r"^[a-z]+\.[a-z-]+\d{1,2}@example\.com$");
        check("$randomColor", r"^[a-z]+$");
        check("$randomIP", r"^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$");
    }

    #[test]
    fn ranges() {
        for _ in 0..500 {
            let n: u32 = dynamic_value("$randomInt").unwrap().parse().unwrap();
            assert!(n <= 1000);
            let ip = dynamic_value("$randomIP").unwrap();
            let octets: Vec<u32> = ip.split('.').map(|o| o.parse().unwrap()).collect();
            assert!(octets.iter().all(|&o| o <= 255), "{ip}");
            assert!((1..=254).contains(&octets[0]), "{ip}");
        }
        let now = unix_now();
        let ts: u64 = dynamic_value("$timestamp").unwrap().parse().unwrap();
        assert!(ts.abs_diff(now) <= 2);
    }

    #[test]
    fn every_listed_name_resolves_and_unknown_does_not() {
        for (name, _) in DYNAMIC_VARIABLES {
            assert!(is_dynamic(name));
            assert!(dynamic_value(name).is_some(), "{name}");
        }
        assert!(!is_dynamic("$foo"));
        assert_eq!(dynamic_value("$foo"), None);
        assert_eq!(dynamic_value("guid"), None);
    }

    #[test]
    fn rfc3339_known_instants() {
        assert_eq!(rfc3339_utc(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        let t = UNIX_EPOCH + Duration::from_millis(951_782_400_000 + 3_723_045);
        // 2000-02-29 (a leap day) 01:02:03.045
        assert_eq!(rfc3339_utc(t), "2000-02-29T01:02:03.045Z");
        let t = UNIX_EPOCH + Duration::from_secs(1_798_761_599);
        assert_eq!(rfc3339_utc(t), "2026-12-31T23:59:59.000Z");
    }
}

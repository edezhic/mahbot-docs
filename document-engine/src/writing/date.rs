//! The current time as a format states it.
//!
//! The reference dates the file it wrote, so a produced file carries the moment it was made and an
//! edited file's modification date advances. Two spellings of one moment are needed: the PDF
//! dialect `D:YYYYMMDDHHMMSSZ` and the W3CDTF `YYYY-MM-DDTHH:MM:SSZ` a package's core properties
//! state. The conversion is Howard Hinnant's `civil_from_days`, repeated from the reading's own
//! number-format module because that module keeps it private to its port: the writing half cannot
//! reach across the boundary, and the algorithm is short.

/// The current time as `D:YYYYMMDDHHMMSSZ`, in UTC.
pub(super) fn now_pdf() -> String {
    let (year, month, day, hour, minute, second) = now();
    format!("D:{year:04}{month:02}{day:02}{hour:02}{minute:02}{second:02}Z")
}

/// The current time as `YYYY-MM-DDTHH:MM:SSZ`, in UTC — the spelling a package's core properties
/// state, with the trailing `Z` meaning UTC rather than a local time wearing one.
pub(super) fn now_w3cdtf() -> String {
    let (year, month, day, hour, minute, second) = now();
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// The current time as the W3CDTF a Word document's core properties state, to the millisecond: the
/// reference writes the moment it made the document with `Date.prototype.toISOString`, whose
/// resolution is finer than a second. Measured rather than assumed: two documents made within one
/// second still differ in these three digits.
pub(super) fn now_w3cdtf_millis() -> String {
    w3cdtf_millis(std::time::SystemTime::now())
}

/// The W3CDTF spelling of one clock reading, to the millisecond. The civil date and the fraction
/// come from the same `SystemTime`, so the three digits can never belong to a different second.
fn w3cdtf_millis(time: std::time::SystemTime) -> String {
    let elapsed = time
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let (year, month, day, hour, minute, second) = civil_parts(elapsed.as_secs());
    let millis = elapsed.subsec_millis();
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// The current UTC time as its parts: the civil date and the time of day.
fn now() -> (i64, u32, u32, u64, u64, u64) {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();
    civil_parts(seconds)
}

/// The UTC civil date and time of day `seconds` after 1970-01-01.
fn civil_parts(seconds: u64) -> (i64, u32, u32, u64, u64, u64) {
    let of_day = seconds % 86_400;
    let (year, month, day) = civil(seconds);
    (
        year,
        month,
        day,
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60,
    )
}

/// The civil date `seconds` after 1970-01-01 (Howard Hinnant's `civil_from_days`).
#[expect(
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a clock's seconds since 1970 fit an `i64` for any date the format can state, and the \
              month and day of month are non-negative"
)]
fn civil(seconds: u64) -> (i64, u32, u32) {
    let days = (seconds / 86_400) as i64 + 719_468;
    let era = days.div_euclid(146_097);
    let of_era = days.rem_euclid(146_097);
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_of_year = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_of_year + 2) / 5 + 1) as u32;
    let month = if month_of_year < 10 {
        month_of_year + 3
    } else {
        month_of_year - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::w3cdtf_millis;

    #[test]
    fn the_date_and_the_millis_come_from_one_clock_reading() {
        // One reading, one spelling: the second and the fraction are the same moment, even when the
        // fraction is at the second's edge.
        let at = UNIX_EPOCH + Duration::from_millis(1_700_000_000_999);
        assert_eq!(w3cdtf_millis(at), "2023-11-14T22:13:20.999Z");
        // A millisecond later moves the fraction into the next second, and the date follows it.
        let later = at + Duration::from_millis(1);
        assert_eq!(w3cdtf_millis(later), "2023-11-14T22:13:21.000Z");
        assert_ne!(w3cdtf_millis(at), w3cdtf_millis(later));
    }
}

//! The current time as the format's date.
//!
//! The reference dates the file it wrote, so a produced file carries the moment it was made and an
//! edited file's modification date advances. The conversion is Howard Hinnant's `civil_from_days`,
//! repeated from the reading's own number-format module because that module keeps it private to its
//! port: the writing half cannot reach across the boundary, and the algorithm is short.

/// The current time as `D:YYYYMMDDHHMMSSZ`, in UTC.
pub(super) fn now() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();
    let of_day = seconds % 86_400;
    let (year, month, day) = civil(seconds);
    format!(
        "D:{year:04}{month:02}{day:02}{:02}{:02}{:02}Z",
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60
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

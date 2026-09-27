use jiff::Zoned;
use subutf8_core::batch::DayFolder;

use crate::constants::TIMESTAMP_FORMAT;

/// NAME-10: today in the local time zone, from `TZ` or `/etc/localtime`; UTC without either.
pub fn today() -> DayFolder {
    let date = Zoned::now().date();
    DayFolder::new(date.year(), date.month(), date.day())
}

/// HIST-01: the local time, such as `2026-09-27T14:03:11+03:00`.
pub fn timestamp() -> String {
    Zoned::now().strftime(TIMESTAMP_FORMAT).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE_DAY: &str = "2026-09-27";
    const EXAMPLE_TIMESTAMP: &str = "2026-09-27T14:03:11+03:00";

    fn same_shape(text: &str, example: &str) -> bool {
        text.len() == example.len()
            && text
                .bytes()
                .zip(example.bytes())
                .all(|(byte, expected)| byte.is_ascii_digit() == expected.is_ascii_digit())
    }

    #[test]
    fn dates_and_times_have_a_fixed_shape() {
        assert!(same_shape(today().name(), EXAMPLE_DAY));
        let stamp = timestamp();
        assert!(same_shape(&stamp, EXAMPLE_TIMESTAMP), "{stamp}");
    }
}

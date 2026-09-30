pub(super) const SUPPORTED_STATUSES: [&str; 5] = [
    "Proposed",
    "Accepted",
    "Rejected",
    "Deprecated",
    "Superseded",
];

pub(super) fn valid_status(value: &str) -> bool {
    SUPPORTED_STATUSES.contains(&value)
}

pub(super) fn valid_iso_date(value: &str) -> bool {
    let mut parts = value.split('-');
    let (Some(year), Some(month), Some(day), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    if year.len() != 4 || month.len() != 2 || day.len() != 2 {
        return false;
    }
    let (Ok(year), Ok(month), Ok(day)) =
        (year.parse::<u16>(), month.parse::<u8>(), day.parse::<u8>())
    else {
        return false;
    };
    valid_date(year, month, day)
}

pub(super) fn valid_milestone(value: &str) -> bool {
    milestone_number(value).is_some()
}

pub(super) fn milestone_number(value: &str) -> Option<usize> {
    let (identifier, outcome) = value.split_once(" - ")?;
    let number = identifier.strip_prefix('M')?;
    if number.len() != 2
        || !number.bytes().all(|byte| byte.is_ascii_digit())
        || outcome != outcome.trim()
        || !outcome.chars().any(char::is_alphanumeric)
    {
        return None;
    }
    let number = number.parse::<usize>().ok()?;
    (number > 0).then_some(number)
}

fn valid_date(year: u16, month: u8, day: u8) -> bool {
    if year < 2000 || !(1..=12).contains(&month) {
        return false;
    }
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let maximum = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    (1..=maximum).contains(&day)
}

use std::fmt;

use jiff::civil::Date;
use opcua_line_gateway_config::AsciiDigitsOrUpper;
use regex::regex;
use thiserror::Error;
use tracing::instrument;

/// Errors that can occur during part identifier handling.
#[derive(Debug, Error)]
pub(super) enum PartIdentifierError {
    #[error("invalid part reference (got \"{0}\")")]
    PartReference(String),
    #[error("invalid serial number (should fit in 5 digits, got {0})")]
    SerialTooBig(u32),
    #[error("invalid year and/or day: {0}")]
    InvalidYearAndDay(jiff::Error),
    #[error("invalid serial number (expected 5 digits, got {0})")]
    InvalidSerial(String),
}

/// Represents the part reference portion in the part identifier.
#[derive(Debug)]
struct PartReference<'a> {
    /// Part family (1 or 2 digits).
    family: &'a str,
    /// Incremental part of reference (3 or 4 digits).
    incremental: &'a str,
    /// Part size (2 to 4 digits).
    size: &'a str,
}

impl<'a> TryFrom<&'a str> for PartReference<'a> {
    type Error = PartIdentifierError;

    fn try_from(value: &'a str) -> Result<Self, Self::Error> {
        let re = regex!(r"^P?(\d{1,2})-(\d{3,4})[A-Za-z]{2,4}(\d{2,3})(?:\D|$)");

        let (_, [family, incremental, size]) = re
            .captures(value)
            .map(|caps| caps.extract())
            .ok_or(PartIdentifierError::PartReference(value.to_owned()))?;

        Ok(Self {
            family,
            incremental,
            size,
        })
    }
}

impl fmt::Display for PartReference<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:0>2}{:0>4}{:0>3}",
            self.family, self.incremental, self.size
        )
    }
}

/// Create the part identifier from provided arguments.
///
/// The part identifier is made out of those components:
///
/// * Part reference (9 digits);
/// * Raw material batch (2 ASCII characters);
/// * Production line identifier (2 digits);
/// * Current year (2 digits);
/// * Day of year (3 digits);
/// * Per-day incremental serial number (5 digits);
#[instrument(err)]
pub(super) fn create_part_identifier(
    part_ref: &str,
    batch: AsciiDigitsOrUpper<2>,
    line_id: AsciiDigitsOrUpper<2>,
    today: Date,
    serial: u32,
) -> Result<String, PartIdentifierError> {
    if serial > 99999 {
        return Err(PartIdentifierError::SerialTooBig(serial));
    }

    let part_ref: PartReference = part_ref.try_into()?;

    let year_and_day = today.strftime("%y%j");

    let s = format!("{part_ref}{batch}{line_id}{year_and_day}{serial:05}");

    Ok(s)
}

/// Validate the provided part identifier, mainly to prevent errors when inserting
/// in the database.
#[instrument(err)]
pub(super) fn validate_part_identifier(
    s: AsciiDigitsOrUpper<23>,
) -> Result<(), PartIdentifierError> {
    let part_ref = &s.as_array()[..9];
    if !part_ref.iter().all(|b| b.is_ascii_digit()) {
        let inner = String::from_utf8_lossy(part_ref).into_owned();
        return Err(PartIdentifierError::PartReference(inner));
    }

    let year_and_day = &s.as_str()[13..18];
    Date::strptime("%y%j", year_and_day).map_err(PartIdentifierError::InvalidYearAndDay)?;

    let serial = &s.as_array()[18..];
    if !serial.iter().all(|b| b.is_ascii_digit()) {
        let inner = String::from_utf8_lossy(serial).into_owned();
        return Err(PartIdentifierError::InvalidSerial(inner));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;

    mod part_reference {
        use super::PartReference;

        macro_rules! success_test {
            ($name:ident, $in:literal, $expected:literal) => {
                #[test]
                fn $name() {
                    let part_ref = PartReference::try_from($in)
                        .expect("parsing and formatting should not fail");

                    assert_eq!(part_ref.to_string(), $expected);
                }
            };
        }

        macro_rules! failure_test {
            ($name:ident, $in:literal) => {
                #[test]
                fn $name() {
                    PartReference::try_from($in).expect_err("parsing should fail");
                }
            };
        }

        success_test!(full, "P89-4865ABCD513A-X846", "894865513");
        success_test!(no_prefix, "89-4865ABCD513A-X846", "894865513");
        success_test!(no_suffix, "P89-4865ABCD513", "894865513");
        success_test!(single_digit_family, "P8-4865ABCD513A-X846", "084865513");
        success_test!(
            three_digits_incremental,
            "P89-485ABCD513A-X846",
            "890485513"
        );
        success_test!(two_chars_joint_type, "P89-4865XY513A-X846", "894865513");
        success_test!(two_digits_size, "P89-4865ABCD42A-X846", "894865042");
        success_test!(minimal, "1-498GR13", "010498013");

        failure_test!(invalid_prefix, "F89-4865ABCD513A-X846");
        failure_test!(missing_family, "P-4865ABCD513A-X846");
        failure_test!(family_too_long, "P897-4865ABCD513A-X846");
        failure_test!(invalid_dash, "P89/4865ABCD513A-X846");
        failure_test!(missing_dash, "P894865ABCD513A-X846");
        failure_test!(missing_incremental, "P89-ABCD513A-X846");
        failure_test!(incremental_too_short, "P89-25ABCD513A-X846");
        failure_test!(incremental_too_long, "P89-48657ABCD513A-X846");
        failure_test!(missing_joint_type, "P89-4865513A-X846");
        failure_test!(joint_type_too_short, "P89-4865B513A-X846");
        failure_test!(joint_type_too_long, "P89-4865ABCDE513A-X846");
        failure_test!(missing_joint_size, "P89-4865ABCDA-X846");
        failure_test!(joint_size_too_short, "P89-4865ABCD9A-X846");
        failure_test!(joint_size_too_long_with_postfix, "P89-4865ABCD8513A-X846");
        failure_test!(joint_size_too_long_without_postfix, "P89-4865ABCD8513");
    }

    mod create_part_identifier {
        use super::*;

        #[test]
        fn serial_too_long() {
            let batch = "XX".parse().expect("parsing the batch should not fail");
            let line_id = "42".parse().expect("parsing the batch should not fail");
            let today = Date::constant(2026, 12, 9);

            let result = create_part_identifier("12-3498GR713", batch, line_id, today, 100000);

            assert_matches!(result, Err(PartIdentifierError::SerialTooBig(100000)));
        }

        #[test]
        fn ok_with_padding() {
            let batch = "XX".parse().expect("parsing the batch should not fail");
            let line_id = "42".parse().expect("parsing the batch should not fail");
            let today = Date::constant(2005, 1, 9);

            let part_id = create_part_identifier("12-3498GR713", batch, line_id, today, 3)
                .expect("creating part identifier should not fail");

            assert_eq!(part_id, "123498713XX420500900003");
        }

        #[test]
        fn ok_no_padding() {
            let batch = "XX".parse().expect("parsing the batch should not fail");
            let line_id = "42".parse().expect("parsing the batch should not fail");
            let today = Date::constant(2026, 12, 9);

            let part_id = create_part_identifier("12-3498GR713", batch, line_id, today, 12345)
                .expect("creating part identifier should not fail");

            assert_eq!(part_id, "123498713XX422634312345");
        }
    }

    mod validate_part_identifier {
        use super::*;

        #[test]
        fn invalid_part_reference() {
            let part_id = "1234A6789XX422634300001"
                .parse()
                .expect("parsing part identifier should not fail");

            let result = validate_part_identifier(part_id);

            assert_matches!(result, Err(PartIdentifierError::PartReference(err)) if err == "1234A6789");
        }

        #[test]
        fn invalid_day() {
            let part_id = "123456789XX422650000001"
                .parse()
                .expect("parsing part identifier should not fail");

            let result = validate_part_identifier(part_id);

            assert_matches!(
                result,
                Err(PartIdentifierError::InvalidYearAndDay(err)) if err.is_range()
            );
        }

        #[test]
        fn invalid_serial() {
            let part_id = "123456789XX422634300A01"
                .parse()
                .expect("parsing part identifier should not fail");

            let result = validate_part_identifier(part_id);

            assert_matches!(result, Err(PartIdentifierError::InvalidSerial(err)) if err == "00A01");
        }

        #[test]
        fn ok() {
            let part_id = "123456789XX422634300001"
                .parse()
                .expect("parsing part identifier should not fail");

            validate_part_identifier(part_id).expect("part identifier should be valid");
        }
    }
}

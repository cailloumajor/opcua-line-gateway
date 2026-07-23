use std::fmt;

use jiff::civil::Date;
use opcua_line_gateway_config::AsciiText;
use regex::regex;
use thiserror::Error;

/// Errors that can occur during part identifier handling.
#[derive(Debug, Error)]
pub(super) enum PartIdentifierError {
    #[error("invalid part reference (got \"{0}\")")]
    PartReference(String),
    #[error("invalid serial number (should fit in 5 digits, got {0})")]
    SerialTooBig(u32),
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
pub(super) fn create_part_identifier(
    part_ref: &str,
    batch: AsciiText<2>,
    line_id: AsciiText<2>,
    today: Date,
    serial: u32,
) -> Result<String, PartIdentifierError> {
    if serial > 99999 {
        return Err(PartIdentifierError::SerialTooBig(serial));
    }

    let part_ref: PartReference = part_ref.try_into()?;

    let year = today.year() % 100;
    let day = today.day_of_year();

    let s = format!("{part_ref}{batch}{line_id}{year:02}{day:03}{serial:05}");

    Ok(s)
}

#[cfg(test)]
mod tests {
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

    mod part_identifier {
        use super::create_part_identifier;

        macro_rules! success_test {
            ($name:ident, $part_ref:literal,$batch:literal,$line_id:literal,$today:literal,$serial:literal,     $expected:literal) => {
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
    }
}

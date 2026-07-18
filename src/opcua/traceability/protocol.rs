use nom::bytes::complete::take_while_m_n;
use nom::character::complete::{char, digit1};
use nom::combinator::not;
use nom::{AsChar, Finish, Parser};
use opcua::types::{IntoVariant, Variant};
use strum::FromRepr;

/// Traceability request code.
#[derive(Clone, Copy, FromRepr)]
#[repr(u8)]
pub(super) enum TraceabilityRequest {
    /// Reset state of the request.
    Reset = 0,
    /// Request for creating a part ID.
    CreatePartId = 1,
    /// Request from the machine for getting part data sheets.
    GetPartSheets = 2,
    /// Request from the machine for saving part data sheets.
    SavePartSheets = 3,
}

/// Traceability response code.
#[derive(Clone, Copy)]
#[repr(u8)]
pub(super) enum TraceabilityResponse {
    /// Reset state of the response.
    Reset = 0,

    // Request handling errors.
    RequestGetValueError = 11,
    RequestUnknownValue = 12,
}

impl IntoVariant for TraceabilityResponse {
    fn into_variant(self) -> Variant {
        (self as u8).into()
    }
}

/// Parse the part reference and format it to be used in part ID.
pub(super) fn parse_and_format_part_reference(
    input: &str,
) -> Result<String, nom::error::Error<&str>> {
    let mut parser_chain = (
        // Optional 'P' (product) prefix from Galia specification. Not kept.
        take_while_m_n(0, 1, |c| c == 'P'),
        // Part family (1 or 2 digits number).
        take_while_m_n(1, 2, AsChar::is_dec_digit),
        // Dash. Not kept.
        char('-'),
        // Incremental reference (3 or 4 digits number)
        take_while_m_n(3, 4, AsChar::is_dec_digit),
        // Joint type (2 to 4 characters). Not kept.
        take_while_m_n(2, 4, AsChar::is_alpha),
        // Joint size (2 or 3 digits number).
        take_while_m_n(2, 3, AsChar::is_dec_digit),
        // Postfix or end of input. Not kept.
        not(digit1),
    );
    let (_, (_, family, _, incremental, _, size, _)) = parser_chain.parse(input).finish()?;

    let out = format!("{family:0>2}{incremental:0>4}{size:0>3}");

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    mod part_reference {
        use super::*;

        macro_rules! success_test {
            ($name:ident, $in:literal, $expected:literal) => {
                #[test]
                fn $name() {
                    let formatted = parse_and_format_part_reference($in)
                        .expect("parsing and formatting should not fail");

                    assert_eq!(formatted, $expected);
                }
            };
        }

        macro_rules! failure_test {
            ($name:ident, $in:literal) => {
                #[test]
                fn $name() {
                    parse_and_format_part_reference($in).expect_err("parsing should fail");
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
}

use opcua::types::{DataValue, StatusCode, UAString, Variant};
use opcua_line_gateway_config::{AsciiText, AsciiTextError};
use thiserror::Error;

/// Errors that can occur using [`TryFromDataValue`].
#[derive(Debug, Error)]
pub(super) enum TryFromDataValueError {
    #[error("data value status is not good: {0}")]
    BadStatus(StatusCode),
    #[error("missing data value")]
    MissingValue,
    #[error("invalid data value type (expected {0}, got {1})")]
    InvalidType(&'static str, String),
    #[error("string value is null")]
    NullString,
    #[error(transparent)]
    AsciiText(#[from] AsciiTextError),
}

/// Models the ability to convert a [`DataValue`] to useful types.
pub(super) trait TryFromDataValue<'a>: Sized {
    /// Try to convert the provided [`DataValue`] to this type.
    ///
    /// # Errors
    ///
    /// Returns [`TryFromDataValueError`] if the underlying `Variant` is
    /// absent (e.g. a bad status code with no value) or does not match
    /// the requested target type. This does not attempt any numeric or
    /// type-level casting — the stored variant must already match `T`.
    fn try_from_data_value(v: &'a DataValue) -> Result<Self, TryFromDataValueError>;
}

/// Validates the status and extracts the `&Variant` payload, common to
/// every [`TryFromDataValue`] impl generated below.
fn extract_variant(v: &DataValue) -> Result<&Variant, TryFromDataValueError> {
    let status = v.status();
    if !status.is_good() {
        return Err(TryFromDataValueError::BadStatus(status));
    }
    let Some(variant) = &v.value else {
        return Err(TryFromDataValueError::MissingValue);
    };

    Ok(variant)
}

/// Generate [`TryFromDataValue`] implementation for the provided type and [`Variant`] enum
/// variant.
///
/// # Input formats
///
/// * `copy: $type, $variant` — payload is `Copy`, returned by value.
/// * `ref: $type, $variant` — payload borrowed as `&'a $type`, no clone.
macro_rules! impl_try_from_data_value_primitive {
    (copy: $type:ty, $variant:ident) => {
        impl TryFromDataValue<'_> for $type {
            fn try_from_data_value(v: &DataValue) -> Result<Self, TryFromDataValueError> {
                let variant = extract_variant(v)?;

                let Variant::$variant(val) = variant else {
                    return Err(TryFromDataValueError::InvalidType(
                        stringify!($variant),
                        format!("{:?}", variant.type_id()),
                    ));
                };

                Ok(*val)
            }
        }
    };
    (ref: $type:ty, $variant:ident) => {
        impl<'a> TryFromDataValue<'a> for &'a $type {
            fn try_from_data_value(v: &'a DataValue) -> Result<Self, TryFromDataValueError> {
                let variant = extract_variant(v)?;

                let Variant::$variant(val) = variant else {
                    return Err(TryFromDataValueError::InvalidType(
                        stringify!($variant),
                        format!("{:?}", variant.type_id()),
                    ));
                };

                Ok(val)
            }
        }
    };
}

impl_try_from_data_value_primitive!(copy: bool, Boolean);
impl_try_from_data_value_primitive!(copy: i8, SByte);
impl_try_from_data_value_primitive!(copy: u8, Byte);
impl_try_from_data_value_primitive!(copy: i16, Int16);
impl_try_from_data_value_primitive!(copy: u16, UInt16);
impl_try_from_data_value_primitive!(copy: i32, Int32);
impl_try_from_data_value_primitive!(copy: u32, UInt32);
impl_try_from_data_value_primitive!(copy: i64, Int64);
impl_try_from_data_value_primitive!(copy: u64, UInt64);
impl_try_from_data_value_primitive!(copy: f32, Float);
impl_try_from_data_value_primitive!(copy: f64, Double);
impl_try_from_data_value_primitive!(ref: UAString, String);

impl<'a> TryFromDataValue<'a> for &'a str {
    fn try_from_data_value(v: &'a DataValue) -> Result<Self, TryFromDataValueError> {
        let s: &UAString = v.try_as()?;

        s.value()
            .as_deref()
            .ok_or(TryFromDataValueError::NullString)
    }
}

impl<const LENGTH: usize> TryFromDataValue<'_> for AsciiText<LENGTH> {
    fn try_from_data_value(v: &DataValue) -> Result<Self, TryFromDataValueError> {
        let s: &str = v.try_as()?;
        let ascii = s.parse()?;

        Ok(ascii)
    }
}

impl<'a> TryFromDataValue<'a> for &'a Variant {
    fn try_from_data_value(v: &'a DataValue) -> Result<Self, TryFromDataValueError> {
        extract_variant(v)
    }
}

/// Extension trait adding ergonomic conversion methods to [`DataValue`].
pub(super) trait DataValueExt {
    /// Try to convert this [`DataValue`] into `T`.
    ///
    /// This is a thin wrapper around [`TryFromDataValue::try_from_data_value`],
    /// provided as a method so call sites can write `dv.try_as::<&str>()`
    /// instead of the more verbose fully-qualified syntax.
    fn try_as<'a, T: TryFromDataValue<'a>>(&'a self) -> Result<T, TryFromDataValueError>
    where
        Self: 'a;
}

impl DataValueExt for DataValue {
    fn try_as<'a, T: TryFromDataValue<'a>>(&'a self) -> Result<T, TryFromDataValueError>
    where
        Self: 'a,
    {
        T::try_from_data_value(self)
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;

    #[test]
    fn bad_status() {
        let data_value = DataValue {
            value: Some(42u8.into()),
            status: Some(StatusCode::BadShutdown),
            ..Default::default()
        };

        let result = data_value.try_as::<u8>();

        assert_matches!(
            result,
            Err(TryFromDataValueError::BadStatus(StatusCode::BadShutdown))
        );
    }

    #[test]
    fn missing_value() {
        let data_value = DataValue {
            status: Some(StatusCode::GoodClamped),
            ..Default::default()
        };

        let result = data_value.try_as::<u8>();

        assert_matches!(result, Err(TryFromDataValueError::MissingValue));
    }

    #[test]
    fn bad_value_type() {
        let data_value = DataValue {
            value: Some(42u16.into()),
            status: Some(StatusCode::GoodClamped),
            ..Default::default()
        };

        let result = data_value.try_as::<u8>();

        assert_matches!(result, Err(TryFromDataValueError::InvalidType("Byte", got)) if got == "Scalar(UInt16)");
    }

    #[test]
    fn bool_ok_no_status() {
        let data_value = DataValue {
            value: Some(true.into()),
            ..Default::default()
        };

        let got: bool = data_value.try_as().expect("should be successful");

        assert!(got);
    }

    #[test]
    fn i8_ok() {
        let data_value = DataValue {
            value: Some((-42i8).into()),
            status: Some(StatusCode::GoodClamped),
            ..Default::default()
        };

        let got: i8 = data_value.try_as().expect("should be successful");

        assert_eq!(got, -42);
    }

    #[test]
    fn u8_ok() {
        let data_value = DataValue {
            value: Some(42u8.into()),
            status: Some(StatusCode::GoodClamped),
            ..Default::default()
        };

        let got: u8 = data_value.try_as().expect("should be successful");

        assert_eq!(got, 42);
    }

    #[test]
    fn i16_ok() {
        let data_value = DataValue {
            value: Some((-546i16).into()),
            status: Some(StatusCode::GoodClamped),
            ..Default::default()
        };

        let got: i16 = data_value.try_as().expect("should be successful");

        assert_eq!(got, -546);
    }

    #[test]
    fn u16_ok() {
        let data_value = DataValue {
            value: Some(561u16.into()),
            status: Some(StatusCode::GoodClamped),
            ..Default::default()
        };

        let got: u16 = data_value.try_as().expect("should be successful");

        assert_eq!(got, 561);
    }

    #[test]
    fn i32_ok() {
        let data_value = DataValue {
            value: Some((-71234i32).into()),
            status: Some(StatusCode::GoodClamped),
            ..Default::default()
        };

        let got: i32 = data_value.try_as().expect("should be successful");

        assert_eq!(got, -71234);
    }

    #[test]
    fn u32_ok() {
        let data_value = DataValue {
            value: Some((812345u32).into()),
            status: Some(StatusCode::GoodClamped),
            ..Default::default()
        };

        let got: u32 = data_value.try_as().expect("should be successful");

        assert_eq!(got, 812345);
    }

    #[test]
    fn i64_ok() {
        let data_value = DataValue {
            value: Some((-9812345678i64).into()),
            status: Some(StatusCode::GoodClamped),
            ..Default::default()
        };

        let got: i64 = data_value.try_as().expect("should be successful");

        assert_eq!(got, -9812345678);
    }

    #[test]
    fn u64_ok() {
        let data_value = DataValue {
            value: Some((9812345678u64).into()),
            status: Some(StatusCode::GoodClamped),
            ..Default::default()
        };

        let got: u64 = data_value.try_as().expect("should be successful");

        assert_eq!(got, 9812345678);
    }

    #[test]
    fn f32_ok() {
        let data_value = DataValue {
            value: Some((-12.375f32).into()),
            status: Some(StatusCode::GoodClamped),
            ..Default::default()
        };

        let got: f32 = data_value.try_as().expect("should be successful");

        assert_eq!(got, -12.375);
    }

    #[test]
    fn f64_ok() {
        let data_value = DataValue {
            value: Some((std::f64::consts::PI).into()),
            status: Some(StatusCode::GoodClamped),
            ..Default::default()
        };

        let got: f64 = data_value.try_as().expect("should be successful");

        assert_eq!(got, std::f64::consts::PI);
    }

    #[test]
    fn str_ok() {
        let data_value = DataValue {
            value: Some("hello gateway".to_string().into()),
            status: Some(StatusCode::GoodClamped),
            ..Default::default()
        };

        let got: &str = data_value.try_as().expect("should be successful");

        assert_eq!(got, "hello gateway");
    }
}

use opcua::types::{DataValue, StatusCode, Variant};
use opcua_line_gateway_config::{AsciiText, AsciiTextError};
use thiserror::Error;

/// Errors that can occur using [`TryFromDataValue`].
#[derive(Debug, Error)]
pub(super) enum TryFromDataValueError {
    #[error("missing data value status code")]
    MissingStatus,
    #[error("data value status is not good")]
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

/// Generate [`TryFromDataValue`] implementation for the provided type and [`Variant`] enum
/// variant.
macro_rules! impl_try_from_data_value_primitive {
    ($type:ty, $variant:ident) => {
        impl TryFromDataValue<'_> for $type {
            fn try_from_data_value(v: &DataValue) -> Result<Self, TryFromDataValueError> {
                let Some(status) = v.status else {
                    return Err(TryFromDataValueError::MissingStatus);
                };
                if !status.is_good() {
                    return Err(TryFromDataValueError::BadStatus(status));
                }
                let Some(variant) = &v.value else {
                    return Err(TryFromDataValueError::MissingValue);
                };
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
}

impl_try_from_data_value_primitive!(bool, Boolean);
impl_try_from_data_value_primitive!(i8, SByte);
impl_try_from_data_value_primitive!(u8, Byte);
impl_try_from_data_value_primitive!(i16, Int16);
impl_try_from_data_value_primitive!(u16, UInt16);
impl_try_from_data_value_primitive!(i32, Int32);
impl_try_from_data_value_primitive!(u32, UInt32);
impl_try_from_data_value_primitive!(i64, Int64);
impl_try_from_data_value_primitive!(u64, UInt64);
impl_try_from_data_value_primitive!(f32, Float);
impl_try_from_data_value_primitive!(f64, Double);

impl<'a> TryFromDataValue<'a> for &'a str {
    fn try_from_data_value(v: &'a DataValue) -> Result<Self, TryFromDataValueError> {
        let Some(status) = v.status else {
            return Err(TryFromDataValueError::MissingStatus);
        };
        if !status.is_good() {
            return Err(TryFromDataValueError::BadStatus(status));
        }
        let Some(variant) = &v.value else {
            return Err(TryFromDataValueError::MissingValue);
        };
        let Variant::String(ua_string) = variant else {
            return Err(TryFromDataValueError::InvalidType(
                "String",
                format!("{:?}", variant.type_id()),
            ));
        };
        let Some(s) = ua_string.value() else {
            return Err(TryFromDataValueError::NullString);
        };

        Ok(s.as_str())
    }
}

impl<const LENGTH: usize> TryFromDataValue<'_> for AsciiText<LENGTH> {
    fn try_from_data_value(v: &DataValue) -> Result<Self, TryFromDataValueError> {
        let s: &str = v.try_as()?;
        let ascii = s.parse()?;

        Ok(ascii)
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
    fn missing_status() {
        let data_value = DataValue {
            value: Some(42u8.into()),
            ..Default::default()
        };

        let result = data_value.try_as::<u8>();

        assert_matches!(result, Err(TryFromDataValueError::MissingStatus));
    }

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
    fn bool_ok() {
        let data_value = DataValue {
            value: Some(true.into()),
            status: Some(StatusCode::GoodClamped),
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

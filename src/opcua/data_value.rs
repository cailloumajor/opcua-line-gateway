use opcua::types::{DataValue, StatusCode, Variant};
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
}

/// Models the ability to convert a [`DataValue`] to useful types.
pub(super) trait TryFromDataValue: Sized {
    /// Try to convert the provided [`DataValue`] to this type.
    ///
    /// This does not try to cast the variant type to the target type.
    fn try_from_data_value(v: DataValue) -> Result<Self, TryFromDataValueError>;
}

/// Generate [`TryFromDataValue`] implementation for the provided type and [`Variant`] enum
/// variant.
macro_rules! impl_try_from_data_value_primitive {
    ($type:ty, $variant:ident) => {
        impl TryFromDataValue for $type {
            fn try_from_data_value(v: DataValue) -> Result<Self, TryFromDataValueError> {
                let Some(status) = v.status else {
                    return Err(TryFromDataValueError::MissingStatus);
                };
                if !status.is_good() {
                    return Err(TryFromDataValueError::BadStatus(status));
                }
                let Some(variant) = v.value else {
                    return Err(TryFromDataValueError::MissingValue);
                };
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

impl TryFromDataValue for String {
    fn try_from_data_value(v: DataValue) -> Result<Self, TryFromDataValueError> {
        let Some(status) = v.status else {
            return Err(TryFromDataValueError::MissingStatus);
        };
        if !status.is_good() {
            return Err(TryFromDataValueError::BadStatus(status));
        }
        let Some(variant) = v.value else {
            return Err(TryFromDataValueError::MissingValue);
        };
        let Variant::String(ua_string) = variant else {
            return Err(TryFromDataValueError::InvalidType(
                "String",
                format!("{:?}", variant.type_id()),
            ));
        };
        let Some(s) = ua_string.value().to_owned() else {
            return Err(TryFromDataValueError::NullString);
        };

        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_status() {
        let data_value = DataValue {
            value: Some(42u8.into()),
            ..Default::default()
        };

        u8::try_from_data_value(data_value).expect_err("should return an error");
    }

    #[test]
    fn bad_status() {
        let data_value = DataValue {
            value: Some(42u8.into()),
            status: Some(StatusCode::BadShutdown),
            ..Default::default()
        };

        u8::try_from_data_value(data_value).expect_err("should return an error");
    }

    #[test]
    fn missing_value() {
        let data_value = DataValue {
            status: Some(StatusCode::GoodClamped),
            ..Default::default()
        };

        u8::try_from_data_value(data_value).expect_err("should return an error");
    }

    #[test]
    fn bad_value_type() {
        let data_value = DataValue {
            value: Some(42u16.into()),
            status: Some(StatusCode::GoodClamped),
            ..Default::default()
        };

        u8::try_from_data_value(data_value).expect_err("should return an error");
    }

    #[test]
    fn bool_ok() {
        let data_value = DataValue {
            value: Some(true.into()),
            status: Some(StatusCode::GoodClamped),
            ..Default::default()
        };

        assert!(bool::try_from_data_value(data_value).expect("should be successful"));
    }

    #[test]
    fn i8_ok() {
        let data_value = DataValue {
            value: Some((-42i8).into()),
            status: Some(StatusCode::GoodClamped),
            ..Default::default()
        };

        assert_eq!(
            i8::try_from_data_value(data_value).expect("should be successful"),
            -42
        );
    }

    #[test]
    fn u8_ok() {
        let data_value = DataValue {
            value: Some(42u8.into()),
            status: Some(StatusCode::GoodClamped),
            ..Default::default()
        };

        assert_eq!(
            u8::try_from_data_value(data_value).expect("should be successful"),
            42
        );
    }

    #[test]
    fn i16_ok() {
        let data_value = DataValue {
            value: Some((-546i16).into()),
            status: Some(StatusCode::GoodClamped),
            ..Default::default()
        };

        assert_eq!(
            i16::try_from_data_value(data_value).expect("should be successful"),
            -546
        );
    }

    #[test]
    fn u16_ok() {
        let data_value = DataValue {
            value: Some(561u16.into()),
            status: Some(StatusCode::GoodClamped),
            ..Default::default()
        };

        assert_eq!(
            u16::try_from_data_value(data_value).expect("should be successful"),
            561
        );
    }

    #[test]
    fn i32_ok() {
        let data_value = DataValue {
            value: Some((-71234i32).into()),
            status: Some(StatusCode::GoodClamped),
            ..Default::default()
        };
        assert_eq!(
            i32::try_from_data_value(data_value).expect("should be successful"),
            -71234
        );
    }

    #[test]
    fn u32_ok() {
        let data_value = DataValue {
            value: Some((812345u32).into()),
            status: Some(StatusCode::GoodClamped),
            ..Default::default()
        };
        assert_eq!(
            u32::try_from_data_value(data_value).expect("should be successful"),
            812345
        );
    }

    #[test]
    fn i64_ok() {
        let data_value = DataValue {
            value: Some((-9812345678i64).into()),
            status: Some(StatusCode::GoodClamped),
            ..Default::default()
        };
        assert_eq!(
            i64::try_from_data_value(data_value).expect("should be successful"),
            -9812345678
        );
    }

    #[test]
    fn u64_ok() {
        let data_value = DataValue {
            value: Some((9812345678u64).into()),
            status: Some(StatusCode::GoodClamped),
            ..Default::default()
        };
        assert_eq!(
            u64::try_from_data_value(data_value).expect("should be successful"),
            9812345678
        );
    }

    #[test]
    fn f32_ok() {
        let data_value = DataValue {
            value: Some((-12.375f32).into()),
            status: Some(StatusCode::GoodClamped),
            ..Default::default()
        };
        assert_eq!(
            f32::try_from_data_value(data_value).expect("should be successful"),
            -12.375
        );
    }

    #[test]
    fn f64_ok() {
        let data_value = DataValue {
            value: Some((std::f64::consts::PI).into()),
            status: Some(StatusCode::GoodClamped),
            ..Default::default()
        };
        assert_eq!(
            f64::try_from_data_value(data_value).expect("should be successful"),
            std::f64::consts::PI
        );
    }

    #[test]
    fn string_ok() {
        let data_value = DataValue {
            value: Some("hello gateway".to_string().into()),
            status: Some(StatusCode::GoodClamped),
            ..Default::default()
        };
        assert_eq!(
            String::try_from_data_value(data_value).expect("should be successful"),
            "hello gateway"
        );
    }
}

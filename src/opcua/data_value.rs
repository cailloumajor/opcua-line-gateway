use opcua::types::{DataValue, StatusCode, UAString, Variant};
use opcua_line_gateway_config::{AsciiDigitsOrUpper, AsciiTextError};
use thiserror::Error;

/// Errors that can occur during conversion of OPC-UA value.
#[derive(Debug, Error)]
pub(crate) enum TryFromOpcUaValueError {
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

impl TryFromOpcUaValueError {
    fn invalid_type(expected: &'static str, got: &Variant) -> Self {
        Self::InvalidType(expected, format!("{:?}", got.type_id()))
    }
}

/// Fallible conversion of [`Variant`] to useful types.
pub(crate) trait TryFromVariant: Sized {
    /// Try to convert the provided [`Variant`] reference to this type.
    ///
    /// # Errors
    ///
    /// Returns [`TryFromOpcUaValueError`] if provided [`Variant`] does not match
    /// the requested target type. This does not attempt any numeric or
    /// type-level casting — the stored variant must already match `T`.
    fn try_from_variant(v: Variant) -> Result<Self, TryFromOpcUaValueError>;
}

/// Generate [`TryFromVariant`] implementation for the provided type and [`Variant`] enum
/// variant.
macro_rules! impl_try_from_variant_primitive {
    ($type:ty, $variant:ident) => {
        impl TryFromVariant for $type {
            fn try_from_variant(v: Variant) -> Result<Self, TryFromOpcUaValueError> {
                let Variant::$variant(val) = v else {
                    return Err(TryFromOpcUaValueError::invalid_type(
                        stringify!($variant),
                        &v,
                    ));
                };

                Ok(val)
            }
        }
    };
}

impl_try_from_variant_primitive!(bool, Boolean);
impl_try_from_variant_primitive!(i8, SByte);
impl_try_from_variant_primitive!(u8, Byte);
impl_try_from_variant_primitive!(i16, Int16);
impl_try_from_variant_primitive!(u16, UInt16);
impl_try_from_variant_primitive!(i32, Int32);
impl_try_from_variant_primitive!(u32, UInt32);
impl_try_from_variant_primitive!(i64, Int64);
impl_try_from_variant_primitive!(u64, UInt64);
impl_try_from_variant_primitive!(f32, Float);
impl_try_from_variant_primitive!(f64, Double);
impl_try_from_variant_primitive!(UAString, String);

impl TryFromVariant for String {
    fn try_from_variant(v: Variant) -> Result<Self, TryFromOpcUaValueError> {
        let s: UAString = TryFromVariant::try_from_variant(v)?;

        s.value().clone().ok_or(TryFromOpcUaValueError::NullString)
    }
}

impl<const LENGTH: usize> TryFromVariant for AsciiDigitsOrUpper<LENGTH> {
    fn try_from_variant(v: Variant) -> Result<Self, TryFromOpcUaValueError> {
        let s: String = TryFromVariant::try_from_variant(v)?;
        let ascii = s.parse()?;

        Ok(ascii)
    }
}

/// Extension trait adding ergonomic conversion methods to [`DataValue`].
pub(crate) trait DataValueExt {
    /// Validates the status and extracts the `Variant` payload, common to
    /// every [`TryFromDataValue`] impl generated below.
    fn try_into_variant(self) -> Result<Variant, TryFromOpcUaValueError>;

    /// Try to convert this [`DataValue`] into `T`.
    ///
    /// This allows writing `dv.try_as::<T>()` instead of the more verbose
    /// fully-qualified syntax.
    fn try_ua_value_as<T>(self) -> Result<T, TryFromOpcUaValueError>
    where
        T: TryFromVariant;
}

impl DataValueExt for DataValue {
    fn try_into_variant(self) -> Result<Variant, TryFromOpcUaValueError> {
        let status = self.status();
        if !status.is_good() {
            return Err(TryFromOpcUaValueError::BadStatus(status));
        }
        let Some(variant) = self.value else {
            return Err(TryFromOpcUaValueError::MissingValue);
        };

        Ok(variant)
    }

    fn try_ua_value_as<T>(self) -> Result<T, TryFromOpcUaValueError>
    where
        T: TryFromVariant,
    {
        let variant = self.try_into_variant()?;

        T::try_from_variant(variant)
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;

    mod try_from_variant {
        use super::*;

        #[test]
        fn bad_value_type() {
            let result = u8::try_from_variant(42u16.into());

            assert_matches!(result, Err(TryFromOpcUaValueError::InvalidType("Byte", got)) if got == "Scalar(UInt16)");
        }

        #[test]
        fn bool_ok() {
            let got = bool::try_from_variant(true.into()).expect("should be successful");

            assert!(got);
        }

        #[test]
        fn i8_ok() {
            let got = i8::try_from_variant((-42i8).into()).expect("should be successful");

            assert_eq!(got, -42);
        }

        #[test]
        fn u8_ok() {
            let got = u8::try_from_variant((42u8).into()).expect("should be successful");

            assert_eq!(got, 42);
        }

        #[test]
        fn i16_ok() {
            let got = i16::try_from_variant((-546i16).into()).expect("should be successful");

            assert_eq!(got, -546);
        }

        #[test]
        fn u16_ok() {
            let got = u16::try_from_variant((561u16).into()).expect("should be successful");

            assert_eq!(got, 561);
        }

        #[test]
        fn i32_ok() {
            let got = i32::try_from_variant((-71234i32).into()).expect("should be successful");

            assert_eq!(got, -71234);
        }

        #[test]
        fn u32_ok() {
            let got = u32::try_from_variant((812345u32).into()).expect("should be successful");

            assert_eq!(got, 812345);
        }

        #[test]
        fn i64_ok() {
            let got = i64::try_from_variant((-9812345678i64).into()).expect("should be successful");

            assert_eq!(got, -9812345678);
        }

        #[test]
        fn u64_ok() {
            let got = u64::try_from_variant((9812345678u64).into()).expect("should be successful");

            assert_eq!(got, 9812345678);
        }

        #[test]
        fn f32_ok() {
            let got = f32::try_from_variant((-12.375f32).into()).expect("should be successful");

            assert_eq!(got, -12.375);
        }

        #[test]
        fn f64_ok() {
            let got =
                f64::try_from_variant((std::f64::consts::PI).into()).expect("should be successful");

            assert_eq!(got, std::f64::consts::PI);
        }

        #[test]
        fn str_ok() {
            let variant: Variant = "hello gateway".to_string().into();
            let got = String::try_from_variant(variant).expect("should be successful");

            assert_eq!(got, "hello gateway");
        }
    }

    mod try_ua_value_as {
        use super::*;

        #[test]
        fn bad_status() {
            let data_value = DataValue {
                value: Some(42u8.into()),
                status: Some(StatusCode::BadShutdown),
                ..Default::default()
            };

            let result = data_value.try_ua_value_as::<u8>();

            assert_matches!(
                result,
                Err(TryFromOpcUaValueError::BadStatus(StatusCode::BadShutdown))
            );
        }

        #[test]
        fn missing_value() {
            let data_value = DataValue {
                status: Some(StatusCode::GoodClamped),
                ..Default::default()
            };

            let result = data_value.try_ua_value_as::<u8>();

            assert_matches!(result, Err(TryFromOpcUaValueError::MissingValue));
        }

        #[test]
        fn bool_ok_no_status() {
            let data_value = DataValue {
                value: Some(true.into()),
                ..Default::default()
            };

            let got: bool = data_value.try_ua_value_as().expect("should be successful");

            assert!(got);
        }
    }
}

//! Defines kernel to extract a upper case of a \[Large\]StringArray

use crate::{
    array::Utf8Array,
    datatypes::DataType,
    error::{ArrowError, Result},
};

/// Returns a new `Array` where each of each of the elements is upper-cased.
/// this function errors when the passed array is not a \[Large\]String array.
pub fn upper(array: &dyn Array) -> Result<Box<dyn Array>> {
    match array.data_type() {
        DataType::LargeUtf8 => Ok(Box::new(utf8_apply(
            str::to_uppercase,
            array
                .as_any()
                .downcast_ref::<Utf8Array<i64>>()
                .expect("A large string is expected"),
        ))),
        DataType::Utf8 => Ok(Box::new(utf8_apply(
            str::to_uppercase,
            array
                .as_any()
                .downcast_ref::<Utf8Array<i32>>()
                .expect("A string is expected"),
        ))),
        _ => Err(ArrowError::InvalidArgumentError(format!(
            "upper does not support type {:?}",
            array.data_type()
        ))),
    }
}

/// Checks if an array of type `datatype` can perform upper operation
///
/// # Examples
/// ```
/// use arrow2::compute::upper::can_upper;
/// use arrow2::datatypes::{DataType};
///
/// let data_type = DataType::Utf8;
/// assert_eq!(can_upper(&data_type), true);
///
/// let data_type = DataType::Null;
/// assert_eq!(can_upper(&data_type), false);
/// ```
pub fn can_upper(data_type: &DataType) -> bool {
    matches!(data_type, DataType::LargeUtf8 | DataType::Utf8)
}

/// Returns a new `Array` where each of each of the elements is lower-cased.
/// this function errors when the passed array is not a \[Large\]String array.
pub fn lower(array: &dyn Array) -> Result<Box<dyn Array>> {
    match array.data_type() {
        DataType::LargeUtf8 => Ok(Box::new(utf8_apply(
            str::to_lowercase,
            array
                .as_any()
                .downcast_ref::<Utf8Array<i64>>()
                .expect("A large string is expected"),
        ))),
        DataType::Utf8 => Ok(Box::new(utf8_apply(
            str::to_lowercase,
            array
                .as_any()
                .downcast_ref::<Utf8Array<i32>>()
                .expect("A string is expected"),
        ))),
        _ => Err(ArrowError::InvalidArgumentError(format!(
            "lower does not support type {:?}",
            array.data_type()
        ))),
    }
}

/// Checks if an array of type `datatype` can perform lower operation
///
/// # Examples
/// ```
/// use arrow2::compute::lower::can_lower;
/// use arrow2::datatypes::{DataType};
///
/// let data_type = DataType::Utf8;
/// assert_eq!(can_lower(&data_type), true);
///
/// let data_type = DataType::Null;
/// assert_eq!(can_lower(&data_type), false);
/// ```
pub fn can_lower(data_type: &DataType) -> bool {
    matches!(data_type, DataType::LargeUtf8 | DataType::Utf8)
}

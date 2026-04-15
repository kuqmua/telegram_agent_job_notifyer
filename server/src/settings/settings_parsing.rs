use std::{fmt::Display, str::FromStr};

use super::{EnvironmentError, MESSAGE_VALUE_MUST_BE_GREATER_THAN_ZERO};

pub(super) fn parse_positive_variable<Value>(
    variable_name: &'static str,
    variable_value: &str,
) -> Result<Value, EnvironmentError>
where
    Value: FromStr + PartialEq + From<u8>,
    <Value as FromStr>::Err: Display,
{
    let parsed_value = parse_variable::<Value>(variable_name, variable_value)?;
    if parsed_value == Value::from(0) {
        return Err(EnvironmentError::InvalidEnvironmentVariable {
            message: String::from(MESSAGE_VALUE_MUST_BE_GREATER_THAN_ZERO),
            variable_name,
        });
    }
    Ok(parsed_value)
}

pub(super) fn parse_variable<Value>(
    variable_name: &'static str,
    variable_value: &str,
) -> Result<Value, EnvironmentError>
where
    Value: FromStr,
    <Value as FromStr>::Err: Display,
{
    variable_value.parse::<Value>().map_err(|parse_error| {
        EnvironmentError::InvalidEnvironmentVariable {
            message: parse_error.to_string(),
            variable_name,
        }
    })
}

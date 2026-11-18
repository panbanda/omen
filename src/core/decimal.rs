//! Serialization of a 32-bit float as the decimal that the code has.
//!
//! JSON has 64-bit numbers. A direct conversion of the `f32` value `0.3`
//! gives `0.30000001192092896`. The functions of this module give `0.3`,
//! which is the shortest decimal that has the same `f32` value.

use std::collections::{BTreeMap, HashMap};

use serde::{Serialize, Serializer};

/// The shortest decimal that gives `value` again, as a 64-bit float.
pub fn shortest(value: f32) -> f64 {
    if !value.is_finite() {
        return f64::from(value);
    }
    value.to_string().parse().unwrap_or(f64::from(value))
}

/// Use with `#[serde(serialize_with = "...")]` on an `f32` field.
pub fn f32_as_decimal<S: Serializer>(value: &f32, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_f64(shortest(*value))
}

/// Use with `#[serde(serialize_with = "...")]` on a map of `f32` values. The keys are in order.
pub fn f32_map_as_decimal<S: Serializer>(
    values: &HashMap<String, f32>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    values
        .iter()
        .map(|(key, value)| (key.as_str(), shortest(*value)))
        .collect::<BTreeMap<&str, f64>>()
        .serialize(serializer)
}

/// A value with two decimal places, without the noise of the binary sum:
/// `0.9 + 0.05` gives `0.95`, not `0.9500000000000001`.
pub fn round_to_hundredths(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct Row {
        #[serde(serialize_with = "f32_as_decimal")]
        weight: f32,
        #[serde(serialize_with = "f32_map_as_decimal")]
        factors: HashMap<String, f32>,
    }

    #[test]
    fn test_f32_is_the_decimal_of_the_source_in_json() {
        let row = Row {
            weight: 0.3,
            factors: HashMap::from([("churn".to_string(), 0.239), ("age".to_string(), 0.85)]),
        };
        assert_eq!(
            serde_json::to_string(&row).unwrap(),
            r#"{"weight":0.3,"factors":{"age":0.85,"churn":0.239}}"#
        );
        // The direct conversion has the noise.
        assert_eq!(serde_json::to_string(&0.3f32).unwrap(), "0.3");
        assert_eq!(
            serde_json::to_value(0.3f32).unwrap().to_string(),
            "0.30000001192092896"
        );
    }

    #[test]
    fn test_shortest_keeps_the_value_and_the_special_values() {
        for value in [
            0.0f32,
            1.0,
            0.1,
            0.2,
            0.3,
            0.05,
            0.086_274_18,
            1e-7,
            123_456.79,
        ] {
            assert_eq!(shortest(value) as f32, value);
        }
        assert!(shortest(f32::NAN).is_nan());
        assert_eq!(shortest(f32::INFINITY), f64::INFINITY);
    }

    #[test]
    fn test_round_to_hundredths_removes_the_noise_of_a_sum() {
        assert_eq!(round_to_hundredths(0.9 + 0.05), 0.95);
        assert_eq!(round_to_hundredths(0.8), 0.8);
        assert_eq!(
            serde_json::to_string(&round_to_hundredths(0.9 + 0.05)).unwrap(),
            "0.95"
        );
    }
}

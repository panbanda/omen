use super::*;

/// A JSON number that omen wrote must read back to the same bits. Without the exact parser,
/// `serde_json` can read a different `f64` for some text. A score from the parsed files could
/// then differ in the last digit from the score of the same run in memory.
#[test]
fn test_a_json_float_reads_back_to_the_same_bits() {
    for value in [
        99.61538461538461_f64,
        0.1 + 0.2,
        1.0 / 3.0,
        2.0_f64.sqrt() * 41.0,
    ] {
        let text = serde_json::to_string(&value).unwrap();
        let read: f64 = serde_json::from_str(&text).unwrap();
        assert_eq!(read.to_bits(), value.to_bits(), "{text}");
    }
}

#[test]
fn test_the_score_is_bit_identical_in_each_run() {
    let satd = crate::analyzers::satd::Analysis::default();
    let score = || {
        compute_from_components(
            &Components {
                satd: Some(&satd),
                ..Default::default()
            },
            7,
        )
        .unwrap()
    };
    let first = score();
    for _ in 0..50 {
        let again = score();
        assert_eq!(again.overall_score.to_bits(), first.overall_score.to_bits());
        let mut names: Vec<_> = again.components.keys().collect();
        names.sort();
        let mut first_names: Vec<_> = first.components.keys().collect();
        first_names.sort();
        assert_eq!(names, first_names);
    }
}

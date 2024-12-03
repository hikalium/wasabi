use crate::result::Result;
use core::ops::RangeInclusive;

pub fn map_value_in_range_inclusive(
    from: RangeInclusive<i64>,
    to: RangeInclusive<i64>,
    v: i64,
) -> Result<i64> {
    if !from.contains(&v) {
        Err("v is not in range from")
    } else {
        let from_left = (v - *from.start()) as i128;
        let from_width = (from.end() - from.start()) as i128;
        let to_width = (to.end() - to.start()) as i128;
        if from_width == 0 {
            Ok(*to.start())
        } else {
            let to_left = from_left * to_width / from_width;
            to_left
                .try_into()
                .or(Err("failed to convert to_left to the result type"))
                .map(|to_left: i64| to.start() + to_left)
        }
    }
}

#[test_case]
fn map_value_in_range_inclusive_test() {
    assert_eq!(map_value_in_range_inclusive(0..=0, 0..=0, 0), Ok(0));
    assert_eq!(map_value_in_range_inclusive(0..=0, 1..=1, 0), Ok(1));
    assert_eq!(map_value_in_range_inclusive(0..=0, -1..=-1, 0), Ok(-1));
    assert!(matches!(
        map_value_in_range_inclusive(1..=1, 0..=0, 0),
        Err(_)
    ));

    assert_eq!(map_value_in_range_inclusive(0..=3, 0..=270, 0), Ok(0));
    assert_eq!(map_value_in_range_inclusive(0..=3, 0..=270, 1), Ok(90));
    assert_eq!(map_value_in_range_inclusive(0..=3, 0..=270, 2), Ok(180));
    assert_eq!(map_value_in_range_inclusive(0..=3, 0..=270, 3), Ok(270));

    assert_eq!(map_value_in_range_inclusive(0..=3, 5..=275, 0), Ok(5));
    assert_eq!(map_value_in_range_inclusive(0..=3, 5..=275, 1), Ok(95));
    assert_eq!(map_value_in_range_inclusive(0..=3, 5..=275, 2), Ok(185));
    assert_eq!(map_value_in_range_inclusive(0..=3, 5..=275, 3), Ok(275));

    assert_eq!(map_value_in_range_inclusive(-2..=2, -10..=10, -2), Ok(-10));
    assert_eq!(map_value_in_range_inclusive(-2..=2, -10..=10, -1), Ok(-5));
    assert_eq!(map_value_in_range_inclusive(-2..=2, -10..=10, 0), Ok(0));
    assert_eq!(map_value_in_range_inclusive(-2..=2, -10..=10, 1), Ok(5));
    assert_eq!(map_value_in_range_inclusive(-2..=2, -10..=10, 2), Ok(10));
}

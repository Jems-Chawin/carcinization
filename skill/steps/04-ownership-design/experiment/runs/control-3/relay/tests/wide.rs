//! More results computed by `relay.py`, for networks wider than one word of
//! the busy bitset. Each state is reduced to (length, weighted checksum).

use relay::run;

fn digest(state: &[i64]) -> (usize, u64) {
    let sum = state.iter().enumerate().fold(0u64, |acc, (k, &v)| {
        acc.wrapping_mul(1_000_003)
            .wrapping_add((k as u64 + 1).wrapping_mul(v as u64))
    });
    (state.len(), sum)
}

#[test]
fn wide_networks_match_python() {
    let cases: [(usize, u64, usize, usize, u64); 10] = [
        (65, 5, 500, 394, 15738912081844909797),
        (127, 11, 2000, 766, 16260575928729115364),
        (128, 3, 2000, 772, 11105999054663122425),
        (129, 8, 3000, 778, 2822018563855840984),
        (200, 42, 5000, 1204, 15894574079309262387),
        (1000, 77, 20000, 6004, 13213471255211290094),
        (4097, 9, 30000, 24586, 14829663027252231296),
        (3, 18446744073709551615, 5000, 22, 13290555131655676496),
        (2, 0, 3000, 16, 12005936157682868062),
        (7, 12345678901234567890, 20000, 46, 1538632454200724779),
    ];
    for (count, seed, ticks, len, sum) in cases {
        assert_eq!(
            digest(&run(count, seed, ticks)),
            (len, sum),
            "count {count} seed {seed} ticks {ticks}"
        );
    }
}

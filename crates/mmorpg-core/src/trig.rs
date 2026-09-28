//! Integer trigonometry for authoritative yaw.
//!
//! A yaw is a `u16`: 65 536 steps are one full turn. Yaw 0 faces +Z and
//! increasing yaw turns toward +X, so `direction(yaw) = (sin yaw, cos yaw)`.
//! Results are Q16 fixed point (`TRIG_ONE` is 1.0). The quarter-wave table is
//! generated at compile time with integer arithmetic only; no floating point
//! participates in table generation or in any lookup.

/// One quarter turn in yaw steps (90°).
pub const YAW_QUARTER_TURN: u16 = 16_384;
/// One eighth turn in yaw steps (45°).
pub const YAW_EIGHTH_TURN: u16 = 8_192;
/// Q16 fixed-point representation of 1.0 for trigonometric results.
pub const TRIG_ONE: i32 = 1 << 16;

const QUARTER_STEPS: usize = YAW_QUARTER_TURN as usize;
const TABLE_FRACTION_BITS: u32 = 60;
const TABLE_ONE: i128 = 1 << TABLE_FRACTION_BITS;
/// π in Q60, rounded to nearest: 3.14159265358979323846… × 2^60.
const PI_Q60: i128 = 3_622_009_729_038_561_421;
/// Taylor terms after the leading `x`; the truncation error at π/2 is below 2^-50.
const SINE_SERIES_TERMS: i128 = 10;

/// `sin` over the closed first quadrant, index `i` meaning `i` yaw steps.
static QUARTER_SINE: [i32; QUARTER_STEPS + 1] = quarter_sine_table();

const fn quarter_sine_table() -> [i32; QUARTER_STEPS + 1] {
    let mut table = [0; QUARTER_STEPS + 1];
    let mut index = 0;
    while index <= QUARTER_STEPS {
        table[index] = quarter_sine_q16(index as i128);
        index += 1;
    }
    table
}

/// Q16 sine of `steps` yaw steps in `0..=16_384`, computed in Q60 with a
/// Horner-form Taylor series: `x (1 - x²/(2·3) (1 - x²/(4·5) (…)))`.
const fn quarter_sine_q16(steps: i128) -> i32 {
    // x = steps · (π/2) / 16_384 = steps · π / 32_768, rounded to nearest.
    let quarter_turns_denominator = 2 * QUARTER_STEPS as i128;
    let x = (steps * PI_Q60 + quarter_turns_denominator / 2) / quarter_turns_denominator;
    let x_squared = (x * x) >> TABLE_FRACTION_BITS;
    let mut factor = TABLE_ONE;
    let mut term = SINE_SERIES_TERMS;
    while term > 0 {
        let divisor = (2 * term) * (2 * term + 1);
        factor = TABLE_ONE - ((x_squared * factor) >> TABLE_FRACTION_BITS) / divisor;
        term -= 1;
    }
    let sine = (x * factor) >> TABLE_FRACTION_BITS;
    let shift = TABLE_FRACTION_BITS - 16;
    ((sine + (1 << (shift - 1))) >> shift) as i32
}

/// Q16 sine of a yaw.
#[must_use]
pub const fn sin(yaw: u16) -> i32 {
    let offset = (yaw % YAW_QUARTER_TURN) as usize;
    match yaw / YAW_QUARTER_TURN {
        0 => QUARTER_SINE[offset],
        1 => QUARTER_SINE[QUARTER_STEPS - offset],
        2 => -QUARTER_SINE[offset],
        _ => -QUARTER_SINE[QUARTER_STEPS - offset],
    }
}

/// Q16 cosine of a yaw.
#[must_use]
pub const fn cos(yaw: u16) -> i32 {
    sin(yaw.wrapping_add(YAW_QUARTER_TURN))
}

/// Q16 unit vector `(x, z)` a character with this yaw faces.
#[must_use]
pub const fn direction(yaw: u16) -> (i32, i32) {
    (sin(yaw), cos(yaw))
}

/// Scales a Q16 unit component by an integer magnitude, rounding half away
/// from zero so opposite directions produce exactly negated results.
///
/// Returns `None` when the rounded result does not fit in `i32`, for example
/// `i32::MIN` scaled by `-TRIG_ONE`; it never wraps.
#[must_use]
pub const fn checked_scale(magnitude: i32, unit: i32) -> Option<i32> {
    // |magnitude · unit| <= 2^62, so the product and the rounding never overflow.
    let product = magnitude as i64 * unit as i64;
    let half = (TRIG_ONE / 2) as i64;
    let rounded = if product >= 0 {
        (product + half) / TRIG_ONE as i64
    } else {
        (product - half) / TRIG_ONE as i64
    };
    if rounded < i32::MIN as i64 || rounded > i32::MAX as i64 {
        None
    } else {
        Some(rounded as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HALF_TURN: u16 = 2 * YAW_QUARTER_TURN;

    /// Floating point is a test-only oracle; production lookups never use it.
    fn reference_q16(yaw: u16) -> (f64, f64) {
        let radians = f64::from(yaw) * std::f64::consts::TAU / 65_536.0;
        let one = f64::from(TRIG_ONE);
        (radians.sin() * one, radians.cos() * one)
    }

    #[test]
    fn every_yaw_is_correctly_rounded_against_a_floating_point_oracle() {
        for yaw in 0..=u16::MAX {
            let (x, z) = direction(yaw);
            let (expected_x, expected_z) = reference_q16(yaw);
            // Correct rounding: within half a Q16 step of the exact value.
            assert!(
                (f64::from(x) - expected_x).abs() <= 0.5 + 1e-9,
                "sin({yaw}) = {x}, expected {expected_x}"
            );
            assert!(
                (f64::from(z) - expected_z).abs() <= 0.5 + 1e-9,
                "cos({yaw}) = {z}, expected {expected_z}"
            );
        }
    }

    #[test]
    fn cardinal_yaws_are_exact_and_follow_the_zone_convention() {
        assert_eq!(direction(0), (0, TRIG_ONE), "yaw 0 faces +Z");
        assert_eq!(direction(YAW_QUARTER_TURN), (TRIG_ONE, 0), "90° faces +X");
        assert_eq!(direction(HALF_TURN), (0, -TRIG_ONE), "180° faces -Z");
        assert_eq!(
            direction(3 * YAW_QUARTER_TURN),
            (-TRIG_ONE, 0),
            "270° faces -X"
        );
        let (diagonal_x, diagonal_z) = direction(YAW_EIGHTH_TURN);
        assert_eq!(diagonal_x, diagonal_z);
        assert_eq!(diagonal_x, 46_341, "sin 45° rounds to 46 341 / 65 536");
    }

    #[test]
    fn quadrant_symmetries_hold_for_every_yaw() {
        for yaw in 0..=u16::MAX {
            let opposite = yaw.wrapping_add(HALF_TURN);
            assert_eq!(sin(opposite), -sin(yaw));
            assert_eq!(cos(opposite), -cos(yaw));
            assert_eq!(sin(HALF_TURN.wrapping_sub(yaw)), sin(yaw));
            assert_eq!(sin(0_u16.wrapping_sub(yaw)), -sin(yaw));
            assert_eq!(cos(0_u16.wrapping_sub(yaw)), cos(yaw));
            assert_eq!(cos(yaw), sin(yaw.wrapping_add(YAW_QUARTER_TURN)));
            let (x, z) = direction(yaw);
            let length_squared = i64::from(x).pow(2) + i64::from(z).pow(2);
            let one_squared = i64::from(TRIG_ONE).pow(2);
            // Two rounded components change x² + z² by at most about 2·ONE.
            assert!((length_squared - one_squared).abs() <= 2 * i64::from(TRIG_ONE));
        }
    }

    #[test]
    fn first_quadrant_sine_is_monotonic_and_bounded() {
        for yaw in 1..=YAW_QUARTER_TURN {
            assert!(sin(yaw) >= sin(yaw - 1));
            assert!((0..=TRIG_ONE).contains(&sin(yaw)));
        }
    }

    #[test]
    fn scaling_rounds_half_away_from_zero_symmetrically() {
        assert_eq!(checked_scale(21, TRIG_ONE), Some(21));
        assert_eq!(checked_scale(21, -TRIG_ONE), Some(-21));
        assert_eq!(checked_scale(21, 0), Some(0));
        assert_eq!(
            checked_scale(21, TRIG_ONE / 2),
            Some(11),
            "10.5 rounds away from zero"
        );
        assert_eq!(checked_scale(21, -TRIG_ONE / 2), Some(-11));
        assert_eq!(checked_scale(21, 46_341), Some(15));
        for unit in [-TRIG_ONE, -46_341, -1, 1, 777, 46_341, TRIG_ONE] {
            let scaled = checked_scale(13, unit).unwrap();
            assert_eq!(checked_scale(13, -unit), Some(-scaled));
        }
    }

    #[test]
    fn scaling_reports_results_outside_i32_instead_of_wrapping() {
        assert_eq!(checked_scale(i32::MAX, TRIG_ONE), Some(i32::MAX));
        assert_eq!(checked_scale(i32::MAX, -TRIG_ONE), Some(-i32::MAX));
        assert_eq!(checked_scale(i32::MIN, TRIG_ONE), Some(i32::MIN));
        // An ordinary lookup yields exactly -1.0: cos 180° and sin 270°.
        let (_, toward_negative_z) = direction(HALF_TURN);
        let (toward_negative_x, _) = direction(3 * YAW_QUARTER_TURN);
        assert_eq!(toward_negative_z, -TRIG_ONE);
        assert_eq!(toward_negative_x, -TRIG_ONE);
        assert_eq!(
            checked_scale(i32::MIN, toward_negative_z),
            None,
            "+2^31 must not wrap"
        );
        assert_eq!(checked_scale(i32::MIN, toward_negative_x), None);
        // Units beyond ±1.0 are outside a direction's range but still never wrap.
        assert_eq!(checked_scale(i32::MAX, TRIG_ONE + 1), None);
        assert_eq!(checked_scale(i32::MIN, i32::MIN), None);
        assert_eq!(checked_scale(1, i32::MIN), Some(-32_768));
    }
}

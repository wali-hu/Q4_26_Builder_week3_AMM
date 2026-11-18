//! Constant product (x * y = k) math, written without the curve library.
//!
//! Every function works in u128 so products of two u64 values cannot overflow,
//! returns None on overflow or division by zero, and rounds in the POOL's favour:
//! - amounts the pool PAYS OUT (swap output, withdraw) are rounded down,
//! - amounts the pool TAKES IN (deposit, fee) are rounded up.
//! So rounding can never drain the pool, one unit at a time.

use crate::constants::BPS;

/// Output of a swap with the fee already removed from `amount_in`:
/// out = floor(reserve_out * amount_in / (reserve_in + amount_in)).
/// Same as y - k / (x + a), but rounded down instead of up.
pub fn swap_out(reserve_in: u64, reserve_out: u64, amount_in: u64) -> Option<u64> {
    let num = (reserve_out as u128).checked_mul(amount_in as u128)?;
    let den = (reserve_in as u128).checked_add(amount_in as u128)?;
    u64::try_from(num.checked_div(den)?).ok()
}

/// Splits the swap fee. Returns (total_fee, protocol_fee):
/// total_fee    = ceil(amount_in * fee_bps / 10_000)   (rounded up: pool favour)
/// protocol_fee = floor(total_fee * protocol_bps / 10_000)
/// The rest of the fee (total_fee - protocol_fee) stays in the pool for LPs.
pub fn fee_split(amount_in: u64, fee_bps: u16, protocol_bps: u16) -> Option<(u64, u64)> {
    let total = ceil_div((amount_in as u128).checked_mul(fee_bps as u128)?, BPS as u128)?;
    let protocol = total.checked_mul(protocol_bps as u128)? / BPS as u128;
    Some((u64::try_from(total).ok()?, u64::try_from(protocol).ok()?))
}

/// LP tokens for the very first deposit: sqrt(x * y), rounded down.
/// Using the geometric mean makes the LP amount independent of the price ratio.
pub fn initial_lp(x: u64, y: u64) -> Option<u64> {
    let k = (x as u128).checked_mul(y as u128)?;
    u64::try_from(isqrt(k)).ok()
}

/// Tokens a depositor must add to mint `lp` new LP tokens, keeping the ratio:
/// x_in = ceil(lp * x / supply), y_in = ceil(lp * y / supply)  (rounded up).
pub fn deposit_amounts(x: u64, y: u64, supply: u64, lp: u64) -> Option<(u64, u64)> {
    let dx = ceil_div((lp as u128).checked_mul(x as u128)?, supply as u128)?;
    let dy = ceil_div((lp as u128).checked_mul(y as u128)?, supply as u128)?;
    Some((u64::try_from(dx).ok()?, u64::try_from(dy).ok()?))
}

/// Tokens returned for burning `lp` LP tokens:
/// x_out = floor(lp * x / supply), y_out = floor(lp * y / supply)  (rounded down).
pub fn withdraw_amounts(x: u64, y: u64, supply: u64, lp: u64) -> Option<(u64, u64)> {
    let dx = (lp as u128).checked_mul(x as u128)?.checked_div(supply as u128)?;
    let dy = (lp as u128).checked_mul(y as u128)?.checked_div(supply as u128)?;
    Some((u64::try_from(dx).ok()?, u64::try_from(dy).ok()?))
}

fn ceil_div(a: u128, b: u128) -> Option<u128> {
    if b == 0 {
        return None;
    }
    a.checked_add(b - 1).map(|n| n / b)
}

/// Integer square root (largest r with r * r <= n), Newton's method.
pub fn isqrt(n: u128) -> u128 {
    if n < 2 {
        return n;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;
    use constant_product_curve::{ConstantProduct, LiquidityPair};

    #[test]
    fn slide_example_k_600() {
        // x = 20, y = 30, k = 600. Swap 5 X -> 6 Y (no fee).
        assert_eq!(swap_out(20, 30, 5), Some(6));
        // Now x = 25, y = 24 (k still 600). Swap 5 X -> 4 Y.
        assert_eq!(swap_out(25, 24, 5), Some(4));
    }

    #[test]
    fn swap_output_rounds_down() {
        // exact output = 1000 * 7 / 107 = 65.42..., pool pays 65
        assert_eq!(swap_out(100, 1000, 7), Some(65));
    }

    #[test]
    fn fee_rounds_up_and_protocol_share_rounds_down() {
        // 30 bps of 1_000 = 3 exactly; protocol 20% of 3 = 0.6 -> 0
        assert_eq!(fee_split(1_000, 30, 2_000), Some((3, 0)));
        // 30 bps of 1_001 = 3.003 -> 4 (up); protocol 20% of 4 = 0.8 -> 0
        assert_eq!(fee_split(1_001, 30, 2_000), Some((4, 0)));
        // 1_000_000 at 30 bps = 3_000; protocol 20% = 600, LPs keep 2_400
        assert_eq!(fee_split(1_000_000, 30, 2_000), Some((3_000, 600)));
        assert_eq!(fee_split(1_000_000, 0, 2_000), Some((0, 0)));
    }

    #[test]
    fn lp_math() {
        assert_eq!(initial_lp(100, 400), Some(200));
        assert_eq!(initial_lp(2, 3), Some(2)); // sqrt(6) = 2.44 -> 2
        assert_eq!(isqrt(u64::MAX as u128 * u64::MAX as u128), u64::MAX as u128);
        // pool 1000 X / 2000 Y / 1000 LP: 10 LP needs 10 X and 20 Y
        assert_eq!(deposit_amounts(1_000, 2_000, 1_000, 10), Some((10, 20)));
        // uneven: 1 LP of (1001, 1999, 1000) -> ceil(1.001)=2, ceil(1.999)=2
        assert_eq!(deposit_amounts(1_001, 1_999, 1_000, 1), Some((2, 2)));
        // withdraw rounds down: floor(1.001)=1, floor(1.999)=1
        assert_eq!(withdraw_amounts(1_001, 1_999, 1_000, 1), Some((1, 1)));
    }

    #[test]
    fn division_by_zero_and_overflow_return_none() {
        assert_eq!(swap_out(0, 0, 0), None);
        assert_eq!(deposit_amounts(1, 1, 0, 1), None);
        assert_eq!(withdraw_amounts(1, 1, 0, 1), None);
    }

    // ---- comparison with the constant-product-curve library ----

    #[test]
    fn swap_matches_library_or_is_one_unit_lower() {
        // The library computes y - floor(k / (x + a)), which rounds the output UP.
        // Ours rounds down, so it is equal or exactly 1 unit lower, never higher.
        let cases = [
            (20u64, 30u64, 5u64),
            (25, 24, 5),
            (100, 1_000, 7),
            (1_000_000, 2_000_000, 12_345),
            (5_000_000_000, 3_000_000_000, 999_999),
            (7, 13, 3),
        ];
        for (x, y, a) in cases {
            let lib = ConstantProduct::delta_y_from_x_swap_amount(x, y, a).unwrap();
            let ours = swap_out(x, y, a).unwrap();
            assert!(ours == lib || ours + 1 == lib, "x={x} y={y} a={a}: ours {ours} lib {lib}");
        }
        // (100, 1000, 7): exact 65.42; library says 66 (pool loses), ours 65
        assert_eq!(ConstantProduct::delta_y_from_x_swap_amount(100, 1_000, 7).unwrap(), 66);
    }

    #[test]
    fn library_skips_the_fee_on_y_to_x_swaps() {
        // Same pool, same 1% fee, same 1_000 input. X -> Y charges the fee,
        // but the library's Y branch uses the raw amount, so Y -> X pays no fee.
        let mut c = ConstantProduct::init(1_000_000, 1_000_000, 1_000_000, 100, None).unwrap();
        let y_to_x = c.swap(LiquidityPair::Y, 1_000, 0).unwrap().withdraw;
        let no_fee = ConstantProduct::delta_x_from_y_swap_amount(1_000_000, 1_000_000, 1_000).unwrap();
        assert_eq!(y_to_x, no_fee, "library: Y -> X output equals the no-fee output");

        // Ours charges the fee in both directions.
        let (fee, _) = fee_split(1_000, 100, 0).unwrap();
        let ours = swap_out(1_000_000, 1_000_000, 1_000 - fee).unwrap();
        assert!(ours < no_fee);
    }

    #[test]
    fn library_precision_is_a_multiplier_not_decimals() {
        // Pool: 1_000 X, 2_000 Y, 1_000 LP. Burn or mint 100 LP (10%).
        // Correct answer: 100 X and 200 Y.
        assert_eq!(withdraw_amounts(1_000, 2_000, 1_000, 100), Some((100, 200)));
        assert_eq!(deposit_amounts(1_000, 2_000, 1_000, 100), Some((100, 200)));

        // The library computes a ratio scaled by `precision`. Passing 6 (as if it
        // meant "6 decimals", which the class reference AMM does) makes the ratio
        // floor(900 * 6 / 1000) = 5, i.e. 5/6 instead of 9/10:
        let w = ConstantProduct::xy_withdraw_amounts_from_l(1_000, 2_000, 1_000, 100, 6).unwrap();
        assert_eq!((w.x, w.y), (167, 334), "withdraw pays 16.7% for 10% of the LP");
        // and floor(1100 * 6 / 1000) = 6 = exactly 6/6, so a deposit costs nothing:
        let d = ConstantProduct::xy_deposit_amounts_from_l(1_000, 2_000, 1_000, 100, 6).unwrap();
        assert_eq!((d.x, d.y), (0, 0), "100 LP for free");

        // With a real scale factor the library agrees with our math.
        let w = ConstantProduct::xy_withdraw_amounts_from_l(1_000, 2_000, 1_000, 100, 1_000_000).unwrap();
        assert_eq!(withdraw_amounts(1_000, 2_000, 1_000, 100), Some((w.x, w.y)));
        let d = ConstantProduct::xy_deposit_amounts_from_l(1_000, 2_000, 1_000, 100, 1_000_000).unwrap();
        assert_eq!(deposit_amounts(1_000, 2_000, 1_000, 100), Some((d.x, d.y)));
    }
}

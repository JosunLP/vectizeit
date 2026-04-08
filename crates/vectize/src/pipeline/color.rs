//! Shared color-space helpers used by segmentation and gradient fitting.

/// A color represented in the perceptually-uniform Oklab space.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct OklabColor {
    pub l: f64,
    pub a: f64,
    pub b: f64,
}

/// Convert an sRGB triplet to linear RGB.
pub(crate) fn srgb_to_linear_rgb(color: [u8; 3]) -> [f64; 3] {
    [
        srgb_to_linear_component(color[0]),
        srgb_to_linear_component(color[1]),
        srgb_to_linear_component(color[2]),
    ]
}

/// Convert a linear RGB triplet to sRGB.
pub(crate) fn linear_rgb_to_srgb(color: [f64; 3]) -> [u8; 3] {
    [
        linear_to_srgb_component(color[0]),
        linear_to_srgb_component(color[1]),
        linear_to_srgb_component(color[2]),
    ]
}

/// Convert linear RGB to Oklab.
pub(crate) fn linear_rgb_to_oklab(color: [f64; 3]) -> OklabColor {
    let l =
        (0.412_221_470_8 * color[0]) + (0.536_332_536_3 * color[1]) + (0.051_445_992_9 * color[2]);
    let m =
        (0.211_903_498_2 * color[0]) + (0.680_699_545_1 * color[1]) + (0.107_396_956_6 * color[2]);
    let s =
        (0.088_302_461_9 * color[0]) + (0.281_718_837_6 * color[1]) + (0.629_978_700_5 * color[2]);

    let l_root = l.max(0.0).cbrt();
    let m_root = m.max(0.0).cbrt();
    let s_root = s.max(0.0).cbrt();

    OklabColor {
        l: (0.210_454_255_3 * l_root) + (0.793_617_785_0 * m_root) - (0.004_072_046_8 * s_root),
        a: (1.977_998_495_1 * l_root) - (2.428_592_205_0 * m_root) + (0.450_593_709_9 * s_root),
        b: (0.025_904_037_1 * l_root) + (0.782_771_766_2 * m_root) - (0.808_675_766_0 * s_root),
    }
}

/// Convert sRGB directly to Oklab.
pub(crate) fn srgb_to_oklab(color: [u8; 3]) -> OklabColor {
    linear_rgb_to_oklab(srgb_to_linear_rgb(color))
}

/// Squared Euclidean distance in Oklab space.
pub(crate) fn oklab_distance_sq(left: OklabColor, right: OklabColor) -> f64 {
    let dl = left.l - right.l;
    let da = left.a - right.a;
    let db = left.b - right.b;
    (dl * dl) + (da * da) + (db * db)
}

/// Linearly interpolate between two linear RGB colors.
pub(crate) fn lerp_linear_rgb(left: [f64; 3], right: [f64; 3], t: f64) -> [f64; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        left[0] + ((right[0] - left[0]) * t),
        left[1] + ((right[1] - left[1]) * t),
        left[2] + ((right[2] - left[2]) * t),
    ]
}

fn srgb_to_linear_component(component: u8) -> f64 {
    let value = f64::from(component) / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb_component(component: f64) -> u8 {
    let value = component.clamp(0.0, 1.0);
    let srgb = if value <= 0.003_130_8 {
        value * 12.92
    } else {
        (1.055 * value.powf(1.0 / 2.4)) - 0.055
    };

    (srgb * 255.0).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_rgb_round_trip_stays_close() {
        let color = [32, 128, 224];
        let round_tripped = linear_rgb_to_srgb(srgb_to_linear_rgb(color));

        assert_eq!(round_tripped, color);
    }

    #[test]
    fn oklab_distance_is_zero_for_identical_colors() {
        let color = srgb_to_oklab([120, 80, 40]);
        assert!(oklab_distance_sq(color, color) <= f64::EPSILON);
    }
}

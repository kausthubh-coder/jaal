//! jaal's colors and sizes, from docs/design/jaal.html.
//!
//! A space sets two hues: one tints the chrome, one colors the accent. Every
//! other color is computed from them in OKLCH, once for light and once for
//! dark, so a new space is two numbers.

use gpui::{BoxShadow, Hsla, Rgba, point, px};

/// The sidebar's width and the frame of chrome around the page, in DIPs.
pub const SIDEBAR_WIDTH: i32 = 236;
pub const PAGE_FRAME: i32 = 8;
pub const PAGE_RADIUS: f32 = 10.;

/// What the design's `system-ui` resolves to on Windows 11.
pub const FONT: &str = "Segoe UI Variable Text";

/// An OKLCH color as sRGB. Channels outside sRGB are clipped.
pub fn oklch(lightness: f32, chroma: f32, hue_degrees: f32, alpha: f32) -> Rgba {
    let hue = hue_degrees.to_radians();
    let (a, b) = (chroma * hue.cos(), chroma * hue.sin());
    // OKLab to linear sRGB, Björn Ottosson's matrices.
    let l = (lightness + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m = (lightness - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s = (lightness - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    let gamma = |x: f32| {
        let x = x.clamp(0., 1.);
        if x <= 0.003_130_8 {
            12.92 * x
        } else {
            1.055 * x.powf(1. / 2.4) - 0.055
        }
    };
    Rgba {
        r: gamma(4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s),
        g: gamma(-1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s),
        b: gamma(-0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s),
        a: alpha,
    }
}

/// `0xAARRGGBB`, the form the engine takes colors in.
pub fn argb(color: Rgba) -> u32 {
    let byte = |x: f32| (x.clamp(0., 1.) * 255.).round() as u32;
    byte(color.a) << 24 | byte(color.r) << 16 | byte(color.g) << 8 | byte(color.b)
}

/// Every color the UI uses, named for its job.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub dark: bool,
    /// The chrome is a top-to-bottom gradient between these two.
    pub chrome_top: Rgba,
    pub chrome_bottom: Rgba,
    /// Text: primary, secondary, and hints or icons at rest.
    pub ink: Rgba,
    pub ink_2: Rgba,
    pub ink_3: Rgba,
    /// A resting fill (the address field), a hover fill, a hairline.
    pub fill: Rgba,
    pub fill_hover: Rgba,
    pub line: Rgba,
    pub accent: Rgba,
    pub accent_ink: Rgba,
    pub accent_fill: Rgba,
    /// The one raised thing in the sidebar: the active tab.
    pub raised: Rgba,
    pub raised_shadow: Vec<BoxShadow>,
    /// The command bar's glass and the shadow under it.
    pub glass: Rgba,
    pub glass_shadow: Vec<BoxShadow>,
    /// The page card's shadow and hairline, drawn by the engine.
    pub card_shadow: Rgba,
    pub card_outline: Rgba,
}

fn shadow(color: Rgba, y: f32, blur: f32, spread: f32) -> BoxShadow {
    BoxShadow {
        color: Hsla::from(color),
        offset: point(px(0.), px(y)),
        blur_radius: px(blur),
        spread_radius: px(spread),
    }
}

impl Theme {
    /// `hue` tints the chrome and `accent_hue` colors the accent, both in
    /// OKLCH degrees.
    pub fn new(hue: f32, accent_hue: f32, dark: bool) -> Self {
        let h = hue;
        let ha = accent_hue;
        let white = |alpha: f32| oklch(1., 0., 0., alpha);
        let black = |alpha: f32| oklch(0., 0., 0., alpha);
        if dark {
            Self {
                dark,
                chrome_top: oklch(0.255, 0.016, h, 1.),
                chrome_bottom: oklch(0.215, 0.018, h, 1.),
                ink: oklch(0.94, 0.006, h, 1.),
                ink_2: oklch(0.74, 0.012, h, 1.),
                ink_3: oklch(0.55, 0.014, h, 1.),
                fill: white(0.075),
                fill_hover: white(0.045),
                line: white(0.1),
                accent: oklch(0.76, 0.13, ha, 1.),
                accent_ink: oklch(0.83, 0.11, ha, 1.),
                accent_fill: oklch(0.76, 0.13, ha, 0.2),
                raised: white(0.15),
                raised_shadow: vec![
                    shadow(white(0.07), 0., 0., 0.5),
                    shadow(black(0.3), 1., 2., 0.),
                ],
                glass: oklch(0.27, 0.014, h, 0.72),
                glass_shadow: vec![
                    shadow(white(0.18), 0., 0., 0.5),
                    shadow(black(0.55), 28., 64., 0.),
                ],
                card_shadow: black(0.35),
                card_outline: white(0.1),
            }
        } else {
            Self {
                dark,
                chrome_top: oklch(0.932, 0.014, h, 1.),
                chrome_bottom: oklch(0.9, 0.02, h, 1.),
                ink: oklch(0.27, 0.012, h, 1.),
                ink_2: oklch(0.52, 0.018, h, 1.),
                ink_3: oklch(0.69, 0.022, h, 1.),
                fill: oklch(0.35, 0.04, h, 0.085),
                fill_hover: oklch(0.35, 0.04, h, 0.05),
                line: oklch(0.35, 0.04, h, 0.14),
                accent: oklch(0.58, 0.15, ha, 1.),
                accent_ink: oklch(0.47, 0.14, ha, 1.),
                accent_fill: oklch(0.58, 0.15, ha, 0.14),
                raised: white(1.),
                raised_shadow: vec![
                    shadow(oklch(0.3, 0.04, h, 0.12), 0., 0., 0.5),
                    shadow(oklch(0.3, 0.04, h, 0.16), 1., 2.5, 0.),
                ],
                glass: oklch(0.975, 0.006, h, 0.74),
                glass_shadow: vec![
                    shadow(oklch(0.25, 0.03, h, 0.24), 0., 0., 0.5),
                    shadow(oklch(0.25, 0.03, h, 0.08), 2., 6., 0.),
                    shadow(oklch(0.25, 0.03, h, 0.32), 28., 64., 0.),
                ],
                card_shadow: oklch(0.3, 0.04, h, 0.12),
                card_outline: oklch(0.3, 0.04, h, 0.16),
            }
        }
    }

    /// The first space in the design: "Jaal".
    pub fn jaal(dark: bool) -> Self {
        Self::new(75., 42., dark)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(color: Rgba) -> String {
        format!("#{:06x}", argb(color) & 0x00ff_ffff)
    }

    // Reference values from the same formulas in JavaScript, which match what
    // a browser paints for these `oklch()` colors in docs/design/jaal.html.
    #[test]
    fn oklch_tokens_convert_to_the_srgb_a_browser_paints() {
        assert_eq!(hex(oklch(1., 0., 0., 1.)), "#ffffff");
        assert_eq!(hex(oklch(0., 0., 0., 1.)), "#000000");
        assert_eq!(hex(oklch(0.932, 0.014, 75., 1.)), "#eee7de");
        assert_eq!(hex(oklch(0.9, 0.02, 75., 1.)), "#e6dcd0");
        assert_eq!(hex(oklch(0.27, 0.012, 75., 1.)), "#2a2620");
        assert_eq!(hex(oklch(0.58, 0.15, 42., 1.)), "#c15627");
        assert_eq!(hex(oklch(0.76, 0.13, 42., 1.)), "#f6946d");
    }

    #[test]
    fn the_jaal_space_has_the_designs_chrome_and_accent() {
        let light = Theme::jaal(false);
        assert_eq!(hex(light.chrome_top), "#eee7de");
        assert_eq!(hex(light.chrome_bottom), "#e6dcd0");
        assert_eq!(hex(light.accent), "#c15627");

        let dark = Theme::jaal(true);
        assert_eq!(hex(dark.chrome_top), "#27221a");
        assert_eq!(hex(dark.chrome_bottom), "#1e1810");
        assert_eq!(hex(dark.accent), "#f6946d");
    }

    #[test]
    fn argb_keeps_alpha_in_the_top_byte() {
        assert_eq!(
            argb(Rgba {
                r: 1.,
                g: 0.,
                b: 0.5,
                a: 0.2
            }),
            0x33ff_0080
        );
    }
}

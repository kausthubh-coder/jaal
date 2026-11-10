//! The icon set from docs/design/jaal.html: 16 px outlines, one stroke
//! weight. GPUI draws an SVG as a mask in the element's text color, so every
//! icon takes the color of the text beside it.

use gpui::{AssetSource, Result, SharedString};
use std::borrow::Cow;

pub const BACK: &str = "icons/back.svg";
pub const FORWARD: &str = "icons/forward.svg";
pub const RELOAD: &str = "icons/reload.svg";
pub const PLUS: &str = "icons/plus.svg";
pub const CLOSE: &str = "icons/close.svg";
pub const SEARCH: &str = "icons/search.svg";
pub const SITE: &str = "icons/site.svg";
pub const SPACE: &str = "icons/space.svg";
pub const LINK: &str = "icons/link.svg";
pub const WINDOW_MINIMIZE: &str = "icons/window-minimize.svg";
pub const WINDOW_MAXIMIZE: &str = "icons/window-maximize.svg";
pub const WINDOW_RESTORE: &str = "icons/window-restore.svg";
pub const WINDOW_CLOSE: &str = "icons/window-close.svg";

/// The drawing inside each icon's `<svg>`, in a 16 by 16 box. The window
/// buttons use the design's 10 by 10 glyphs with a 1 px stroke.
fn body(path: &str) -> Option<(&'static str, &'static str, &'static str)> {
    let outline = |drawing| Some(("0 0 16 16", "1.4", drawing));
    let caption = |drawing| Some(("0 0 10 10", "1", drawing));
    match path {
        BACK => outline(r#"<path d="M9.5 3.5 5 8l4.5 4.5"/>"#),
        FORWARD => outline(r#"<path d="M6.5 3.5 11 8l-4.5 4.5"/>"#),
        RELOAD => {
            outline(r#"<path d="M12.5 8a4.5 4.5 0 1 1-1.4-3.25"/><path d="M12.7 2.8v2.7H10"/>"#)
        }
        PLUS => outline(r#"<path d="M8 3.5v9M3.5 8h9"/>"#),
        CLOSE => outline(r#"<path d="m4.8 4.8 6.4 6.4m0-6.4-6.4 6.4"/>"#),
        SEARCH => outline(r#"<circle cx="7" cy="7" r="4.2"/><path d="m10.2 10.2 3 3"/>"#),
        SITE => outline(
            r#"<path d="M3 5.5h4.5m3.5 0h2M3 10.5h2m3.5 0H13"/><circle cx="9.3" cy="5.5" r="1.6"/><circle cx="6.7" cy="10.5" r="1.6"/>"#,
        ),
        SPACE => outline(r#"<path d="m6 4.5-3.5 3.5L6 11.5M10 4.5 13.5 8 10 11.5"/>"#),
        LINK => outline(
            r#"<path d="M7 9a2.5 2.5 0 0 0 3.5 0l2-2A2.5 2.5 0 0 0 9 3.5l-.8.8"/><path d="M9 7a2.5 2.5 0 0 0-3.5 0l-2 2A2.5 2.5 0 0 0 7 12.5l.8-.8"/>"#,
        ),
        WINDOW_MINIMIZE => caption(r#"<path d="M0 5.5h10"/>"#),
        WINDOW_MAXIMIZE => caption(r#"<rect x=".5" y=".5" width="9" height="9"/>"#),
        WINDOW_RESTORE => {
            caption(r#"<rect x=".5" y="2.5" width="7" height="7"/><path d="M2.5 2.5v-2h7v7h-2"/>"#)
        }
        WINDOW_CLOSE => caption(r#"<path d="m0 0 10 10M10 0 0 10"/>"#),
        _ => None,
    }
}

pub struct Icons;

impl AssetSource for Icons {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(body(path).map(|(view_box, stroke_width, drawing)| {
            Cow::Owned(
                format!(
                    r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{view_box}" fill="none" stroke="#000" stroke-width="{stroke_width}" stroke-linecap="round" stroke-linejoin="round">{drawing}</svg>"##
                )
                .into_bytes(),
            )
        }))
    }

    fn list(&self, _path: &str) -> Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_named_icon_loads_as_an_svg() {
        for path in [
            BACK,
            FORWARD,
            RELOAD,
            PLUS,
            CLOSE,
            SEARCH,
            SITE,
            SPACE,
            LINK,
            WINDOW_MINIMIZE,
            WINDOW_MAXIMIZE,
            WINDOW_RESTORE,
            WINDOW_CLOSE,
        ] {
            let bytes = Icons
                .load(path)
                .unwrap()
                .unwrap_or_else(|| panic!("{path} is missing"));
            let text = String::from_utf8(bytes.into_owned()).unwrap();
            assert!(
                text.starts_with("<svg ") && text.ends_with("</svg>"),
                "{path}: {text}"
            );
        }
    }

    #[test]
    fn an_unknown_path_is_not_an_asset() {
        assert!(Icons.load("icons/nope.svg").unwrap().is_none());
    }
}

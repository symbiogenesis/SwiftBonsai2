//! Small Windows boundary for system contrast colors and fatal launch errors.

use eframe::egui::{Color32, Shadow, Stroke, Visuals};
use windows_sys::{
    Win32::{
        Graphics::Gdi::{
            COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_WINDOW, COLOR_WINDOWTEXT, GetSysColor,
        },
        UI::{
            Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
            WindowsAndMessaging::{
                MB_ICONERROR, MB_OK, MessageBoxW, SPI_GETHIGHCONTRAST, SystemParametersInfoW,
            },
        },
    },
    core::w,
};

#[derive(Clone, Copy)]
struct SystemColors {
    window: Color32,
    text: Color32,
    highlight: Color32,
    highlight_text: Color32,
}

/// Return the user's contrast palette, or leave the ordinary app theme intact.
#[allow(
    unsafe_code,
    reason = "Read-only Windows system settings boundary; all buffers are local and correctly sized."
)]
pub(super) fn high_contrast_visuals() -> Option<Visuals> {
    let mut contrast = HIGHCONTRASTW {
        cbSize: u32::try_from(size_of::<HIGHCONTRASTW>()).ok()?,
        ..HIGHCONTRASTW::default()
    };
    // SAFETY: Windows writes this SDK structure synchronously. Its size is set,
    // and no reference or pointer escapes the call.
    let available = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            contrast.cbSize,
            (&raw mut contrast).cast(),
            0,
        )
    };
    if available == 0 || contrast.dwFlags & HCF_HIGHCONTRASTON == 0 {
        return None;
    }
    // SAFETY: COLOR_WINDOW is a documented system-color index; the call owns no resources.
    let window = unsafe { GetSysColor(COLOR_WINDOW) };
    // SAFETY: COLOR_WINDOWTEXT is a documented system-color index.
    let text = unsafe { GetSysColor(COLOR_WINDOWTEXT) };
    // SAFETY: COLOR_HIGHLIGHT is a documented system-color index.
    let highlight = unsafe { GetSysColor(COLOR_HIGHLIGHT) };
    // SAFETY: COLOR_HIGHLIGHTTEXT is a documented system-color index.
    let highlight_text = unsafe { GetSysColor(COLOR_HIGHLIGHTTEXT) };
    Some(contrast_visuals(SystemColors {
        window: color_ref(window),
        text: color_ref(text),
        highlight: color_ref(highlight),
        highlight_text: color_ref(highlight_text),
    }))
}

const fn color_ref(color: u32) -> Color32 {
    let [red, green, blue, _reserved] = color.to_le_bytes();
    Color32::from_rgb(red, green, blue)
}

fn contrast_visuals(colors: SystemColors) -> Visuals {
    let mut visuals = Visuals {
        dark_mode: u16::from(colors.window.r())
            + u16::from(colors.window.g())
            + u16::from(colors.window.b())
            < 384,
        override_text_color: None,
        weak_text_color: Some(colors.text),
        faint_bg_color: colors.window,
        extreme_bg_color: colors.window,
        text_edit_bg_color: Some(colors.window),
        code_bg_color: colors.window,
        window_fill: colors.window,
        panel_fill: colors.window,
        window_stroke: Stroke::new(1.0, colors.text),
        window_shadow: Shadow::NONE,
        popup_shadow: Shadow::NONE,
        hyperlink_color: colors.text,
        warn_fg_color: colors.text,
        error_fg_color: colors.text,
        disabled_alpha: 1.0,
        button_frame: true,
        collapsing_header_frame: true,
        ..Visuals::dark()
    };
    // egui also uses active.fg_stroke for strong labels outside controls. Keep
    // every unselected foreground/background on the same system color pair;
    // thicker borders distinguish hover/focus without relying on color alone.
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.bg_fill = colors.window;
        widget.weak_bg_fill = colors.window;
        widget.bg_stroke = Stroke::new(1.0, colors.text);
        widget.fg_stroke = Stroke::new(1.0, colors.text);
    }
    visuals.widgets.hovered.bg_stroke.width = 2.0;
    visuals.widgets.active.bg_stroke.width = 2.0;
    visuals.widgets.open.bg_stroke.width = 2.0;
    visuals.selection.bg_fill = colors.highlight;
    visuals.selection.stroke = Stroke::new(1.0, colors.highlight_text);
    visuals.text_cursor.stroke = Stroke::new(2.0, colors.text);
    visuals.ime_composition.active_underline_stroke = Stroke::new(2.0, colors.text);
    visuals.ime_composition.inactive_underline_stroke = Stroke::new(1.0, colors.text);
    visuals
}

/// Keep fatal startup failures visible when an egui window cannot be created.
#[allow(
    unsafe_code,
    reason = "Synchronous native startup dialog; Windows retains no pointer to the owned UTF-16 message."
)]
pub(super) fn show_error(message: &str) {
    let message: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    // SAFETY: No parent is supplied; both strings stay valid for the synchronous
    // modal call and Windows owns the dialog internals.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            w!("Swift Bonsai 2"),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_palette_preserves_system_color_pairs_for_text_and_selection() {
        for (window, text) in [
            (Color32::BLACK, Color32::WHITE),
            (Color32::WHITE, Color32::BLACK),
        ] {
            let colors = SystemColors {
                window,
                text,
                highlight: Color32::from_rgb(34, 68, 102),
                highlight_text: Color32::from_rgb(255, 255, 0),
            };
            let visuals = contrast_visuals(colors);
            assert_eq!(visuals.panel_fill, window);
            assert_eq!(visuals.text_edit_bg_color, Some(window));
            assert_eq!(visuals.text_color(), text);
            assert_eq!(visuals.weak_text_color(), text);
            assert_eq!(visuals.strong_text_color(), text);
            assert_eq!(visuals.selection.bg_fill, colors.highlight);
            assert_eq!(visuals.selection.stroke.color, colors.highlight_text);
            assert_eq!(visuals.disabled_alpha.to_bits(), 1.0_f32.to_bits());
            assert_eq!(visuals.warn_fg_color, text);
            assert_eq!(visuals.error_fg_color, text);
            for widget in [
                visuals.widgets.noninteractive,
                visuals.widgets.inactive,
                visuals.widgets.hovered,
                visuals.widgets.active,
                visuals.widgets.open,
            ] {
                assert_eq!(widget.bg_fill, window);
                assert_eq!(widget.fg_stroke.color, text);
                assert!(widget.bg_stroke.width >= 1.0);
            }
        }
    }

    #[test]
    fn system_color_ref_channels_are_decoded_without_alpha_or_endian_swaps() {
        assert_eq!(color_ref(0x0012_3456), Color32::from_rgb(0x56, 0x34, 0x12));
    }
}

//! The floating timer window and the settings window.

use tauri::{
    AppHandle, LogicalSize, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

pub const TIMER_WINDOW: &str = "timer";
pub const SETTINGS_WINDOW: &str = "settings";

pub fn timer_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(TIMER_WINDOW)
}

pub fn is_timer_window_visible(app: &AppHandle) -> bool {
    timer_window(app)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
}

/// Shows or hides the floating window. When shown, the window is moved to
/// the primary display if its saved position is on a disconnected display.
pub fn set_timer_window_visible(app: &AppHandle, visible: bool) -> tauri::Result<()> {
    let Some(window) = timer_window(app) else {
        return Ok(());
    };
    if visible {
        ensure_on_screen(&window)?;
        window.show()?;
    } else {
        window.hide()?;
    }
    Ok(())
}

/// Opens the settings window, or focuses it if already open.
pub fn open_settings_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(SETTINGS_WINDOW) {
        window.unminimize()?;
        window.show()?;
        return window.set_focus();
    }
    // The Angular app picks the view from the window label.
    let window =
        WebviewWindowBuilder::new(app, SETTINGS_WINDOW, WebviewUrl::App("index.html".into()))
            .title("Työaikalaskuri – Asetukset")
            .inner_size(480.0, 640.0)
            .min_inner_size(400.0, 480.0)
            .center()
            .build()?;
    window.set_focus()
}

/// Logical size of the floating window while it shows the day summary.
const SUMMARY_WIDTH: f64 = 360.0;
/// Header, "timer running" note, total row and padding around the list.
const SUMMARY_CHROME_HEIGHT: f64 = 168.0;
const SUMMARY_ROW_HEIGHT: f64 = 34.0;
const SUMMARY_MIN_HEIGHT: f64 = 240.0;
const SUMMARY_MAX_HEIGHT: f64 = 560.0;

/// How to undo [`expand_timer_window`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactGeometry {
    /// The user's own size of the window.
    size: PhysicalSize<u32>,
    /// How far the window was moved to keep the expanded window on screen.
    shift: (i32, i32),
}

/// Logical height of the summary window for `rows` tasks. Longer lists scroll.
fn summary_height(rows: usize) -> f64 {
    let rows = u32::try_from(rows.max(1)).unwrap_or(u32::MAX);
    (SUMMARY_CHROME_HEIGHT + f64::from(rows) * SUMMARY_ROW_HEIGHT)
        .clamp(SUMMARY_MIN_HEIGHT, SUMMARY_MAX_HEIGHT)
}

/// Shows the floating window at the summary size for `rows` tasks, moving it
/// if it would extend past the screen edge. Pass the geometry returned by an
/// earlier call to re-fit an already expanded window.
pub fn expand_timer_window(
    app: &AppHandle,
    rows: usize,
    expanded: Option<CompactGeometry>,
) -> tauri::Result<Option<CompactGeometry>> {
    let Some(window) = timer_window(app) else {
        return Ok(expanded);
    };
    let mut geometry = match expanded {
        Some(geometry) => geometry,
        None => CompactGeometry {
            size: window.inner_size()?,
            shift: (0, 0),
        },
    };
    let size = LogicalSize::new(SUMMARY_WIDTH, summary_height(rows))
        .to_physical::<u32>(window.scale_factor()?);
    if window.inner_size()? != size {
        window.set_size(size)?;
    }
    ensure_on_screen(&window)?;
    let position = window.outer_position()?;
    if let Some(monitor) = window.current_monitor()? {
        let area = monitor.work_area();
        let (dx, dy) = fit_shift(position, size, area.position, area.size);
        if (dx, dy) != (0, 0) {
            window.set_position(PhysicalPosition::new(position.x + dx, position.y + dy))?;
            geometry.shift = (geometry.shift.0 + dx, geometry.shift.1 + dy);
        }
    }
    window.show()?;
    Ok(Some(geometry))
}

/// Returns the floating window to the size it had before the summary.
pub fn restore_timer_window(app: &AppHandle, geometry: CompactGeometry) -> tauri::Result<()> {
    let Some(window) = timer_window(app) else {
        return Ok(());
    };
    window.set_size(geometry.size)?;
    if geometry.shift != (0, 0) {
        let position = window.outer_position()?;
        window.set_position(PhysicalPosition::new(
            position.x - geometry.shift.0,
            position.y - geometry.shift.1,
        ))?;
    }
    Ok(())
}

/// How far to move a window left/up so that its right and bottom edges stay
/// within `area`, without pushing its top-left corner out of it.
fn fit_shift(
    pos: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    area_pos: PhysicalPosition<i32>,
    area_size: PhysicalSize<u32>,
) -> (i32, i32) {
    let axis = |pos: i32, len: u32, start: i32, area_len: u32| {
        let overflow = i64::from(pos) + i64::from(len) - (i64::from(start) + i64::from(area_len));
        let room = i64::from(pos) - i64::from(start);
        let shift = overflow.min(room).max(0);
        -i32::try_from(shift).unwrap_or(0)
    };
    (
        axis(pos.x, size.width, area_pos.x, area_size.width),
        axis(pos.y, size.height, area_pos.y, area_size.height),
    )
}

/// Centers the window if it does not overlap any connected monitor.
fn ensure_on_screen(window: &WebviewWindow) -> tauri::Result<()> {
    let position = window.outer_position()?;
    let size = window.outer_size()?;
    let visible = window
        .available_monitors()?
        .iter()
        .any(|m| overlaps(position, size, *m.position(), *m.size()));
    if !visible {
        log::info!("timer window is off-screen; moving it to the primary display");
        window.center()?;
    }
    Ok(())
}

fn overlaps(
    a_pos: PhysicalPosition<i32>,
    a_size: PhysicalSize<u32>,
    b_pos: PhysicalPosition<i32>,
    b_size: PhysicalSize<u32>,
) -> bool {
    let right =
        |p: PhysicalPosition<i32>, s: PhysicalSize<u32>| i64::from(p.x) + i64::from(s.width);
    let bottom =
        |p: PhysicalPosition<i32>, s: PhysicalSize<u32>| i64::from(p.y) + i64::from(s.height);
    i64::from(a_pos.x) < right(b_pos, b_size)
        && i64::from(b_pos.x) < right(a_pos, a_size)
        && i64::from(a_pos.y) < bottom(b_pos, b_size)
        && i64::from(b_pos.y) < bottom(a_pos, a_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_overlap_with_monitor() {
        let monitor = (PhysicalPosition::new(0, 0), PhysicalSize::new(1920, 1080));
        let window = PhysicalSize::new(260, 120);
        assert!(overlaps(
            PhysicalPosition::new(100, 100),
            window,
            monitor.0,
            monitor.1
        ));
        assert!(!overlaps(
            PhysicalPosition::new(2500, 100),
            window,
            monitor.0,
            monitor.1
        ));
        assert!(overlaps(
            PhysicalPosition::new(-200, 100),
            window,
            monitor.0,
            monitor.1
        ));
    }

    #[test]
    fn summary_height_grows_with_rows_within_limits() {
        assert_eq!(summary_height(0), SUMMARY_MIN_HEIGHT);
        assert!(summary_height(5) > summary_height(4));
        assert_eq!(summary_height(100), SUMMARY_MAX_HEIGHT);
    }

    #[test]
    fn expanded_window_is_moved_back_on_screen() {
        let area = (PhysicalPosition::new(0, 25), PhysicalSize::new(1920, 1055));
        let size = PhysicalSize::new(680, 800);
        // Fits: no shift.
        assert_eq!(
            fit_shift(PhysicalPosition::new(100, 100), size, area.0, area.1),
            (0, 0)
        );
        // Near the bottom right corner: moved left and up.
        assert_eq!(
            fit_shift(PhysicalPosition::new(1500, 900), size, area.0, area.1),
            (-260, -620)
        );
        // Taller than the area: the top edge stays visible.
        assert_eq!(
            fit_shift(
                PhysicalPosition::new(0, 225),
                PhysicalSize::new(100, 2000),
                area.0,
                area.1
            ),
            (0, -200)
        );
    }
}

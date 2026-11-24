//! Layout sizes of the settings boards, each expressed in spacing tokens (no free numbers).

use ultar_ui::tokens::Space;

/// Width of the sidebar (board: 288 = 6 × 48).
pub fn sidebar() -> f32 {
    6.0 * Space::S48.points()
}

/// Width of the arrangement column of the Displays page (board: 640 = 20 × 32).
pub fn display_main_column() -> f32 {
    20.0 * Space::S32.points()
}

/// Width of the status column of the account page (board: 320 = 10 × 32).
pub fn account_status_column() -> f32 {
    10.0 * Space::S32.points()
}

/// Width of the devices column of the account page (board: 336 = 7 × 48).
pub fn account_devices_column() -> f32 {
    7.0 * Space::S48.points()
}

/// Height of the arrangement canvas (board: 144 = 3 × 48).
pub fn arrangement_height() -> f32 {
    3.0 * Space::S48.points()
}

/// Width of the brightness control (board: 240 = 5 × 48).
pub fn brightness_width() -> f32 {
    5.0 * Space::S48.points()
}

/// Shares of the driver table columns (board: flex 3 : 2 : 2 : 3).
pub const DRIVER_COLUMNS: [f32; 4] = [3.0, 2.0, 2.0, 3.0];

/// Widths of the driver table columns for a table of `inner` points with `gap` between.
pub fn driver_columns(inner: f32, gap: f32) -> [f32; 4] {
    let total: f32 = DRIVER_COLUMNS.iter().sum();
    let free = (inner - gap * (DRIVER_COLUMNS.len() as f32 - 1.0)).max(0.0);
    DRIVER_COLUMNS.map(|share| free * share / total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_sizes() {
        assert_eq!(sidebar(), 288.0);
        assert_eq!(display_main_column(), 640.0);
        assert_eq!(account_status_column(), 320.0);
        assert_eq!(account_devices_column(), 336.0);
        assert_eq!(arrangement_height(), 144.0);
        let c = driver_columns(1054.0, 16.0);
        assert!((c.iter().sum::<f32>() + 48.0 - 1054.0).abs() < 0.01);
        assert!((c[0] / c[1] - 1.5).abs() < 1e-4);
        assert_eq!(driver_columns(10.0, 16.0), [0.0; 4]);
    }
}

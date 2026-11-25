//! The desktop's surfaces on the shared compositor host (`ultar-shell-host`): every surface
//! shows a desktop [`View`].

use crate::view::View;
use ultar_ui::ui::Ui;

pub use ultar_shell_host::{HostEvent, Tree, MAX_SURFACES, SWIPE_MIN};

/// A compositor client whose surfaces show desktop views.
pub type Host = ultar_shell_host::Host<View>;

/// One surface showing a desktop view.
pub type HostSurface = ultar_shell_host::HostSurface<View>;

impl Tree for View {
    fn ui(&self) -> &Ui {
        &self.ui
    }
    fn ui_mut(&mut self) -> &mut Ui {
        &mut self.ui
    }
}

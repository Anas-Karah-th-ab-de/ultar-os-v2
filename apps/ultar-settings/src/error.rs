//! Errors of the app.

use ultar_ui::ui::UiError;

/// What can stop the app.
#[derive(Debug)]
pub enum AppError {
    /// The widget tree refused an operation (a bug or a limit).
    Ui(UiError),
    /// The compositor connection failed.
    Surface(String),
    /// The command line is wrong.
    Usage,
}

impl AppError {
    /// i18n key of the error.
    pub fn key(&self) -> &'static str {
        match self {
            AppError::Ui(_) => "app.settings.error.ui",
            AppError::Surface(_) => "app.settings.error.surface",
            AppError::Usage => "app.settings.error.usage",
        }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::Ui(e) => write!(f, "{}: {e:?}", self.key()),
            AppError::Surface(e) => write!(f, "{}: {e}", self.key()),
            AppError::Usage => f.write_str(self.key()),
        }
    }
}

impl std::error::Error for AppError {}

impl From<UiError> for AppError {
    fn from(e: UiError) -> AppError {
        AppError::Ui(e)
    }
}

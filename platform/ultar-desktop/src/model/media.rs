//! Removable media: what the shell does when a drive with media is plugged in
//! (`apps.removable_media`). The shell follows the setting and reacts to a new drive in
//! `volumes.state` (`shell::media`).

/// What happens when a drive with media is plugged in (`apps.removable_media`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RemovableMedia {
    /// Ask the person (a notice with an "Open" button).
    #[default]
    Ask,
    /// Open the drive in Files at once.
    OpenFiles,
    /// Do nothing.
    Nothing,
}

impl RemovableMedia {
    /// From the setting's value; an unknown word keeps the default (ask).
    pub fn parse(word: &str) -> RemovableMedia {
        match word {
            "open_files" => RemovableMedia::OpenFiles,
            "nothing" => RemovableMedia::Nothing,
            _ => RemovableMedia::Ask,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_parse_and_an_unknown_one_asks() {
        assert_eq!(RemovableMedia::parse("ask"), RemovableMedia::Ask);
        assert_eq!(
            RemovableMedia::parse("open_files"),
            RemovableMedia::OpenFiles
        );
        assert_eq!(RemovableMedia::parse("nothing"), RemovableMedia::Nothing);
        assert_eq!(RemovableMedia::parse("nonsense"), RemovableMedia::Ask);
        assert_eq!(RemovableMedia::default(), RemovableMedia::Ask);
    }
}

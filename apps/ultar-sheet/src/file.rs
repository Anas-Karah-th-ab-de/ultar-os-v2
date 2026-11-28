//! Opening and saving workbook files: `.xlsx` and `.csv` (UTF-8 and Windows-1256 among
//! others). A save goes through a temporary file and a rename, so a crash never leaves half a
//! file behind.

use std::io::Write;
use std::path::{Path, PathBuf};
use ultar_office::code::{self, Enc, Eol};
use ultar_office::sheet::{self, Workbook};
use ultar_office::xlsx;

/// Largest file opened (a bigger one stays closed with a message).
pub const MAX_BYTES: u64 = 64 * 1024 * 1024;

/// Why a file could not be opened or saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocError {
    /// The system refused (permission, read-only place).
    Denied,
    /// The file or its folder does not exist.
    Missing,
    /// A file of that name exists.
    Exists,
    /// A name that cannot be used.
    BadName,
    /// Larger than [`MAX_BYTES`] or with more cells than the app holds.
    TooBig,
    /// Not a workbook (a broken zip, broken XML).
    Damaged,
    /// Any other input/output error.
    Io,
}

impl DocError {
    /// The message key of the reason.
    pub fn key(self) -> &'static str {
        match self {
            DocError::Denied => "sheet.reason.denied",
            DocError::Missing => "sheet.reason.missing",
            DocError::Exists => "sheet.reason.exists",
            DocError::BadName => "sheet.reason.name",
            DocError::TooBig => "sheet.reason.too_big",
            DocError::Damaged => "sheet.reason.damaged",
            DocError::Io => "sheet.reason.io",
        }
    }

    fn of(e: &std::io::Error) -> DocError {
        match e.kind() {
            std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem => {
                DocError::Denied
            }
            std::io::ErrorKind::NotFound => DocError::Missing,
            std::io::ErrorKind::AlreadyExists => DocError::Exists,
            _ => DocError::Io,
        }
    }
}

/// How a workbook is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// An Excel workbook.
    Xlsx,
    /// Delimited text of the shown sheet.
    Csv {
        /// Text encoding.
        enc: Enc,
        /// Field separator.
        sep: char,
        /// Line ends.
        eol: Eol,
    },
}

impl Format {
    /// The format a file name asks for.
    pub fn of_name(path: &Path) -> Format {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "csv" | "txt" => Format::csv(','),
            "tsv" => Format::csv('\t'),
            _ => Format::Xlsx,
        }
    }

    /// Text with Excel's habits: UTF-8 with a byte-order mark (Excel shows Arabic right only
    /// with it), Windows line ends.
    pub fn csv(sep: char) -> Format {
        Format::Csv {
            enc: Enc::Utf8Bom,
            sep,
            eol: Eol::CrLf,
        }
    }

    /// `true` for `.csv`-like text.
    pub fn is_text(self) -> bool {
        matches!(self, Format::Csv { .. })
    }
}

/// A workbook read from a file.
#[derive(Debug)]
pub struct Opened {
    /// The cells.
    pub book: Workbook,
    /// How it was stored (kept for saving).
    pub format: Format,
}

/// The separator of delimited text: the one that is most frequent in the first line.
fn separator(text: &str) -> char {
    let first = text
        .trim_start_matches('\u{feff}')
        .lines()
        .next()
        .unwrap_or("");
    let best = [';', '\t', ',']
        .into_iter()
        .max_by_key(|c| first.matches(*c).count())
        .unwrap_or(',');
    if first.matches(best).count() == 0 {
        ','
    } else {
        best
    }
}

/// Reads a workbook from bytes; `name` decides between `.xlsx` and delimited text.
pub fn read_bytes(name: &Path, bytes: &[u8]) -> Result<Opened, DocError> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err(DocError::TooBig);
    }
    if !Format::of_name(name).is_text() {
        return match xlsx::read(bytes) {
            Ok(book) => Ok(Opened {
                book,
                format: Format::Xlsx,
            }),
            Err(e) if e == xlsx::TOO_LARGE => Err(DocError::TooBig),
            Err(_) => Err(DocError::Damaged),
        };
    }
    let (text, enc, eol) = code::decode(bytes);
    let mut sh = sheet::read_csv(&text);
    if sh.cells.len() > xlsx::MAX_CELLS {
        return Err(DocError::TooBig);
    }
    sh.name = name
        .file_stem()
        .map(|s| s.to_string_lossy().chars().take(31).collect::<String>())
        .filter(|s| ultar_office::sheet_edit::valid_sheet_name(s))
        .unwrap_or_else(|| "Sheet1".to_owned());
    Ok(Opened {
        book: Workbook { sheets: vec![sh] },
        format: Format::Csv {
            enc,
            sep: separator(&text),
            eol,
        },
    })
}

/// Opens `path`.
pub fn open(path: &Path) -> Result<Opened, DocError> {
    let size = std::fs::metadata(path).map_err(|e| DocError::of(&e))?.len();
    if size > MAX_BYTES {
        return Err(DocError::TooBig);
    }
    let bytes = std::fs::read(path).map_err(|e| DocError::of(&e))?;
    read_bytes(path, &bytes)
}

/// What a save did besides saving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Saved {
    /// The text encoding had to change because the old one cannot hold every character
    /// (the new one).
    pub encoding_changed: Option<Enc>,
    /// Delimited text keeps only the shown sheet's values: formulas, formats and the other
    /// sheets are not in the file.
    pub values_only: bool,
}

/// The bytes of `wb` in `format`; `active` is the sheet delimited text keeps.
pub fn write_bytes(
    wb: &Workbook,
    format: Format,
    active: usize,
) -> Result<(Vec<u8>, Saved), DocError> {
    match format {
        Format::Xlsx => {
            let bytes = xlsx::write(wb).map_err(|_| DocError::Io)?;
            Ok((
                bytes,
                Saved {
                    encoding_changed: None,
                    values_only: false,
                },
            ))
        }
        Format::Csv { enc, sep, eol } => {
            let text = sheet::write_csv(wb, active, sep);
            let mut enc_used = enc;
            let mut bytes = code::encode(&text, enc, eol);
            // a character the old encoding cannot hold must not turn into a replacement
            let faithful = code::decode(&bytes).0.replace("\r\n", "\n") == text;
            let mut changed = None;
            if !faithful {
                enc_used = Enc::Utf8Bom;
                bytes = code::encode(&text, enc_used, eol);
                changed = Some(enc_used);
            }
            let _ = enc_used;
            let lossy = wb.sheets.len() > 1
                || wb
                    .sheets
                    .iter()
                    .any(|s| !s.styles.is_empty() || s.cells.values().any(|c| c.starts_with('=')));
            Ok((
                bytes,
                Saved {
                    encoding_changed: changed,
                    values_only: lossy,
                },
            ))
        }
    }
}

/// Writes `bytes` to `path` through a temporary file in the same folder and a rename.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), DocError> {
    let dir = path.parent().ok_or(DocError::BadName)?;
    let name = path.file_name().ok_or(DocError::BadName)?.to_string_lossy();
    let tmp: PathBuf = dir.join(format!(".{name}.tmp{}", std::process::id()));
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(DocError::of(&e));
    }
    Ok(())
}

/// Saves `wb` to `path`.
pub fn save(path: &Path, wb: &Workbook, format: Format, active: usize) -> Result<Saved, DocError> {
    let (bytes, saved) = write_bytes(wb, format, active)?;
    write_atomic(path, &bytes)?;
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("ultar-sheet-file-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn book() -> Workbook {
        let mut wb = Workbook::new();
        wb.sheets[0].set(0, 0, "اسم");
        wb.sheets[0].set(0, 1, "قيمة");
        wb.sheets[0].set(1, 0, "تفاحة");
        wb.sheets[0].set(1, 1, "12.5");
        wb
    }

    #[test]
    fn xlsx_round_trip_through_a_file() {
        let d = tmp("xlsx");
        let p = d.join("a.xlsx");
        let mut wb = book();
        wb.sheets[0].set(2, 1, "=B2*2");
        save(&p, &wb, Format::Xlsx, 0).unwrap();
        let back = open(&p).unwrap();
        assert_eq!(back.format, Format::Xlsx);
        assert_eq!(back.book.sheets[0].cells, wb.sheets[0].cells);
        // no temporary file stays behind
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
    }

    #[test]
    fn csv_in_utf8_and_windows_1256() {
        let d = tmp("csv");
        let wb = book();
        let p = d.join("a.csv");
        let s = save(&p, &wb, Format::csv(','), 0).unwrap();
        assert!(!s.values_only && s.encoding_changed.is_none());
        let raw = std::fs::read(&p).unwrap();
        assert!(raw.starts_with(&[0xEF, 0xBB, 0xBF]));
        let back = open(&p).unwrap();
        assert_eq!(back.book.sheets[0].get(1, 0), "تفاحة");
        // a Windows-1256 file is read as such and written back as such
        let ar = Format::Csv {
            enc: Enc::Win1256,
            sep: ';',
            eol: Eol::CrLf,
        };
        let q = d.join("b.csv");
        save(&q, &wb, ar, 0).unwrap();
        let raw = std::fs::read(&q).unwrap();
        assert!(!raw.starts_with(&[0xEF]) && raw.iter().any(|b| *b >= 0xC1));
        let back = open(&q).unwrap();
        assert_eq!(back.format, ar);
        assert_eq!(back.book.sheets[0].get(0, 1), "قيمة");
        assert_eq!(back.book.sheets[0].get(1, 1), "12.5");
    }

    #[test]
    fn a_character_windows_1256_cannot_hold_changes_the_encoding() {
        let mut wb = book();
        wb.sheets[0].set(3, 0, "✓");
        let ar = Format::Csv {
            enc: Enc::Win1256,
            sep: ',',
            eol: Eol::Lf,
        };
        let (bytes, saved) = write_bytes(&wb, ar, 0).unwrap();
        assert_eq!(saved.encoding_changed, Some(Enc::Utf8Bom));
        assert!(String::from_utf8_lossy(&bytes).contains('✓'));
    }

    #[test]
    fn csv_keeps_only_values_and_says_so() {
        let mut wb = book();
        wb.sheets[0].set(2, 1, "=B2*2");
        let (bytes, saved) = write_bytes(&wb, Format::csv(','), 0).unwrap();
        assert!(saved.values_only);
        assert!(String::from_utf8_lossy(&bytes).contains("25"));
    }

    #[test]
    fn errors_have_reasons() {
        let d = tmp("err");
        assert_eq!(
            open(&d.join("missing.xlsx")).unwrap_err(),
            DocError::Missing
        );
        std::fs::write(d.join("bad.xlsx"), b"this is not a zip").unwrap();
        assert_eq!(open(&d.join("bad.xlsx")).unwrap_err(), DocError::Damaged);
        std::fs::write(d.join("empty.xlsx"), b"").unwrap();
        assert_eq!(open(&d.join("empty.xlsx")).unwrap_err(), DocError::Damaged);
        assert_eq!(
            save(&d.join("nodir/a.xlsx"), &book(), Format::Xlsx, 0).unwrap_err(),
            DocError::Missing
        );
        assert_eq!(
            read_bytes(Path::new("a.xlsx"), &[0u8; 8]).unwrap_err(),
            DocError::Damaged
        );
    }

    #[test]
    fn the_separator_is_found() {
        assert_eq!(separator("a;b;c\n1;2;3"), ';');
        assert_eq!(separator("a\tb\n"), '\t');
        assert_eq!(separator("abc"), ',');
        let r = read_bytes(Path::new("x.csv"), "a;b\n1;2\n".as_bytes()).unwrap();
        assert!(matches!(r.format, Format::Csv { sep: ';', .. }));
        assert_eq!(r.book.sheets[0].name, "x");
    }
}

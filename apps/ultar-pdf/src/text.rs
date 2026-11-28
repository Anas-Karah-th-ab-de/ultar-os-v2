//! The text layer of a page: its words in reading order with their boxes (PDF points, y up),
//! the word nearest to a point, and the text of a run of words (for select and copy).

use ultar_office::pdf::{Line, Word};

/// The words of one page in reading order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageText {
    pub words: Vec<Word>,
    /// The line each word belongs to.
    lines: Vec<usize>,
}

impl PageText {
    /// The text layer of `lines` (as the office library delivers them).
    pub fn new(lines: &[Line]) -> PageText {
        let mut words = Vec::new();
        let mut line_of = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            for w in &line.words {
                words.push(w.clone());
                line_of.push(i);
            }
        }
        PageText {
            words,
            lines: line_of,
        }
    }

    /// A text layer from words that each form their own line (tests).
    pub fn from_words(words: Vec<Word>) -> PageText {
        let lines = (0..words.len()).collect();
        PageText { words, lines }
    }

    /// The page has no text (an empty or scanned page).
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// The word nearest to the page point `p` (0 away when inside its box); `None` without words.
    pub fn nearest_word(&self, p: (f32, f32)) -> Option<usize> {
        let dist = |w: &Word| {
            let dx = (w.x0 - p.0).max(p.0 - w.x1).max(0.0);
            let dy = (w.y0 - p.1).max(p.1 - w.y1).max(0.0);
            // A line is taller than it is wide to the eye: weigh the vertical miss more.
            dx * dx + 4.0 * dy * dy
        };
        self.words
            .iter()
            .enumerate()
            .min_by(|a, b| dist(a.1).total_cmp(&dist(b.1)))
            .map(|(i, _)| i)
    }

    /// The text of the words `first..=last` (a run in reading order): spaces inside a line,
    /// a line break between lines.
    pub fn text(&self, first: usize, last: usize) -> String {
        let mut out = String::new();
        let (first, last) = (first.min(last), first.max(last));
        for i in first..=last.min(self.words.len().saturating_sub(1)) {
            let Some(w) = self.words.get(i) else {
                break;
            };
            if i > first {
                out.push(if self.lines[i] == self.lines[i - 1] { ' ' } else { '\n' });
            }
            out.push_str(&w.text);
        }
        out
    }

    /// The whole page text.
    pub fn all(&self) -> String {
        if self.words.is_empty() {
            return String::new();
        }
        self.text(0, self.words.len() - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, x: f32, y: f32) -> Word {
        Word {
            text: text.into(),
            x0: x,
            y0: y,
            x1: x + 40.0,
            y1: y + 12.0,
        }
    }

    fn line(words: Vec<Word>) -> Line {
        let text = words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
        Line {
            text,
            x0: words[0].x0,
            y0: words[0].y0,
            x1: words[words.len() - 1].x1,
            y1: words[0].y1,
            words,
        }
    }

    fn page() -> PageText {
        PageText::new(&[
            line(vec![word("مرحبا", 150.0, 700.0), word("بكم", 100.0, 700.0)]),
            line(vec![word("hello", 50.0, 680.0), word("world", 100.0, 680.0)]),
        ])
    }

    #[test]
    fn text_joins_words_with_spaces_and_lines_with_breaks() {
        let p = page();
        assert_eq!(p.words.len(), 4);
        assert_eq!(p.text(0, 1), "مرحبا بكم");
        assert_eq!(p.text(1, 2), "بكم\nhello");
        assert_eq!(p.text(3, 0), "مرحبا بكم\nhello world");
        assert_eq!(p.all(), "مرحبا بكم\nhello world");
        assert_eq!(p.text(2, 99), "hello world");
        assert_eq!(PageText::default().all(), "");
    }

    #[test]
    fn the_nearest_word_is_the_one_under_the_point_or_next_to_it() {
        let p = page();
        assert_eq!(p.nearest_word((160.0, 705.0)), Some(0));
        assert_eq!(p.nearest_word((110.0, 705.0)), Some(1));
        // Below the second line: the word above is nearest.
        assert_eq!(p.nearest_word((60.0, 660.0)), Some(2));
        // Far right of the first line: its end word.
        assert_eq!(p.nearest_word((500.0, 702.0)), Some(0));
        assert_eq!(PageText::default().nearest_word((0.0, 0.0)), None);
    }
}

//! Search in the document: which words match (Arabic normalisation as APP-RULES §10 says),
//! the list of hits as they arrive page by page, and the "n of m" position.

use crate::text::PageText;
use ultar_i18n::arabic::normalize_for_search;

/// A box on a page: x0, y0, x1, y1 in PDF points (y up).
pub type Rect = (f32, f32, f32, f32);

/// One hit: a run of words on a page (inclusive indices into the page's words).
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub page: usize,
    pub first: usize,
    pub last: usize,
    /// The box of each word of the run.
    pub rects: Vec<Rect>,
}

/// The normalised words of a query (empty for a blank query).
pub fn tokens(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(normalize_for_search)
        .filter(|t| !t.is_empty())
        .collect()
}

/// Every hit of `tokens` on one page. One token matches each word that contains it; several
/// tokens match runs of consecutive words, each containing its token.
pub fn find_in_text(page: usize, text: &PageText, tokens: &[String]) -> Vec<Hit> {
    if tokens.is_empty() || text.words.len() < tokens.len() {
        return Vec::new();
    }
    let folded: Vec<String> = text.words.iter().map(|w| normalize_for_search(&w.text)).collect();
    let mut out = Vec::new();
    for first in 0..=folded.len() - tokens.len() {
        let all = tokens
            .iter()
            .enumerate()
            .all(|(k, t)| folded[first + k].contains(t.as_str()));
        if all {
            let last = first + tokens.len() - 1;
            let rects = text.words[first..=last]
                .iter()
                .map(|w| (w.x0, w.y0, w.x1, w.y1))
                .collect();
            out.push(Hit {
                page,
                first,
                last,
                rects,
            });
        }
    }
    out
}

/// The search of the open document: the hits found so far and the one shown.
#[derive(Debug, Clone, Default)]
pub struct Search {
    /// The query as typed.
    pub query: String,
    /// The number of the search the worker answers for (older answers are dropped).
    pub id: u32,
    hits: Vec<Hit>,
    /// The shown hit: its page and first word.
    current: Option<(usize, usize)>,
    scanned: usize,
    total: usize,
}

impl Search {
    /// Starts a new search over `total` pages (the old hits are dropped).
    pub fn begin(&mut self, query: &str, id: u32, total: usize) {
        self.query = query.to_owned();
        self.id = id;
        self.hits.clear();
        self.current = None;
        self.scanned = 0;
        self.total = if tokens(query).is_empty() { 0 } else { total };
    }

    /// Forgets everything (the search bar closed).
    pub fn clear(&mut self) {
        *self = Search {
            id: self.id,
            ..Search::default()
        };
    }

    /// Takes the hits of one scanned page; the first hit that arrives (the nearest to where
    /// the reader was) becomes the shown one. Returns `true` when the shown hit is new.
    pub fn add_page(&mut self, id: u32, hits: Vec<Hit>) -> bool {
        if id != self.id {
            return false;
        }
        self.scanned += 1;
        let first_hit = self.hits.is_empty() && !hits.is_empty();
        if !hits.is_empty() {
            self.hits.extend(hits);
            self.hits.sort_by_key(|h| (h.page, h.first));
        }
        if first_hit {
            self.current = self.hits.iter().next().map(|h| (h.page, h.first));
        }
        first_hit
    }

    /// All hits, in page order.
    pub fn hits(&self) -> &[Hit] {
        &self.hits
    }

    /// The number of hits.
    pub fn count(&self) -> usize {
        self.hits.len()
    }

    /// Every page has been scanned.
    pub fn done(&self) -> bool {
        self.scanned >= self.total
    }

    /// The search has something to look for.
    pub fn active(&self) -> bool {
        self.total > 0
    }

    /// Pages scanned so far and in total.
    pub fn progress(&self) -> (usize, usize) {
        (self.scanned, self.total)
    }

    /// The index of the shown hit.
    pub fn position(&self) -> Option<usize> {
        let cur = self.current?;
        self.hits.iter().position(|h| (h.page, h.first) == cur)
    }

    /// The shown hit.
    pub fn current(&self) -> Option<&Hit> {
        self.position().and_then(|i| self.hits.get(i))
    }

    /// Moves to the next hit (wrapping around); `None` without hits.
    pub fn next(&mut self) -> Option<&Hit> {
        self.step(1)
    }

    /// Moves to the previous hit (wrapping around).
    pub fn prev(&mut self) -> Option<&Hit> {
        self.step(-1)
    }

    fn step(&mut self, dir: isize) -> Option<&Hit> {
        if self.hits.is_empty() {
            return None;
        }
        let n = self.hits.len() as isize;
        let at = self.position().map_or(if dir > 0 { -1 } else { 0 }, |p| p as isize);
        let to = (at + dir).rem_euclid(n) as usize;
        self.current = self.hits.get(to).map(|h| (h.page, h.first));
        self.hits.get(to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_office::pdf::Word;

    fn word(text: &str, x: f32) -> Word {
        Word {
            text: text.into(),
            x0: x,
            y0: 700.0,
            x1: x + 30.0,
            y1: 712.0,
        }
    }

    fn text(words: &[&str]) -> PageText {
        PageText::from_words(
            words
                .iter()
                .enumerate()
                .map(|(i, w)| word(w, 50.0 + 40.0 * i as f32))
                .collect(),
        )
    }

    #[test]
    fn tokens_are_normalised_and_blank_queries_are_empty() {
        assert_eq!(tokens("  HELLO   World "), vec!["hello", "world"]);
        assert!(tokens("   ").is_empty());
        assert!(tokens("").is_empty());
        // tashkeel and tatweel vanish, alef forms and digits fold
        assert_eq!(tokens("مَرْحَبًا"), vec!["مرحبا"]);
        assert_eq!(tokens("أحمد ٣"), vec!["احمد", "3"]);
    }

    #[test]
    fn arabic_search_ignores_tashkeel_alef_forms_and_presentation_forms() {
        let t = text(&["\u{FEE3}\u{FEAE}\u{FEA3}\u{FE92}\u{FE8E}", "أحمد", "Hello", "مدرسة"]);
        assert_eq!(find_in_text(0, &t, &tokens("مرحبا")).len(), 1);
        assert_eq!(find_in_text(0, &t, &tokens("مَرحَبا")).len(), 1);
        assert_eq!(find_in_text(0, &t, &tokens("احمد")).len(), 1);
        assert_eq!(find_in_text(0, &t, &tokens("مدرسه")).len(), 1);
        assert_eq!(find_in_text(0, &t, &tokens("HELLO")).len(), 1);
        assert_eq!(find_in_text(0, &t, &tokens("ell")).len(), 1);
        assert!(find_in_text(0, &t, &tokens("nothing")).is_empty());
        assert!(find_in_text(0, &t, &tokens("  ")).is_empty());
    }

    #[test]
    fn adjacent_matches_are_separate_hits_and_phrases_are_runs() {
        let t = text(&["ha", "ha", "hello", "big", "world"]);
        let hits = find_in_text(3, &t, &tokens("ha"));
        assert_eq!(hits.len(), 2);
        assert_eq!((hits[0].first, hits[1].first, hits[0].page), (0, 1, 3));
        let run = find_in_text(0, &t, &tokens("hello big"));
        assert_eq!(run.len(), 1);
        assert_eq!((run[0].first, run[0].last, run[0].rects.len()), (2, 3, 2));
        assert!(find_in_text(0, &t, &tokens("hello world")).is_empty());
    }

    fn hit(page: usize, first: usize) -> Hit {
        Hit {
            page,
            first,
            last: first,
            rects: vec![(0.0, 0.0, 1.0, 1.0)],
        }
    }

    #[test]
    fn hits_arrive_per_page_and_the_nearest_one_is_shown_first() {
        let mut s = Search::default();
        s.begin("x", 7, 4);
        assert!(s.active() && !s.done());
        // The worker started on page 2: its hit arrives first, then pages 3, 0 and 1.
        assert!(s.add_page(7, vec![hit(2, 5), hit(2, 9)]));
        assert!(!s.add_page(7, vec![]));
        assert!(!s.add_page(7, vec![hit(0, 1)]));
        assert!(!s.add_page(9, vec![hit(1, 1)]), "a stale answer is dropped");
        assert_eq!(s.progress(), (3, 4));
        assert!(!s.add_page(7, vec![hit(1, 3)]));
        assert!(s.done());
        assert_eq!(s.count(), 4);
        assert_eq!(s.position(), Some(2), "page order, shown hit is the first that arrived");
        assert_eq!(s.current().map(|h| (h.page, h.first)), Some((2, 5)));
    }

    #[test]
    fn next_and_previous_wrap_around() {
        let mut s = Search::default();
        s.begin("x", 1, 2);
        s.add_page(1, vec![hit(0, 0), hit(0, 4)]);
        s.add_page(1, vec![hit(1, 2)]);
        assert_eq!(s.position(), Some(0));
        assert_eq!(s.next().map(|h| (h.page, h.first)), Some((0, 4)));
        assert_eq!(s.next().map(|h| (h.page, h.first)), Some((1, 2)));
        assert_eq!(s.next().map(|h| (h.page, h.first)), Some((0, 0)));
        assert_eq!(s.prev().map(|h| (h.page, h.first)), Some((1, 2)));
        s.clear();
        assert!(s.next().is_none() && s.count() == 0 && !s.active());
        s.begin("   ", 2, 10);
        assert!(!s.active(), "a blank query searches nothing");
    }
}

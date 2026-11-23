//! The two system slots.

/// One of the two system slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    /// First slot.
    A,
    /// Second slot.
    B,
}

impl Slot {
    /// Both slots.
    pub const BOTH: [Slot; 2] = [Slot::A, Slot::B];

    /// The slot that is not this one.
    pub fn other(self) -> Slot {
        match self {
            Slot::A => Slot::B,
            Slot::B => Slot::A,
        }
    }

    /// 0 for A, 1 for B.
    pub fn index(self) -> usize {
        match self {
            Slot::A => 0,
            Slot::B => 1,
        }
    }

    /// The slot with this index.
    pub fn from_index(index: u8) -> Option<Slot> {
        match index {
            0 => Some(Slot::A),
            1 => Some(Slot::B),
            _ => None,
        }
    }

    /// `a` or `b`.
    pub fn letter(self) -> &'static str {
        match self {
            Slot::A => "a",
            Slot::B => "b",
        }
    }

    /// The slot with this letter.
    pub fn from_letter(letter: &str) -> Option<Slot> {
        Slot::BOTH.into_iter().find(|s| s.letter() == letter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_helpers() {
        assert_eq!(Slot::A.other(), Slot::B);
        assert_eq!(Slot::B.other(), Slot::A);
        for slot in Slot::BOTH {
            assert_eq!(Slot::from_index(slot.index() as u8), Some(slot));
            assert_eq!(Slot::from_letter(slot.letter()), Some(slot));
        }
        assert_eq!(Slot::from_index(2), None);
        assert_eq!(Slot::from_letter("c"), None);
        assert_eq!(Slot::from_letter("A"), None);
    }
}

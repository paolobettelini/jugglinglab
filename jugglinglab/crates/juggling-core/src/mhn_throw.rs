use std::cmp::Ordering;
use std::fmt;

pub const RIGHT_HAND: usize = 0;
pub const LEFT_HAND: usize = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MhnThrowRef {
    pub juggler: usize,
    pub hand: usize,
    pub index: isize,
    pub slot: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MhnThrowLink {
    Matrix(MhnThrowRef),
    External(usize),
}

#[derive(Clone, Debug)]
pub struct MhnThrow {
    pub juggler: usize,
    pub hand: usize,
    pub index: isize,
    pub slot: usize,
    pub target_juggler: usize,
    pub target_hand: usize,
    pub target_index: isize,
    pub target_slot: isize,
    pub throw_mod: Option<String>,

    pub hands_beat: isize,
    pub primary: Option<MhnThrowRef>,
    pub source: Option<MhnThrowLink>,
    pub target: Option<MhnThrowLink>,
    pub path_num: isize,
    pub catching: bool,
    pub catch_num: isize,
    pub dwell_window: usize,
    pub throw_time: f64,
    pub catch_time: f64,
}

impl MhnThrow {
    pub fn new(
        juggler: usize,
        hand: usize,
        index: isize,
        slot: usize,
        target_juggler: usize,
        target_hand: usize,
        target_index: isize,
        target_slot: isize,
        throw_mod: Option<String>,
    ) -> Self {
        Self {
            juggler,
            hand,
            index,
            slot,
            target_juggler,
            target_hand,
            target_index,
            target_slot,
            throw_mod,
            hands_beat: 0,
            primary: None,
            source: None,
            target: None,
            path_num: -1,
            catching: false,
            catch_num: -1,
            dwell_window: 0,
            throw_time: 0.0,
            catch_time: 0.0,
        }
    }

    pub fn matrix_ref(&self) -> MhnThrowRef {
        MhnThrowRef {
            juggler: self.juggler,
            hand: self.hand,
            index: self.index,
            slot: self.slot,
        }
    }

    pub fn is_hold(&self) -> bool {
        if self.throw_value() > 2
            || self.hand != self.target_hand
            || self.juggler != self.target_juggler
        {
            return false;
        }
        self.throw_mod
            .as_deref()
            .is_none_or(|value| !value.contains('T'))
    }

    pub fn is_zero(&self) -> bool {
        self.throw_value() == 0
    }

    pub fn throw_value(&self) -> isize {
        self.target_index - self.index
    }

    pub fn is_thrown_one(&self) -> bool {
        self.throw_mod
            .as_deref()
            .is_some_and(|value| !value.starts_with('H') && self.throw_value() == 1)
    }

    pub fn compare_to(&self, other: &Self) -> Ordering {
        let beats1 = self.throw_value();
        let beats2 = other.throw_value();

        if beats1 > beats2 {
            return Ordering::Greater;
        } else if beats1 < beats2 {
            return Ordering::Less;
        }

        let is_pass1 = self.target_juggler != self.juggler;
        let is_pass2 = other.target_juggler != other.juggler;
        let is_cross1 = (self.target_hand == self.hand) ^ (beats1 % 2 == 0);
        let is_cross2 = (other.target_hand == other.hand) ^ (beats2 % 2 == 0);

        if is_pass1 && !is_pass2 {
            return Ordering::Greater;
        } else if !is_pass1 && is_pass2 {
            return Ordering::Less;
        }

        if is_pass1 {
            if self.target_juggler < other.target_juggler {
                return Ordering::Greater;
            } else if self.target_juggler > other.target_juggler {
                return Ordering::Less;
            }
        }

        if is_cross1 && !is_cross2 {
            return Ordering::Greater;
        } else if !is_cross1 && is_cross2 {
            return Ordering::Less;
        }

        match (&self.throw_mod, &other.throw_mod) {
            (Some(_), None) => return Ordering::Greater,
            (None, Some(_)) => return Ordering::Less,
            (Some(left), Some(right)) => match left.cmp(right) {
                Ordering::Less => return Ordering::Greater,
                Ordering::Greater => return Ordering::Less,
                Ordering::Equal => {}
            },
            (None, None) => {}
        }

        if self.index != other.index {
            return self.index.cmp(&other.index);
        }
        if self.juggler != other.juggler {
            return self.juggler.cmp(&other.juggler);
        }
        if self.hand != other.hand {
            return other.hand.cmp(&self.hand);
        }

        Ordering::Equal
    }
}

impl PartialEq for MhnThrow {
    fn eq(&self, other: &Self) -> bool {
        self.juggler == other.juggler
            && self.hand == other.hand
            && self.index == other.index
            && self.slot == other.slot
            && self.target_juggler == other.target_juggler
            && self.target_hand == other.target_hand
            && self.target_index == other.target_index
            && self.target_slot == other.target_slot
            && self.throw_mod == other.throw_mod
    }
}

impl Eq for MhnThrow {}

impl PartialOrd for MhnThrow {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.compare_to(other))
    }
}

impl Ord for MhnThrow {
    fn cmp(&self, other: &Self) -> Ordering {
        self.compare_to(other)
    }
}

impl fmt::Display for MhnThrow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "({}, {}, {}, {} -> {}, {}, {}, {})",
            self.juggler,
            self.hand,
            self.index,
            self.slot,
            self.target_juggler,
            self.target_hand,
            self.target_index,
            self.target_slot
        )?;
        if self.primary == Some(self.matrix_ref()) {
            write!(f, "*")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_throw_properties() {
        let hold = MhnThrow::new(1, RIGHT_HAND, 0, 0, 1, RIGHT_HAND, 2, -1, None);
        assert!(hold.is_hold());
        assert_eq!(hold.throw_value(), 2);
        assert!(!hold.is_zero());

        let zero = MhnThrow::new(1, RIGHT_HAND, 0, 0, 1, RIGHT_HAND, 0, -1, None);
        assert!(zero.is_zero());
    }

    #[test]
    fn detects_thrown_one() {
        let thrown = MhnThrow::new(
            1,
            RIGHT_HAND,
            0,
            0,
            1,
            LEFT_HAND,
            1,
            -1,
            Some("T".to_string()),
        );
        assert!(thrown.is_thrown_one());
    }

    #[test]
    fn compares_by_throw_value_first() {
        let short = MhnThrow::new(1, RIGHT_HAND, 0, 0, 1, LEFT_HAND, 3, -1, None);
        let long = MhnThrow::new(1, RIGHT_HAND, 0, 0, 1, RIGHT_HAND, 5, -1, None);
        assert_eq!(short.compare_to(&long), Ordering::Less);
    }

    #[test]
    fn display_marks_primary_self_reference() {
        let mut throw = MhnThrow::new(1, RIGHT_HAND, 0, 0, 1, LEFT_HAND, 3, -1, None);
        throw.primary = Some(throw.matrix_ref());
        assert_eq!(throw.to_string(), "(1, 0, 0, 0 -> 1, 1, 3, -1)*");
    }
}

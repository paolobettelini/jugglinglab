use crate::permutation::Permutation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MhnSymmetryType {
    Delay,
    Switch,
    SwitchDelay,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MhnSymmetry {
    pub symmetry_type: MhnSymmetryType,
    pub number_of_jugglers: usize,
    pub jug_perm: Option<String>,
    pub delay: isize,
    pub juggler_perm: Permutation,
}

impl MhnSymmetry {
    pub const TYPE_DELAY: i32 = 1;
    pub const TYPE_SWITCH: i32 = 2;
    pub const TYPE_SWITCHDELAY: i32 = 3;

    pub fn new(
        symmetry_type: MhnSymmetryType,
        number_of_jugglers: usize,
        jug_perm: Option<String>,
        delay: isize,
    ) -> Result<Self, String> {
        let juggler_perm = if let Some(perm) = &jug_perm {
            Permutation::parse(number_of_jugglers, perm, true)?
        } else {
            Permutation::new(number_of_jugglers, true)
        };

        Ok(Self {
            symmetry_type,
            number_of_jugglers,
            jug_perm,
            delay,
            juggler_perm,
        })
    }

    pub fn type_code(&self) -> i32 {
        match self.symmetry_type {
            MhnSymmetryType::Delay => Self::TYPE_DELAY,
            MhnSymmetryType::Switch => Self::TYPE_SWITCH,
            MhnSymmetryType::SwitchDelay => Self::TYPE_SWITCHDELAY,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_identity_juggler_permutation() {
        let sym = MhnSymmetry::new(MhnSymmetryType::Delay, 2, None, 3).unwrap();
        assert_eq!(sym.type_code(), MhnSymmetry::TYPE_DELAY);
        assert_eq!(sym.juggler_perm.map(1), 1);
        assert_eq!(sym.juggler_perm.map(-1), -1);
    }

    #[test]
    fn parses_reversing_juggler_permutation() {
        let sym = MhnSymmetry::new(
            MhnSymmetryType::SwitchDelay,
            2,
            Some("(1,2*)".to_string()),
            1,
        )
        .unwrap();
        assert_eq!(sym.type_code(), MhnSymmetry::TYPE_SWITCHDELAY);
        assert_eq!(sym.juggler_perm.map(1), -2);
        assert_eq!(sym.juggler_perm.map(-2), 1);
    }
}

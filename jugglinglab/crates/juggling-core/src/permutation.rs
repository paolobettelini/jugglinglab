use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Permutation {
    size: usize,
    mapping: Vec<i32>,
    reverses: bool,
}

impl Permutation {
    pub fn identity(size: usize) -> Self {
        Self::new(size, false)
    }

    pub fn new(size: usize, reverses: bool) -> Self {
        let mapping = if reverses {
            (0..(size * 2 + 1))
                .map(|index| index as i32 - size as i32)
                .collect()
        } else {
            (1..=size).map(|index| index as i32).collect()
        };

        Self {
            size,
            mapping,
            reverses,
        }
    }

    pub fn from_mapping(size: usize, mapping: Vec<i32>, reverses: bool) -> Self {
        Self {
            size,
            mapping,
            reverses,
        }
    }

    pub fn parse(size: usize, perm: &str, reverses: bool) -> Result<Self, String> {
        if size == 0 && perm.trim().is_empty() {
            return Ok(Self::new(size, reverses));
        }

        let mut permutation = Self {
            size,
            mapping: if reverses {
                vec![0; size * 2 + 1]
            } else {
                vec![0; size]
            },
            reverses,
        };
        let mut used = if reverses {
            vec![false; size * 2 + 1]
        } else {
            vec![false; size]
        };

        if !perm.contains('(') {
            permutation.parse_explicit_mapping(perm, &mut used)?;
        } else {
            permutation.parse_cycle_mapping(perm, &mut used)?;
        }

        if reverses {
            for elem in 1..=size as i32 {
                let pos = permutation.reverse_index(elem);
                let neg = permutation.reverse_index(-elem);
                match (used[pos], used[neg]) {
                    (true, false) => permutation.mapping[neg] = -permutation.mapping[pos],
                    (false, true) => permutation.mapping[pos] = -permutation.mapping[neg],
                    (false, false) => {
                        permutation.mapping[neg] = 0;
                        permutation.mapping[pos] = 0;
                    }
                    (true, true) => {}
                }
            }
        } else {
            for (index, slot_used) in used.iter().enumerate() {
                if !slot_used {
                    permutation.mapping[index] = index as i32 + 1;
                }
            }
        }

        Ok(permutation)
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn has_reverses(&self) -> bool {
        self.reverses
    }

    pub fn map(&self, elem: i32) -> i32 {
        if self.reverses {
            self.mapping[self.reverse_index(elem)]
        } else {
            self.mapping[(elem - 1) as usize]
        }
    }

    pub fn map_power(&self, elem: i32, power: i32) -> i32 {
        let mut current = elem;
        if power > 0 {
            for _ in 0..power {
                current = self.map(current);
            }
        } else if power < 0 {
            for _ in 0..(-power) {
                current = self.map_inverse(current);
            }
        }
        current
    }

    pub fn composed_with(&self, second: Option<&Permutation>) -> Permutation {
        let Some(second) = second else {
            return self.clone();
        };
        if self.size != second.size || self.reverses || second.reverses {
            return self.clone();
        }

        let mapping = (1..=self.size)
            .map(|elem| second.map(self.map(elem as i32)))
            .collect::<Vec<_>>();
        Self::from_mapping(self.size, mapping, false)
    }

    pub fn map_inverse(&self, elem: i32) -> i32 {
        if self.reverses {
            for (index, mapped) in self.mapping.iter().enumerate() {
                if *mapped == elem {
                    return index as i32 - self.size as i32;
                }
            }
        } else {
            for (index, mapped) in self.mapping.iter().enumerate() {
                if *mapped == elem {
                    return index as i32 + 1;
                }
            }
        }
        0
    }

    pub fn inverse(&self) -> Permutation {
        if self.reverses {
            let mut inverse = vec![0; self.size * 2 + 1];
            for index in 0..self.mapping.len() {
                inverse[self.reverse_index(self.mapping[index])] = index as i32 - self.size as i32;
            }
            Self::from_mapping(self.size, inverse, true)
        } else {
            let mut inverse = vec![0; self.size];
            for index in 0..self.mapping.len() {
                inverse[(self.mapping[index] - 1) as usize] = index as i32 + 1;
            }
            Self::from_mapping(self.size, inverse, false)
        }
    }

    pub fn order(&self) -> usize {
        let mut order = 1usize;
        for elem in 1..=self.size as i32 {
            if self.map(elem) != 0 {
                order = lcm(order, self.order_of(elem));
            }
        }
        order
    }

    pub fn max_order(&self) -> usize {
        let mut order = 1usize;
        for elem in 1..=self.size as i32 {
            if self.map(elem) != 0 {
                order = order.max(self.order_of(elem));
            }
        }
        order
    }

    pub fn order_of(&self, elem: i32) -> usize {
        let mut order = 1usize;
        let mut index = if self.reverses {
            self.reverse_index(elem)
        } else {
            (elem - 1) as usize
        };

        while self.mapping[index] != elem {
            order += 1;
            index = if self.reverses {
                self.reverse_index(self.mapping[index])
            } else {
                (self.mapping[index] - 1) as usize
            };
        }

        order
    }

    pub fn cycle_of(&self, elem: i32) -> Vec<i32> {
        let order = self.order_of(elem);
        let mut term = elem;
        let mut result = Vec::with_capacity(order);

        for _ in 0..order {
            result.push(term);
            term = if self.reverses {
                self.mapping[self.reverse_index(term)]
            } else {
                self.mapping[(term - 1) as usize]
            };
        }

        result
    }

    pub fn to_string_with_cycles(&self, cycle_notation: bool) -> String {
        if cycle_notation {
            self.to_cycle_string()
        } else {
            self.to_explicit_string()
        }
    }

    fn parse_explicit_mapping(&mut self, perm: &str, used: &mut [bool]) -> Result<(), String> {
        let tokens = perm.split(',').collect::<Vec<_>>();
        if tokens.len() != self.size && self.size != 0 {
            return Err(format!(
                "Invalid permutation format: expected {} elements",
                self.size
            ));
        }

        for (index, token) in tokens.iter().enumerate() {
            let num = token
                .trim()
                .parse::<i32>()
                .map_err(|_| "Invalid number in permutation".to_string())?;
            if num < 1 || num > self.size as i32 {
                return Err("Permutation element out of range".to_string());
            }
            let used_index = if self.reverses {
                self.reverse_index(num)
            } else {
                (num - 1) as usize
            };
            if used[used_index] {
                return Err("Permutation is not one-to-one".to_string());
            }

            used[used_index] = true;
            if self.reverses {
                let source = index as i32 + 1;
                let source_index = self.reverse_index(source);
                self.mapping[source_index] = num;
            } else {
                self.mapping[index] = num;
            }
        }

        Ok(())
    }

    fn parse_cycle_mapping(&mut self, perm: &str, used: &mut [bool]) -> Result<(), String> {
        for cycle_token in perm.split(')').filter(|token| !token.is_blank()) {
            let mut cycle = cycle_token.trim();
            if !cycle.starts_with('(') {
                return Err("Invalid parentheses in permutation".to_string());
            }
            cycle = &cycle[1..];

            let mut last_num = -(self.size as i32 + 1);
            for element_token in cycle.split(',') {
                let num = self.parse_cycle_element(element_token)?;
                if self.reverses {
                    self.add_reversing_cycle_element(num, last_num, used)?;
                } else {
                    self.add_cycle_element(num, last_num, used)?;
                }
                last_num = num;
            }
        }

        Ok(())
    }

    fn parse_cycle_element(&self, element: &str) -> Result<i32, String> {
        let mut token = element.trim();
        let mut negate = false;
        if self.reverses && token.ends_with('*') {
            negate = true;
            token = token[..token.len() - 1].trim();
        }

        let mut num = token
            .parse::<i32>()
            .map_err(|_| "Invalid number in permutation".to_string())?;
        if negate {
            num = -num;
        }
        Ok(num)
    }

    fn add_cycle_element(
        &mut self,
        num: i32,
        last_num: i32,
        used: &mut [bool],
    ) -> Result<(), String> {
        if num < 1 || num > self.size as i32 {
            return Err("Permutation element out of range".to_string());
        }
        let index = (num - 1) as usize;
        if used[index] {
            return Err("Permutation is not one-to-one".to_string());
        }
        used[index] = true;

        if last_num == -(self.size as i32 + 1) {
            self.mapping[index] = num;
        } else {
            let last_index = (last_num - 1) as usize;
            self.mapping[index] = self.mapping[last_index];
            self.mapping[last_index] = num;
        }

        Ok(())
    }

    fn add_reversing_cycle_element(
        &mut self,
        num: i32,
        last_num: i32,
        used: &mut [bool],
    ) -> Result<(), String> {
        if num < -(self.size as i32) || num > self.size as i32 || num == 0 {
            return Err("Permutation element out of range".to_string());
        }

        let index = self.reverse_index(num);
        if used[index] {
            return Err("Permutation is not one-to-one".to_string());
        }
        used[index] = true;

        if last_num == -(self.size as i32 + 1) {
            self.mapping[index] = num;
        } else {
            let last_index = self.reverse_index(last_num);
            self.mapping[index] = self.mapping[last_index];
            self.mapping[last_index] = num;
            let opposite_last = self.reverse_index(-last_num);
            if used[opposite_last] && self.mapping[opposite_last] != -num {
                return Err("Permutation is not reversible".to_string());
            }
        }

        Ok(())
    }

    fn to_cycle_string(&self) -> String {
        let mut output = String::new();

        if self.reverses {
            let mut printed = vec![false; self.size];
            for index in 0..self.size {
                if printed[index] {
                    continue;
                }
                let start = index as i32 + 1;
                printed[index] = true;
                let mut current = self.mapping[self.reverse_index(start)];
                if current != 0 {
                    output.push('(');
                    output.push_str(&convert_reverse(start));
                    while current != start {
                        if current > 0 {
                            printed[(current - 1) as usize] = true;
                        } else if current < 0 {
                            printed[(-current - 1) as usize] = true;
                        }
                        output.push(',');
                        output.push_str(&convert_reverse(current));
                        current = self.mapping[self.reverse_index(current)];
                    }
                    output.push(')');
                }
            }
        } else {
            let mut printed = vec![false; self.size];
            let mut left = self.size;
            while left > 0 {
                let start_index = printed
                    .iter()
                    .position(|value| !*value)
                    .expect("left count tracks unprinted elements");
                let start = start_index as i32 + 1;
                printed[start_index] = true;
                output.push('(');
                output.push_str(&start.to_string());
                left -= 1;

                let mut current = self.mapping[start_index];
                while current != start {
                    output.push(',');
                    output.push_str(&current.to_string());
                    printed[(current - 1) as usize] = true;
                    left -= 1;
                    current = self.mapping[(current - 1) as usize];
                }
                output.push(')');
            }
        }

        output
    }

    fn to_explicit_string(&self) -> String {
        if self.size == 0 {
            return String::new();
        }

        if self.reverses {
            let mut output = convert_reverse(self.mapping[self.size + 1]);
            for index in 1..self.size {
                output.push(',');
                output.push_str(&convert_reverse(self.mapping[self.size + 1 + index]));
            }
            output
        } else {
            self.mapping
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        }
    }

    fn reverse_index(&self, elem: i32) -> usize {
        (elem + self.size as i32) as usize
    }
}

impl fmt::Display for Permutation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_cycle_string())
    }
}

fn convert_reverse(num: i32) -> String {
    if num >= 0 {
        num.to_string()
    } else {
        format!("{}*", -num)
    }
}

pub fn lcm(x: usize, y: usize) -> usize {
    if x == 0 || y == 0 {
        return 0;
    }

    let x0 = x;
    let y0 = y;
    let mut x = x;
    let mut y = y;
    let mut gcd = y;

    while x > 0 {
        gcd = x;
        x = y % x;
        y = gcd;
    }

    (x0 * y0) / gcd
}

trait Blank {
    fn is_blank(&self) -> bool;
}

impl Blank for str {
    fn is_blank(&self) -> bool {
        self.trim().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_outputs_cycle_notation() {
        let perm = Permutation::identity(3);
        assert_eq!(perm.to_string(), "(1)(2)(3)");
        assert_eq!(perm.to_string_with_cycles(false), "1,2,3");
        assert_eq!(perm.order(), 1);
    }

    #[test]
    fn parses_explicit_mapping() {
        let perm = Permutation::parse(3, "2,3,1", false).unwrap();
        assert_eq!(perm.map(1), 2);
        assert_eq!(perm.map_power(1, 2), 3);
        assert_eq!(perm.map_inverse(2), 1);
        assert_eq!(perm.inverse().map(2), 1);
        assert_eq!(perm.order(), 3);
        assert_eq!(perm.cycle_of(1), vec![1, 2, 3]);
    }

    #[test]
    fn parses_cycle_mapping() {
        let perm = Permutation::parse(4, "(1,3,2)", false).unwrap();
        assert_eq!(perm.map(1), 3);
        assert_eq!(perm.map(3), 2);
        assert_eq!(perm.map(2), 1);
        assert_eq!(perm.map(4), 4);
        assert_eq!(perm.to_string(), "(1,3,2)(4)");
    }

    #[test]
    fn parses_reversing_cycle_mapping() {
        let perm = Permutation::parse(2, "(1,2*)", true).unwrap();
        assert_eq!(perm.map(1), -2);
        assert_eq!(perm.map(-2), 1);
        assert_eq!(perm.map(-1), 2);
        assert_eq!(perm.map(2), -1);
        assert_eq!(perm.to_string(), "(1,2*)");
    }

    #[test]
    fn parses_explicit_reversing_mapping() {
        let perm = Permutation::parse(2, "2,1", true).unwrap();
        assert_eq!(perm.map(1), 2);
        assert_eq!(perm.map(2), 1);
        assert_eq!(perm.map(-1), -2);
        assert_eq!(perm.map(-2), -1);
    }

    #[test]
    fn computes_lcm() {
        assert_eq!(lcm(6, 8), 24);
    }
}

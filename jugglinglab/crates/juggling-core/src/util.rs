pub fn expand_repeats(input: &str) -> String {
    let chars = input.chars().collect::<Vec<_>>();
    let mut output = String::new();
    add_expansion_to_buffer(&chars, &mut output);
    output
}

fn add_expansion_to_buffer(chars: &[char], output: &mut String) {
    let mut pos = 0usize;

    while pos < chars.len() {
        if chars[pos] == '(' {
            if let Some((repeat_end, repeats, resume_start)) = try_parse_repeat(chars, pos) {
                let repeated = &chars[pos + 1..repeat_end];
                for _ in 0..repeats {
                    add_expansion_to_buffer(repeated, output);
                }
                pos = resume_start;
            } else {
                output.push(chars[pos]);
                pos += 1;
            }
        } else {
            output.push(chars[pos]);
            pos += 1;
        }
    }
}

fn try_parse_repeat(chars: &[char], from_pos: usize) -> Option<(usize, usize, usize)> {
    let mut depth = 0usize;

    for pos in from_pos..chars.len() {
        match chars[pos] {
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    let mut scan = pos + 1;
                    while scan < chars.len() && chars[scan].is_whitespace() {
                        scan += 1;
                    }
                    if chars.get(scan) != Some(&'^') {
                        return None;
                    }
                    scan += 1;
                    while scan < chars.len() && chars[scan].is_whitespace() {
                        scan += 1;
                    }

                    let digits_start = scan;
                    while scan < chars.len() && chars[scan].is_ascii_digit() {
                        scan += 1;
                    }
                    if scan == digits_start {
                        return None;
                    }

                    let repeats = chars[digits_start..scan]
                        .iter()
                        .collect::<String>()
                        .parse::<usize>()
                        .ok()?;
                    return Some((pos, repeats, scan));
                }
            }
            _ => {}
        }
    }

    None
}

pub fn split_on_char_outside_parens(input: &str, delimiter: char) -> Vec<String> {
    if input.is_empty() {
        return vec![String::new()];
    }

    let mut result = Vec::new();
    let mut paren_level = 0i32;
    let mut current = String::new();

    for ch in input.chars() {
        match ch {
            '(' => {
                paren_level += 1;
                current.push(ch);
            }
            ')' => {
                paren_level -= 1;
                current.push(ch);
            }
            _ if ch == delimiter && paren_level == 0 => {
                result.push(current);
                current = String::new();
            }
            _ => current.push(ch),
        }
    }

    result.push(current);
    if result.len() > 1 && result.last().is_some_and(|part| part.is_empty()) {
        result.pop();
    }
    result
}

pub fn parse_finite_double(input: &str) -> Result<f64, String> {
    let value = input
        .trim()
        .parse::<f64>()
        .map_err(|_| format!("Invalid number: {input}"))?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(format!("Non-finite number: {input}"))
    }
}

pub fn to_string_rounded(value: f64, digits: usize) -> String {
    let digits = digits.min(10);
    let mut value = value;
    if digits == 0 {
        value = value.round();
    }

    let mut result = format!("{value:.digits$}");
    if let Some(dot) = result.find('.') {
        let mut end = result.len();
        while end > dot + 1 && result.as_bytes()[end - 1] == b'0' {
            end -= 1;
        }
        if end == dot + 1 {
            end = dot;
        }
        result.truncate(end);
    }

    if result == "-0" {
        "0".to_string()
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_nested_repeats() {
        assert_eq!(expand_repeats("he(l)^2o"), "hello");
        assert_eq!(expand_repeats("((ab)^2c)^2"), "ababcababc");
        assert_eq!(expand_repeats("(hello)^0world"), "world");
    }

    #[test]
    fn splits_only_outside_parens() {
        assert_eq!(
            split_on_char_outside_parens("(1,2).(3.4).", '.'),
            vec!["(1,2)", "(3.4)"]
        );
    }

    #[test]
    fn rounds_like_jugglinglab_strings() {
        assert_eq!(to_string_rounded(32.5, 4), "32.5");
        assert_eq!(to_string_rounded(45.0, 4), "45");
        assert_eq!(to_string_rounded(-0.00001, 4), "0");
    }
}

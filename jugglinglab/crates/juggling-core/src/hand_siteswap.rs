use crate::permutation::lcm;

const HSS_DWELL_DEFAULT: f64 = 0.3;

#[derive(Clone, Debug, PartialEq)]
pub struct HandSiteswapResult {
    pub converted_pattern: String,
    pub dwell_beats: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq)]
struct ObjectPattern {
    throws: Vec<Vec<char>>,
    bounce_modifiers: Vec<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq)]
struct HandPattern {
    throws: Vec<char>,
    hands: usize,
}

pub fn process_hss(
    pattern: &str,
    hss: &str,
    hold: bool,
    dwellmax: bool,
    handspec: Option<&str>,
    dwell: f64,
) -> Result<HandSiteswapResult, String> {
    let mut object = parse_object_siteswap(pattern)?;
    validate_object_permutation(&object.throws)?;

    let mut hands = parse_hand_siteswap(hss)?;
    let hand_orbit = validate_hand_permutation(&hands.throws)?;
    let handmap = if let Some(handspec) = handspec {
        parse_handspec(handspec, hands.hands)?
    } else {
        default_handspec(hands.hands)
    };
    let jugglers = handmap.iter().map(|item| item[0]).max().unwrap_or(1).max(1);

    convert_notation(
        &mut object,
        &mut hands,
        hand_orbit,
        &handmap,
        jugglers,
        hold,
        dwellmax,
        dwell,
    )
}

fn parse_object_siteswap(input: &str) -> Result<ObjectPattern, String> {
    let chars = input.chars().collect::<Vec<_>>();
    let mut throws = Vec::<Vec<char>>::new();
    let mut bounce_modifiers = Vec::<Vec<String>>::new();
    let mut throw_sum = 0usize;
    let mut pos = 0usize;
    let mut saw_throw = false;

    while pos < chars.len() {
        match chars[pos] {
            ch if ch.is_whitespace() || ch == ',' || ch == '.' => pos += 1,
            '[' => {
                pos += 1;
                let mut beat = Vec::new();
                let mut beat_mods = Vec::new();
                while pos < chars.len() && chars[pos] != ']' {
                    if chars[pos].is_whitespace() || chars[pos] == ',' {
                        pos += 1;
                        continue;
                    }
                    let value = numeric_value(chars[pos]).ok_or_else(|| {
                        format!("Invalid HSS object throw at position {}", pos + 1)
                    })?;
                    beat.push(chars[pos]);
                    throw_sum += value;
                    saw_throw = true;
                    pos += 1;
                    beat_mods.push(parse_bounce_modifier(&chars, &mut pos)?);
                }
                if pos >= chars.len() || chars[pos] != ']' || beat.is_empty() {
                    return Err("Invalid HSS object multiplex syntax".to_string());
                }
                pos += 1;
                throws.push(beat);
                bounce_modifiers.push(beat_mods);
            }
            ch => {
                let value = numeric_value(ch)
                    .ok_or_else(|| format!("Invalid HSS object throw at position {}", pos + 1))?;
                throw_sum += value;
                saw_throw = true;
                pos += 1;
                let modifier = parse_bounce_modifier(&chars, &mut pos)?;
                throws.push(vec![ch]);
                bounce_modifiers.push(vec![modifier]);
            }
        }
    }

    if !saw_throw || throws.is_empty() {
        return Err("HSS object siteswap is empty".to_string());
    }
    if throw_sum % throws.len() != 0 {
        return Err("HSS object siteswap has an invalid average".to_string());
    }

    for modifiers in &mut bounce_modifiers {
        for modifier in modifiers {
            modifier.push(' ');
        }
    }

    Ok(ObjectPattern {
        throws,
        bounce_modifiers,
    })
}

fn parse_bounce_modifier(chars: &[char], pos: &mut usize) -> Result<String, String> {
    let mut modifier = String::new();
    if chars.get(*pos) != Some(&'B') {
        return Ok(modifier);
    }

    modifier.push('B');
    *pos += 1;

    match chars.get(*pos).copied() {
        Some('F' | 'L') => {
            modifier.push(chars[*pos]);
            *pos += 1;
        }
        Some('H') => {
            modifier.push('H');
            *pos += 1;
            if matches!(chars.get(*pos), Some('F' | 'L')) {
                modifier.push(chars[*pos]);
                *pos += 1;
            }
        }
        _ => {}
    }

    Ok(modifier)
}

fn parse_hand_siteswap(input: &str) -> Result<HandPattern, String> {
    let mut throws = Vec::new();
    let mut throw_sum = 0usize;

    for (index, ch) in input.chars().enumerate() {
        if ch.is_whitespace() || ch == ',' || ch == '.' {
            continue;
        }
        let value = numeric_value(ch)
            .ok_or_else(|| format!("Invalid HSS hand throw at position {}", index + 1))?;
        throws.push(ch);
        throw_sum += value;
    }

    if throws.is_empty() {
        return Err("HSS hand siteswap is empty".to_string());
    }
    if throw_sum % throws.len() != 0 {
        return Err("HSS hand siteswap has an invalid average".to_string());
    }

    Ok(HandPattern {
        hands: throw_sum / throws.len(),
        throws,
    })
}

fn validate_object_permutation(throws: &[Vec<char>]) -> Result<(), String> {
    let period = throws.len();
    let mut catches = vec![0usize; period];

    for (index, beat) in throws.iter().enumerate() {
        for throw in beat {
            let target = (index + numeric_value(*throw).unwrap_or(0)) % period;
            catches[target] += 1;
        }
    }

    for (index, beat) in throws.iter().enumerate() {
        if catches[index] != beat.len() {
            return Err("HSS object siteswap is not a valid permutation".to_string());
        }
    }

    Ok(())
}

fn validate_hand_permutation(throws: &[char]) -> Result<usize, String> {
    let period = throws.len();
    let mut targets = vec![0usize; period];
    let mut catches = vec![0usize; period];
    for (index, throw) in throws.iter().enumerate() {
        let target = (index + numeric_value(*throw).unwrap_or(0)) % period;
        targets[index] = target;
        catches[target] += 1;
    }

    if catches.iter().any(|count| *count != 1) {
        return Err("HSS hand siteswap is not a valid permutation".to_string());
    }

    let mut orbit_period = 1usize;
    let mut touched = vec![false; period];
    for index in 0..period {
        if touched[index] {
            continue;
        }
        let mut orbit = numeric_value(throws[index]).unwrap_or(0);
        touched[index] = true;
        let mut cursor = targets[index];
        while cursor != index {
            orbit += numeric_value(throws[cursor]).unwrap_or(0);
            touched[cursor] = true;
            cursor = targets[cursor];
        }
        if orbit != 0 {
            orbit_period = lcm(orbit_period, orbit);
        }
    }

    Ok(orbit_period)
}

fn parse_handspec(handspec: &str, hands: usize) -> Result<Vec<[usize; 2]>, String> {
    let mut handmap = vec![[0usize; 2]; hands];
    let chars = handspec.chars().collect::<Vec<_>>();
    let mut pos = 0usize;
    let mut juggler = 0usize;

    while pos < chars.len() {
        if chars[pos].is_whitespace() {
            pos += 1;
            continue;
        }
        if chars[pos] != '(' {
            return Err(format!("Invalid handspec syntax at position {}", pos + 1));
        }
        let close = chars[pos + 1..]
            .iter()
            .position(|ch| *ch == ')')
            .map(|offset| pos + 1 + offset)
            .ok_or_else(|| "Invalid handspec syntax: missing ')'".to_string())?;
        let group = chars[pos + 1..close].iter().collect::<String>();
        let (left, right) = group
            .split_once(',')
            .ok_or_else(|| "Invalid handspec syntax: missing ','".to_string())?;
        juggler += 1;
        assign_handspec_side(&mut handmap, hands, juggler, 0, left.trim())?;
        assign_handspec_side(&mut handmap, hands, juggler, 1, right.trim())?;
        if left.trim().is_empty() && right.trim().is_empty() {
            return Err("Each handspec juggler must contain at least one hand".to_string());
        }
        pos = close + 1;
    }

    if juggler == 0 {
        return Err("Invalid handspec syntax".to_string());
    }
    for (index, entry) in handmap.iter().enumerate() {
        if entry[0] == 0 {
            return Err(format!(
                "HSS hand {} is not assigned to a juggler",
                index + 1
            ));
        }
    }

    Ok(handmap)
}

fn assign_handspec_side(
    handmap: &mut [[usize; 2]],
    hands: usize,
    juggler: usize,
    hand: usize,
    value: &str,
) -> Result<(), String> {
    if value.is_empty() {
        return Ok(());
    }
    let hand_number = value
        .parse::<usize>()
        .map_err(|_| format!("Invalid handspec hand number: {value}"))?;
    if hand_number == 0 || hand_number > hands {
        return Err(format!("HSS hand number out of range: {hand_number}"));
    }
    if handmap[hand_number - 1][0] != 0 {
        return Err(format!("HSS hand {hand_number} is assigned more than once"));
    }
    handmap[hand_number - 1] = [juggler, hand];
    Ok(())
}

fn default_handspec(hands: usize) -> Vec<[usize; 2]> {
    let jugglers = if hands % 2 == 0 {
        hands / 2
    } else {
        (hands + 1) / 2
    };
    let mut handmap = vec![[0usize; 2]; hands];
    for (index, entry) in handmap.iter_mut().enumerate() {
        if index < jugglers {
            *entry = [index + 1, 1];
        } else {
            *entry = [index + 1 - jugglers, 0];
        }
    }
    handmap
}

#[allow(clippy::too_many_arguments)]
fn convert_notation(
    object: &mut ObjectPattern,
    hands: &mut HandPattern,
    hand_orbit: usize,
    handmap: &[[usize; 2]],
    jugglers: usize,
    hold: bool,
    dwellmax: bool,
    dwell: f64,
) -> Result<HandSiteswapResult, String> {
    let object_period = object.throws.len();
    let hand_period = hands.throws.len();
    let pattern_period = lcm(object_period, hand_orbit);

    for index in object_period..pattern_period {
        object
            .throws
            .push(object.throws[index - object_period].clone());
        object
            .bounce_modifiers
            .push(object.bounce_modifiers[index - object_period].clone());
    }
    for index in hand_period..pattern_period {
        hands.throws.push(hands.throws[index - hand_period]);
    }

    let mut assigned_hand = vec![0usize; pattern_period];
    let mut assign_done = vec![false; pattern_period];
    let mut juggler_info = vec![[0usize, usize::MAX]; pattern_period];

    let mut current_hand = 0usize;
    for index in 0..pattern_period {
        if hands.throws[index] == '0' {
            if object.throws[index].iter().any(|throw| *throw != '0') {
                return Err(format!(
                    "HSS has no hand available to throw at beat {}",
                    index + 1
                ));
            }
            assign_done[index] = true;
            continue;
        }

        if !assign_done[index] {
            current_hand += 1;
            assigned_hand[index] = current_hand;
            assign_done[index] = true;
            let mut next =
                (index + numeric_value(hands.throws[index]).unwrap_or(0)) % pattern_period;
            while next != index {
                assigned_hand[next] = current_hand;
                assign_done[next] = true;
                next = (next + numeric_value(hands.throws[next]).unwrap_or(0)) % pattern_period;
            }
        }

        let hand_number = assigned_hand[index].saturating_sub(1);
        let Some(info) = handmap.get(hand_number) else {
            return Err("HSS hand map does not cover all hands".to_string());
        };
        juggler_info[index] = *info;
    }

    let mut dwell_beats = dwell_beats(
        &object.throws,
        &hands.throws,
        &juggler_info,
        pattern_period,
        dwellmax,
        dwell,
    );
    remove_dwell_clashes(&mut dwell_beats);

    let modifiers = hss_throw_modifiers(
        &object.throws,
        &object.bounce_modifiers,
        &hands.throws,
        &juggler_info,
        hold,
    );
    let converted_pattern = converted_pattern(&object.throws, &modifiers, &juggler_info, jugglers);

    Ok(HandSiteswapResult {
        converted_pattern,
        dwell_beats,
    })
}

fn dwell_beats(
    object: &[Vec<char>],
    hands: &[char],
    juggler_info: &[[usize; 2]],
    pattern_period: usize,
    dwellmax: bool,
    default_dwell: f64,
) -> Vec<f64> {
    let mut min_caught = vec![0usize; pattern_period];
    for (index, beat) in object.iter().enumerate() {
        for throw in beat {
            let value = numeric_value(*throw).unwrap_or(0);
            let target = (index + value) % pattern_period;
            if value > 0 && (min_caught[target] == 0 || value < min_caught[target]) {
                min_caught[target] = value;
            }
        }
    }

    let mut result = vec![0.0; pattern_period];
    if !dwellmax {
        let successive_same_hand = (0..pattern_period).any(|index| {
            juggler_info[index][0] == juggler_info[(index + 1) % pattern_period][0]
                && juggler_info[index][1] == juggler_info[(index + 1) % pattern_period][1]
        });
        for index in 0..pattern_period {
            result[index] = if successive_same_hand {
                HSS_DWELL_DEFAULT
            } else {
                default_dwell
            };
            clamp_dwell_to_min_caught(&mut result[index], min_caught[index]);
        }
    } else {
        for index in 0..pattern_period {
            let mut cursor = (index + 1) % pattern_period;
            let mut diff = 1usize;
            while juggler_info[index][0] != juggler_info[cursor][0]
                || juggler_info[index][1] != juggler_info[cursor][1]
            {
                cursor = (cursor + 1) % pattern_period;
                diff += 1;
            }
            result[cursor] = diff as f64 - (1.0 - HSS_DWELL_DEFAULT);
        }
        for index in 0..pattern_period {
            clamp_dwell_to_min_caught(&mut result[index], min_caught[index]);
            if result[index] <= 0.0 {
                result[index] = HSS_DWELL_DEFAULT;
            }
        }
    }

    for index in 0..pattern_period {
        if hands[index] == '0' {
            result[index] = HSS_DWELL_DEFAULT;
        }
    }
    result
}

fn clamp_dwell_to_min_caught(dwell: &mut f64, min_caught: usize) {
    if min_caught > 0 && *dwell >= min_caught as f64 {
        *dwell = min_caught as f64 - (1.0 - HSS_DWELL_DEFAULT);
    }
}

fn remove_dwell_clashes(dwell_beats: &mut [f64]) {
    let pattern_period = dwell_beats.len();
    let mut clash = vec![false; pattern_period];
    for index in 0..pattern_period {
        let mut clash_count = 0usize;
        for offset in 1..pattern_period {
            let lhs = dwell_beats[(index + offset) % pattern_period] - dwell_beats[index];
            if (lhs - offset as f64).rem_euclid(pattern_period as f64) == 0.0 {
                clash[(index + offset) % pattern_period] = true;
                clash_count += 1;
            }
        }
        while clash_count != 0 {
            for item in 0..pattern_period {
                if clash[item] {
                    dwell_beats[item] += HSS_DWELL_DEFAULT / clash_count as f64;
                    clash_count -= 1;
                    clash[item] = false;
                }
            }
        }
    }
}

fn hss_throw_modifiers(
    object: &[Vec<char>],
    bounce_modifiers: &[Vec<String>],
    hands: &[char],
    juggler_info: &[[usize; 2]],
    hold: bool,
) -> Vec<Vec<String>> {
    let pattern_period = object.len();
    let mut modifiers = vec![Vec::<String>::new(); pattern_period];

    for index in 0..pattern_period {
        for (slot, throw) in object[index].iter().enumerate() {
            let value = numeric_value(*throw).unwrap_or(0);
            let target_index = (index + value) % pattern_period;
            let source_juggler = juggler_info[index][0];
            let source_hand = juggler_info[index][1];
            let target_juggler = juggler_info[target_index][0];
            let target_hand = juggler_info[target_index][1];

            let mut modifier = String::new();
            if (value % 2 == 0 && source_hand != target_hand)
                || (value % 2 != 0 && source_hand == target_hand)
            {
                modifier.push('x');
            }
            if source_juggler != target_juggler {
                modifier.push('p');
                modifier.push_str(&target_juggler.to_string());
            } else if hold && value == numeric_value(hands[index]).unwrap_or(0) {
                modifier.push('H');
            }
            modifier.push_str(&bounce_modifiers[index][slot]);
            modifiers[index].push(modifier);
        }
    }

    modifiers
}

fn converted_pattern(
    object: &[Vec<char>],
    modifiers: &[Vec<String>],
    juggler_info: &[[usize; 2]],
    jugglers: usize,
) -> String {
    let mut pattern = String::new();
    for index in 0..object.len() {
        pattern.push('<');
        for juggler in 1..=jugglers {
            if juggler != 1 {
                pattern.push('|');
            }
            if juggler_info[index][1] == 0 {
                pattern.push('(');
                if juggler_info[index][0] == juggler {
                    append_hss_slot(&mut pattern, &object[index], &modifiers[index]);
                    pattern.push_str(",0)!");
                } else {
                    pattern.push_str("0,0)!");
                }
            } else {
                pattern.push_str("(0,");
                if juggler_info[index][0] == juggler {
                    append_hss_slot(&mut pattern, &object[index], &modifiers[index]);
                    pattern.push_str(")!");
                } else {
                    pattern.push_str("0)!");
                }
            }
        }
        pattern.push('>');
    }
    pattern
}

fn append_hss_slot(output: &mut String, throws: &[char], modifiers: &[String]) {
    if throws.len() > 1 {
        output.push('[');
    }
    for (throw, modifier) in throws.iter().zip(modifiers) {
        output.push(*throw);
        output.push_str(modifier);
    }
    if throws.len() > 1 {
        output.push(']');
    }
}

fn numeric_value(ch: char) -> Option<usize> {
    if ch.is_ascii_digit() {
        ch.to_digit(10).map(|value| value as usize)
    } else if ch.is_ascii_lowercase() {
        Some(ch as usize - 'a' as usize + 10)
    } else if ch.is_ascii_uppercase() {
        Some(ch as usize - 'A' as usize + 10)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_two_handed_hss_to_passing_sync_notation() {
        let result = process_hss("3", "114", false, true, None, 1.3).unwrap();
        assert!(result.converted_pattern.contains('<'));
        assert!(result.converted_pattern.contains("(0,3"));
        assert_eq!(result.dwell_beats.len(), 6);
    }

    #[test]
    fn converts_multi_juggler_hss_with_handspec() {
        let result = process_hss("3773", "2266", false, true, Some("(1,2)(3,4)"), 1.3).unwrap();
        assert!(result.converted_pattern.contains("p2"));
        assert_eq!(result.dwell_beats.len(), 8);
    }

    #[test]
    fn preserves_bounce_modifiers() {
        let result = process_hss("3BHL", "2", false, true, None, 1.3).unwrap();
        assert!(result.converted_pattern.contains("BHL"));
    }
}

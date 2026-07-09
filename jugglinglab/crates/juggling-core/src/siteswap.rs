use crate::hand_siteswap::process_hss;
use crate::mhn_body::MhnBody;
use crate::mhn_hands::MhnHands;
use crate::util::expand_repeats;

const DWELL_DEFAULT: f64 = 1.3;
const SQUEEZEBEATS_DEFAULT: f64 = 0.4;

#[derive(Clone, Debug, PartialEq)]
pub struct SiteswapSpec {
    pub raw_config: String,
    pub pattern: String,
    pub title: Option<String>,
    pub bps: f64,
    pub dwell: f64,
    pub squeezebeats: f64,
    pub dwell_array: Option<Vec<f64>>,
    pub prop_name: String,
    pub prop_diam: f64,
    pub colors: Option<String>,
    pub hands: Option<MhnHands>,
    pub body: Option<MhnBody>,
    pub jugglers: usize,
    pub beats: Vec<Beat>,
    pub sync: bool,
    pub vanilla_async: bool,
    pub balls: usize,
    pub max_throw: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Beat {
    pub throws: Vec<ThrowSpec>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ThrowSpec {
    pub value: u32,
    pub hand: Hand,
    pub cross: bool,
    pub source_juggler: usize,
    pub target_juggler: usize,
    pub modifier: Option<String>,
    pub hand_fixed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hand {
    Left,
    Right,
}

pub fn parse_config(config: &str) -> Result<SiteswapSpec, String> {
    let config = config.trim();
    let params = parse_params(config);
    let mut pattern = params
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("pattern"))
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| config.to_string());

    if pattern.trim().is_empty() {
        return Err("No siteswap pattern specified".to_string());
    }

    let title = params
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("title"))
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let bps = params
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("bps"))
        .and_then(|(_, value)| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(3.0);
    let dwell = params
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("dwell"))
        .and_then(|(_, value)| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0 && *value < 2.0)
        .unwrap_or(DWELL_DEFAULT);
    let squeezebeats = params
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("squeezebeats"))
        .and_then(|(_, value)| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(SQUEEZEBEATS_DEFAULT);
    let prop_name = params
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("prop"))
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "ball".to_string());
    let prop_diam = params
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("propdiam"))
        .and_then(|(_, value)| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(10.0);
    let colors = params
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("colors"))
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let hands = params
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("hands"))
        .map(|(_, value)| MhnHands::parse(value))
        .transpose()?;
    let body = params
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("body"))
        .map(|(_, value)| MhnBody::parse(value))
        .transpose()?;
    let hss = params
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("hss"))
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let hold = params
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("hold"))
        .map(|(_, value)| parse_bool(value))
        .transpose()?
        .unwrap_or(false);
    let dwellmax = params
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("dwellmax"))
        .map(|(_, value)| parse_bool(value))
        .transpose()?
        .unwrap_or(true);
    let handspec = params
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("handspec"))
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let dwell_array = if let Some(hss) = &hss {
        let converted = process_hss(&pattern, hss, hold, dwellmax, handspec.as_deref(), dwell)?;
        pattern = converted.converted_pattern;
        Some(converted.dwell_beats)
    } else {
        None
    };

    let parsed_pattern = parse_siteswap_pattern(&pattern)?;
    let beats = parsed_pattern.beats;
    let sync = parsed_pattern.sync;
    let jugglers = parsed_pattern.jugglers;
    let vanilla_async = parsed_pattern.vanilla_async;

    if beats.is_empty() {
        return Err("The siteswap pattern contains no readable throws".to_string());
    }

    let total: u32 = beats
        .iter()
        .flat_map(|beat| beat.throws.iter())
        .map(|throw| throw.value)
        .sum();
    let divisor = if sync { beats.len() * 2 } else { beats.len() };
    let balls = ((total as f64) / (divisor as f64)).round().max(1.0) as usize;
    let max_throw = beats
        .iter()
        .flat_map(|beat| beat.throws.iter())
        .map(|throw| throw.value)
        .max()
        .unwrap_or(3);

    Ok(SiteswapSpec {
        raw_config: config.to_string(),
        pattern,
        title,
        bps,
        dwell,
        squeezebeats,
        dwell_array,
        prop_name,
        prop_diam,
        colors,
        hands,
        body,
        jugglers,
        beats,
        sync,
        vanilla_async,
        balls,
        max_throw,
    })
}

pub fn display_title(spec: &SiteswapSpec) -> String {
    spec.title
        .clone()
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| spec.pattern.clone())
}

fn parse_params(config: &str) -> Vec<(String, String)> {
    config
        .split(';')
        .filter_map(|part| {
            let (key, value) = part.split_once('=')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

fn parse_bool(value: &str) -> Result<bool, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("Invalid boolean value: {value}")),
    }
}

#[derive(Clone, Debug, PartialEq)]
struct ParsedPattern {
    beats: Vec<Beat>,
    sync: bool,
    jugglers: usize,
    vanilla_async: bool,
}

fn parse_siteswap_pattern(pattern: &str) -> Result<ParsedPattern, String> {
    let expanded = expand_siteswap_repeats(&expand_repeats(pattern))?
        .trim()
        .to_string();
    if expanded.contains('<') {
        let expanded = expand_passing_star(&expanded)?;
        let (beats, jugglers) = parse_passing_pattern(&expanded)?;
        let vanilla_async = beats_are_vanilla_async(&beats);
        Ok(ParsedPattern {
            beats,
            sync: false,
            jugglers,
            vanilla_async,
        })
    } else if expanded.contains('(') {
        let expanded = expand_sync_star(&expanded)?;
        Ok(ParsedPattern {
            beats: parse_sync_pattern(&expanded)?,
            sync: true,
            jugglers: 1,
            vanilla_async: false,
        })
    } else {
        let beats = parse_async_pattern(&expanded)?;
        let vanilla_async = beats_are_vanilla_async(&beats);
        Ok(ParsedPattern {
            beats,
            sync: false,
            jugglers: 1,
            vanilla_async,
        })
    }
}

fn beats_are_vanilla_async(beats: &[Beat]) -> bool {
    beats.iter().all(|beat| {
        beat.throws
            .iter()
            .all(|throw| !throw.cross && !throw.hand_fixed)
    })
}

fn parse_async_pattern(pattern: &str) -> Result<Vec<Beat>, String> {
    let mut beats = Vec::new();
    let mut chars = pattern.chars().peekable();
    let mut beat_index = 0usize;

    while let Some(ch) = chars.next() {
        if ch.is_whitespace() || ch == ',' || ch == '.' {
            continue;
        }

        let hand = if beat_index % 2 == 0 {
            Hand::Right
        } else {
            Hand::Left
        };

        if ch == '[' {
            let mut throws = Vec::new();
            while let Some(inner) = chars.next() {
                if inner == ']' {
                    break;
                }
                if inner.is_whitespace() || inner == ',' {
                    continue;
                }
                throws.push(parse_throw_starting_with(
                    inner,
                    &mut chars,
                    ThrowContext::new(1, 1, hand, false),
                )?);
            }
            beats.push(Beat { throws });
            beat_index += 1;
        } else {
            beats.push(Beat {
                throws: vec![parse_throw_starting_with(
                    ch,
                    &mut chars,
                    ThrowContext::new(1, 1, hand, false),
                )?],
            });
            beat_index += 1;
        }
    }

    Ok(beats)
}

fn parse_sync_pattern(pattern: &str) -> Result<Vec<Beat>, String> {
    let mut beats = Vec::new();
    let mut chars = pattern.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '(' {
            continue;
        }

        let mut group = String::new();
        for inner in chars.by_ref() {
            if inner == ')' {
                break;
            }
            group.push(inner);
        }

        let parts: Vec<&str> = group.split(',').collect();
        if parts.len() < 2 {
            return Err(format!("Invalid synchronous group: ({group})"));
        }

        let mut throws = Vec::new();
        for (idx, part) in parts.iter().take(2).enumerate() {
            let hand = if idx == 0 { Hand::Left } else { Hand::Right };
            throws.extend(parse_sync_slot(part.trim(), hand)?);
        }
        beats.push(Beat { throws });
    }

    Ok(beats)
}

fn parse_sync_slot(slot: &str, hand: Hand) -> Result<Vec<ThrowSpec>, String> {
    if slot.is_empty() || slot == "-" {
        return Ok(vec![throw_spec(0, hand, false, 1, 1, None, true)]);
    }

    if slot.starts_with('[') && slot.ends_with(']') {
        let mut chars = slot[1..slot.len() - 1].chars().peekable();
        let mut throws = Vec::new();
        while let Some(ch) = chars.next() {
            if ch.is_whitespace() || ch == ',' {
                continue;
            }
            throws.push(parse_throw_starting_with(
                ch,
                &mut chars,
                ThrowContext::new(1, 1, hand, true),
            )?);
        }
        return Ok(throws);
    }

    let mut chars = slot.chars().peekable();
    let first = chars
        .next()
        .ok_or_else(|| "Empty synchronous slot".to_string())?;
    Ok(vec![parse_throw_starting_with(
        first,
        &mut chars,
        ThrowContext::new(1, 1, hand, true),
    )?])
}

#[derive(Clone, Copy, Debug)]
struct ThrowContext {
    source_juggler: usize,
    default_target_juggler: usize,
    hand: Hand,
    hand_fixed: bool,
}

impl ThrowContext {
    fn new(
        source_juggler: usize,
        default_target_juggler: usize,
        hand: Hand,
        hand_fixed: bool,
    ) -> Self {
        Self {
            source_juggler,
            default_target_juggler,
            hand,
            hand_fixed,
        }
    }
}

fn parse_throw_starting_with<I>(
    first: char,
    chars: &mut std::iter::Peekable<I>,
    context: ThrowContext,
) -> Result<ThrowSpec, String>
where
    I: Iterator<Item = char>,
{
    let value = if first == '{' {
        let mut number = String::new();
        let mut closed = false;
        for ch in chars.by_ref() {
            if ch == '}' {
                closed = true;
                break;
            }
            number.push(ch);
        }
        if !closed {
            return Err("Invalid braced siteswap throw: missing '}'".to_string());
        }
        number
            .trim()
            .parse::<u32>()
            .map_err(|_| format!("Invalid braced siteswap throw: {{{number}}}"))?
    } else {
        throw_value(first).ok_or_else(|| format!("Invalid siteswap throw: {first}"))?
    };
    let mut cross = false;
    let mut target_juggler = context.default_target_juggler;
    let mut modifier = None;

    while matches!(chars.peek(), Some('x') | Some('X')) {
        chars.next();
        cross = !cross;
    }

    if matches!(chars.peek(), Some('p') | Some('P')) {
        chars.next();
        target_juggler = context.source_juggler + 1;
        let mut digits = String::new();
        while chars.peek().is_some_and(|ch| ch.is_ascii_digit()) {
            digits.push(chars.next().expect("peeked digit exists"));
        }
        if !digits.is_empty() {
            target_juggler = digits
                .parse::<usize>()
                .map_err(|_| format!("Invalid passing target: p{digits}"))?;
        }
    }

    if chars.peek() == Some(&'/') {
        chars.next();
    }

    let mut modifier_text = String::new();
    while chars
        .peek()
        .is_some_and(|ch| matches!(ch, 'B' | 'H' | 'F' | 'L' | 'T'))
    {
        modifier_text.push(chars.next().expect("peeked modifier exists"));
    }
    if !modifier_text.is_empty() {
        modifier = Some(modifier_text);
    }

    Ok(throw_spec(
        value,
        context.hand,
        cross,
        context.source_juggler,
        target_juggler,
        modifier,
        context.hand_fixed,
    ))
}

fn throw_spec(
    value: u32,
    hand: Hand,
    cross: bool,
    source_juggler: usize,
    target_juggler: usize,
    modifier: Option<String>,
    hand_fixed: bool,
) -> ThrowSpec {
    ThrowSpec {
        value,
        hand,
        cross,
        source_juggler,
        target_juggler,
        modifier,
        hand_fixed,
    }
}

fn parse_passing_pattern(pattern: &str) -> Result<(Vec<Beat>, usize), String> {
    let groups = passing_groups(pattern)?;
    if groups.is_empty() {
        return Err("The passing siteswap contains no groups".to_string());
    }

    let mut beats = Vec::<Beat>::new();
    let mut jugglers = None;
    let mut right_on_even = Vec::<bool>::new();
    let mut hand_fixed = Vec::<bool>::new();
    let mut current_beat = 0usize;

    for group in groups {
        let parts = split_top_level(&group, '|');
        if parts.is_empty() {
            continue;
        }

        let group_jugglers = parts.len();
        if let Some(expected) = jugglers {
            if expected != group_jugglers {
                return Err("Inconsistent number of jugglers in passing pattern".to_string());
            }
        } else {
            jugglers = Some(group_jugglers);
            right_on_even = vec![true; group_jugglers];
            hand_fixed = vec![false; group_jugglers];
        }

        let mut parsed_parts = Vec::with_capacity(group_jugglers);
        let mut group_beats = None;
        for (juggler_index, part) in parts.iter().enumerate() {
            let parsed = parse_passing_throws(
                part,
                juggler_index + 1,
                current_beat,
                &mut right_on_even,
                &mut hand_fixed,
            )?;
            if let Some(expected) = group_beats {
                if parsed.beats != expected {
                    return Err("Inconsistent number of beats between jugglers".to_string());
                }
            } else {
                group_beats = Some(parsed.beats);
            }
            parsed_parts.push(parsed);
        }

        let group_beats = group_beats.unwrap_or(0);
        ensure_beat_capacity(&mut beats, current_beat + group_beats);
        for parsed in parsed_parts {
            for entry in parsed.entries {
                let beat_index = current_beat + entry.beat_offset;
                ensure_beat_capacity(&mut beats, beat_index + 1);
                beats[beat_index].throws.extend(entry.throws);
            }
        }
        current_beat += group_beats;
    }

    Ok((beats, jugglers.unwrap_or(1)))
}

#[derive(Clone, Debug, PartialEq)]
struct ParsedPassingThrows {
    beats: usize,
    entries: Vec<PassingEntry>,
}

#[derive(Clone, Debug, PartialEq)]
struct PassingEntry {
    beat_offset: usize,
    throws: Vec<ThrowSpec>,
}

fn parse_passing_throws(
    part: &str,
    source_juggler: usize,
    global_beat: usize,
    right_on_even: &mut [bool],
    hand_fixed: &mut [bool],
) -> Result<ParsedPassingThrows, String> {
    let chars = part.chars().collect::<Vec<_>>();
    let mut entries = Vec::new();
    let mut beat_sub = 0usize;
    let mut pos = 0usize;

    while pos < chars.len() {
        match chars[pos] {
            ch if ch.is_whitespace() || ch == ',' || ch == '.' => {
                pos += 1;
            }
            'R' | 'L' => {
                let spec_left = matches!(chars[pos], 'L' | 'l');
                let index = source_juggler - 1;
                if (global_beat + beat_sub) % 2 == 0 {
                    right_on_even[index] = !spec_left;
                } else {
                    right_on_even[index] = spec_left;
                }
                hand_fixed[index] = true;
                pos += 1;
            }
            '(' => {
                let close = matching_delimiter(&chars, pos, '(', ')')
                    .ok_or_else(|| "Invalid passing synchronous group: missing ')'".to_string())?;
                let group = chars[pos + 1..close].iter().collect::<String>();
                let parts = split_top_level(&group, ',');
                if parts.len() < 2 {
                    return Err(format!("Invalid passing synchronous group: ({group})"));
                }
                let mut throws = Vec::new();
                throws.extend(parse_throw_slot_for_passing(
                    parts[0].trim(),
                    source_juggler,
                    Hand::Left,
                    true,
                )?);
                throws.extend(parse_throw_slot_for_passing(
                    parts[1].trim(),
                    source_juggler,
                    Hand::Right,
                    true,
                )?);
                entries.push(PassingEntry {
                    beat_offset: beat_sub,
                    throws,
                });
                pos = close + 1;
                if chars.get(pos) == Some(&'!') {
                    beat_sub += 1;
                    pos += 1;
                } else {
                    beat_sub += 2;
                }
            }
            '[' => {
                let close = matching_delimiter(&chars, pos, '[', ']')
                    .ok_or_else(|| "Invalid passing multiplex: missing ']'".to_string())?;
                let group = chars[pos + 1..close].iter().collect::<String>();
                let hand =
                    hand_for_async_beat(global_beat + beat_sub, right_on_even[source_juggler - 1]);
                let throws = parse_throw_slot_for_passing(
                    &format!("[{group}]"),
                    source_juggler,
                    hand,
                    hand_fixed[source_juggler - 1],
                )?;
                entries.push(PassingEntry {
                    beat_offset: beat_sub,
                    throws,
                });
                beat_sub += 1;
                pos = close + 1;
            }
            ']' | ')' | '>' | '|' => {
                return Err(format!(
                    "Unexpected character in passing siteswap: {}",
                    chars[pos]
                ));
            }
            ch => {
                let hand =
                    hand_for_async_beat(global_beat + beat_sub, right_on_even[source_juggler - 1]);
                let mut tail = chars[pos + 1..].iter().copied().peekable();
                let throw = parse_throw_starting_with(
                    ch,
                    &mut tail,
                    ThrowContext::new(
                        source_juggler,
                        source_juggler,
                        hand,
                        hand_fixed[source_juggler - 1],
                    ),
                )?;
                let consumed_tail = chars[pos + 1..].len() - tail.clone().count();
                entries.push(PassingEntry {
                    beat_offset: beat_sub,
                    throws: vec![throw],
                });
                beat_sub += 1;
                pos += 1 + consumed_tail;
            }
        }
    }

    Ok(ParsedPassingThrows {
        beats: beat_sub,
        entries,
    })
}

fn parse_throw_slot_for_passing(
    slot: &str,
    source_juggler: usize,
    hand: Hand,
    hand_fixed: bool,
) -> Result<Vec<ThrowSpec>, String> {
    if slot.is_empty() || slot == "-" {
        return Ok(vec![throw_spec(
            0,
            hand,
            false,
            source_juggler,
            source_juggler,
            None,
            true,
        )]);
    }

    let inner = slot
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'));
    let mut chars = inner.unwrap_or(slot).chars().peekable();
    let mut throws = Vec::new();
    while let Some(ch) = chars.next() {
        if ch.is_whitespace() || ch == ',' {
            continue;
        }
        throws.push(parse_throw_starting_with(
            ch,
            &mut chars,
            ThrowContext::new(source_juggler, source_juggler, hand, hand_fixed),
        )?);
    }
    Ok(throws)
}

fn passing_groups(pattern: &str) -> Result<Vec<String>, String> {
    let chars = pattern.chars().collect::<Vec<_>>();
    let mut groups = Vec::new();
    let mut pos = 0usize;
    while pos < chars.len() {
        match chars[pos] {
            ch if ch.is_whitespace() || ch == ',' || ch == '.' || ch == '*' => pos += 1,
            '<' => {
                let close = matching_delimiter(&chars, pos, '<', '>')
                    .ok_or_else(|| "Invalid passing group: missing '>'".to_string())?;
                groups.push(chars[pos + 1..close].iter().collect());
                pos = close + 1;
            }
            '(' => {
                let close = matching_delimiter(&chars, pos, '(', ')')
                    .ok_or_else(|| "Invalid grouped passing pattern: missing ')'".to_string())?;
                let inner = chars[pos + 1..close].iter().collect::<String>();
                groups.extend(passing_groups(&inner)?);
                pos = close + 1;
            }
            ch => return Err(format!("Unexpected character in passing siteswap: {ch}")),
        }
    }
    Ok(groups)
}

fn expand_passing_star(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    let Some(base) = trimmed.strip_suffix('*') else {
        return Ok(trimmed.to_string());
    };

    let base = base.trim();
    let mirrored = passing_groups(base)?
        .into_iter()
        .map(|group| {
            mirror_passing_group(&group).map(|mirrored_group| format!("<{mirrored_group}>"))
        })
        .collect::<Result<String, String>>()?;

    Ok(format!("{base}{mirrored}"))
}

fn mirror_passing_group(group: &str) -> Result<String, String> {
    split_top_level(group, '|')
        .into_iter()
        .map(|part| mirror_passing_part(&part))
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.join("|"))
}

fn mirror_passing_part(part: &str) -> Result<String, String> {
    let chars = part.chars().collect::<Vec<_>>();
    let mut output = String::new();
    let mut pos = 0usize;

    while pos < chars.len() {
        match chars[pos] {
            '(' => {
                let close = matching_delimiter(&chars, pos, '(', ')')
                    .ok_or_else(|| "Invalid passing synchronous group: missing ')'".to_string())?;
                let group = chars[pos + 1..close].iter().collect::<String>();
                let slots = split_top_level(&group, ',');
                if slots.len() < 2 {
                    return Err(format!("Invalid passing synchronous group: ({group})"));
                }

                output.push('(');
                output.push_str(slots[1].trim());
                output.push(',');
                output.push_str(slots[0].trim());
                for extra in slots.iter().skip(2) {
                    output.push(',');
                    output.push_str(extra.trim());
                }
                output.push(')');
                pos = close + 1;
            }
            'R' => {
                output.push('L');
                pos += 1;
            }
            'L' => {
                output.push('R');
                pos += 1;
            }
            'r' => {
                output.push('l');
                pos += 1;
            }
            'l' => {
                output.push('r');
                pos += 1;
            }
            ch => {
                output.push(ch);
                pos += 1;
            }
        }
    }

    Ok(output)
}

fn expand_siteswap_repeats(input: &str) -> Result<String, String> {
    let chars = input.chars().collect::<Vec<_>>();
    expand_siteswap_repeats_in_chars(&chars)
}

fn expand_siteswap_repeats_in_chars(chars: &[char]) -> Result<String, String> {
    let mut output = String::new();
    let mut pos = 0usize;

    while pos < chars.len() {
        if chars[pos] != '(' {
            output.push(chars[pos]);
            pos += 1;
            continue;
        }

        let Some(close) = matching_delimiter(chars, pos, '(', ')') else {
            return Err("Invalid grouped pattern: missing ')'".to_string());
        };
        let inner = &chars[pos + 1..close];
        if let Some((body_end, repeats)) = grouped_repeat(inner) {
            let body = expand_siteswap_repeats_in_chars(&inner[..body_end])?;
            for _ in 0..repeats {
                output.push_str(&body);
            }
        } else {
            output.push('(');
            output.push_str(&expand_siteswap_repeats_in_chars(inner)?);
            output.push(')');
        }
        pos = close + 1;
    }

    Ok(output)
}

fn grouped_repeat(chars: &[char]) -> Option<(usize, usize)> {
    let mut pos = chars.len();
    while pos > 0 && chars[pos - 1].is_whitespace() {
        pos -= 1;
    }
    let digits_end = pos;
    while pos > 0 && chars[pos - 1].is_ascii_digit() {
        pos -= 1;
    }
    if pos == digits_end || pos == 0 || chars[pos - 1] != '^' {
        return None;
    }
    let repeats = chars[pos..digits_end]
        .iter()
        .collect::<String>()
        .parse::<usize>()
        .ok()?;
    let mut body_end = pos - 1;
    while body_end > 0 && chars[body_end - 1].is_whitespace() {
        body_end -= 1;
    }
    Some((body_end, repeats))
}

fn expand_sync_star(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    let Some(base) = trimmed.strip_suffix('*') else {
        return Ok(trimmed.to_string());
    };
    let mirrored = mirror_sync_groups(base.trim())?;
    Ok(format!("{}{mirrored}", base.trim()))
}

fn mirror_sync_groups(input: &str) -> Result<String, String> {
    let chars = input.chars().collect::<Vec<_>>();
    let mut output = String::new();
    let mut pos = 0usize;

    while pos < chars.len() {
        if chars[pos].is_whitespace() {
            pos += 1;
            continue;
        }
        if chars[pos] != '(' {
            return Err(format!(
                "Invalid synchronous starred pattern near '{}'",
                chars[pos]
            ));
        }

        let close = matching_delimiter(&chars, pos, '(', ')')
            .ok_or_else(|| "Invalid synchronous starred pattern: missing ')'".to_string())?;
        let group = chars[pos + 1..close].iter().collect::<String>();
        let parts = split_top_level(&group, ',');
        if parts.len() < 2 {
            return Err(format!("Invalid synchronous group: ({group})"));
        }
        output.push('(');
        output.push_str(parts[1].trim());
        output.push(',');
        output.push_str(parts[0].trim());
        output.push(')');
        pos = close + 1;
    }

    Ok(output)
}

fn split_top_level(input: &str, delimiter: char) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut paren = 0i32;
    let mut bracket = 0i32;
    let mut brace = 0i32;
    let mut angle = 0i32;

    for ch in input.chars() {
        match ch {
            '(' => {
                paren += 1;
                current.push(ch);
            }
            ')' => {
                paren -= 1;
                current.push(ch);
            }
            '[' => {
                bracket += 1;
                current.push(ch);
            }
            ']' => {
                bracket -= 1;
                current.push(ch);
            }
            '{' => {
                brace += 1;
                current.push(ch);
            }
            '}' => {
                brace -= 1;
                current.push(ch);
            }
            '<' => {
                angle += 1;
                current.push(ch);
            }
            '>' => {
                angle -= 1;
                current.push(ch);
            }
            _ if ch == delimiter && paren == 0 && bracket == 0 && brace == 0 && angle == 0 => {
                result.push(current.trim().to_string());
                current = String::new();
            }
            _ => current.push(ch),
        }
    }

    result.push(current.trim().to_string());
    result
}

fn matching_delimiter(chars: &[char], open_pos: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    for (pos, ch) in chars.iter().enumerate().skip(open_pos) {
        if *ch == open {
            depth += 1;
        } else if *ch == close {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(pos);
            }
        }
    }
    None
}

fn ensure_beat_capacity(beats: &mut Vec<Beat>, len: usize) {
    while beats.len() < len {
        beats.push(Beat { throws: Vec::new() });
    }
}

fn hand_for_async_beat(beat: usize, right_on_even: bool) -> Hand {
    let right = if beat % 2 == 0 {
        right_on_even
    } else {
        !right_on_even
    };
    if right { Hand::Right } else { Hand::Left }
}

fn throw_value(ch: char) -> Option<u32> {
    if ch.is_ascii_digit() {
        ch.to_digit(10)
    } else if ch.is_ascii_alphabetic() {
        Some(ch.to_ascii_lowercase() as u32 - 'a' as u32 + 10)
    } else {
        None
    }
}

pub fn target_hand(spec: &SiteswapSpec, start: Hand, value: u32, cross: bool) -> Hand {
    let toggles = if spec.sync {
        cross
    } else {
        (value % 2 == 1) ^ cross
    };
    if toggles {
        match start {
            Hand::Left => Hand::Right,
            Hand::Right => Hand::Left,
        }
    } else {
        start
    }
}

pub fn beat_duration(spec: &SiteswapSpec, throw_value: u32) -> f64 {
    let beats = if spec.sync {
        (throw_value as f64 / 2.0).max(0.25)
    } else {
        (throw_value as f64).max(0.25)
    };
    beats / spec.bps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_async_siteswap() {
        let spec = parse_config("pattern=531;bps=4.0").unwrap();
        assert_eq!(spec.pattern, "531");
        assert_eq!(spec.beats.len(), 3);
        assert_eq!(spec.balls, 3);
        assert_eq!(spec.max_throw, 5);
        assert!((spec.bps - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parses_sync_siteswap() {
        let spec = parse_config("pattern=(4x,2)(2,4x);title=Chops").unwrap();
        assert!(spec.sync);
        assert_eq!(spec.beats.len(), 2);
        assert_eq!(display_title(&spec), "Chops");
    }

    #[test]
    fn parses_synchronous_star_as_mirrored_half() {
        let spec = parse_config("pattern=(2,6x)(2x,6)*;colors=orbits").unwrap();
        assert!(spec.sync);
        assert_eq!(spec.beats.len(), 4);
        assert_eq!(spec.balls, 4);
        assert_eq!(target_hand(&spec, Hand::Right, 6, true), Hand::Left);
    }

    #[test]
    fn parses_hands_specifier() {
        let spec = parse_config("pattern=3;hands=(32.5)(10).(10)(32.5).").unwrap();
        let hands = spec.hands.unwrap();
        assert_eq!(hands.get_period(1), 2);
        assert_eq!(hands.to_jugglinglab_string(), "(32.5)(10).(10)(32.5).");
    }

    #[test]
    fn parses_body_specifier() {
        let spec = parse_config("pattern=3;body=(0)...(90)...").unwrap();
        let body = spec.body.unwrap();
        assert_eq!(body.get_period(1), 6);
    }

    #[test]
    fn parses_prop_parameters() {
        let spec = parse_config("pattern=3;prop=square;propdiam=12;colors=mixed").unwrap();
        assert_eq!(spec.prop_name, "square");
        assert_eq!(spec.prop_diam, 12.0);
        assert_eq!(spec.colors.as_deref(), Some("mixed"));
    }

    #[test]
    fn parses_two_juggler_passing_group() {
        let spec = parse_config("pattern=<2|0>;title=PUNCH").unwrap();
        assert!(!spec.sync);
        assert_eq!(spec.jugglers, 2);
        assert_eq!(spec.beats.len(), 1);
        assert_eq!(spec.balls, 2);
        assert_eq!(display_title(&spec), "PUNCH");

        let first = &spec.beats[0].throws[0];
        assert_eq!(first.source_juggler, 1);
        assert_eq!(first.target_juggler, 1);
        assert_eq!(first.value, 2);

        let second = &spec.beats[0].throws[1];
        assert_eq!(second.source_juggler, 2);
        assert_eq!(second.value, 0);
    }

    #[test]
    fn parses_passing_targets_and_bounce_modifiers() {
        let spec = parse_config("pattern=<3p|3p2><3BHL|0>").unwrap();
        assert_eq!(spec.jugglers, 2);
        assert_eq!(spec.beats.len(), 2);
        assert_eq!(spec.beats[0].throws[0].target_juggler, 2);
        assert_eq!(spec.beats[0].throws[1].target_juggler, 2);
        assert_eq!(spec.beats[1].throws[0].modifier.as_deref(), Some("BHL"));
    }

    #[test]
    fn parses_bounce_and_braced_throws() {
        let bounce = parse_config("pattern=3BHL").unwrap();
        assert_eq!(bounce.beats.len(), 1);
        assert_eq!(bounce.beats[0].throws[0].modifier.as_deref(), Some("BHL"));

        let braced = parse_config("pattern={666};bps=600").unwrap();
        assert_eq!(braced.max_throw, 666);
    }

    #[test]
    fn converts_hss_before_siteswap_parsing() {
        let spec = parse_config("pattern=3773;hss=2266;handspec=(1,2)(3,4)").unwrap();
        assert_eq!(spec.jugglers, 2);
        assert!(spec.pattern.contains('<'));
        assert!(spec.pattern.contains("p2"));
        assert_eq!(spec.dwell_array.as_ref().unwrap().len(), 8);
    }

    #[test]
    fn parses_builtin_hss_records() {
        let failures = crate::library::builtin_records()
            .into_iter()
            .filter_map(|record| {
                let config = record.config?;
                if !record
                    .notation
                    .as_deref()
                    .is_some_and(|notation| notation.eq_ignore_ascii_case("siteswap"))
                    || !config.contains("hss=")
                {
                    return None;
                }
                parse_config(&config)
                    .err()
                    .map(|err| format!("{}: {err}", record.display))
            })
            .collect::<Vec<_>>();

        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn parses_builtin_siteswap_records() {
        let failures = crate::library::builtin_records()
            .into_iter()
            .filter_map(|record| {
                let config = record.config?;
                if !record
                    .notation
                    .as_deref()
                    .is_some_and(|notation| notation.eq_ignore_ascii_case("siteswap"))
                {
                    return None;
                }
                parse_config(&config)
                    .err()
                    .map(|err| format!("{}: {err}", record.display))
            })
            .collect::<Vec<_>>();

        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}

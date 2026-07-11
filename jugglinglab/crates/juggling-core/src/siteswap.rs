use crate::hand_siteswap::process_hss;
use crate::mhn_body::MhnBody;
use crate::mhn_hands::MhnHands;

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
    pub sync: bool,
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
    let divisor = beats.len();
    if total as usize % divisor != 0 {
        return Err("The siteswap pattern does not have an integer average".to_string());
    }
    let balls = total as usize / divisor;
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
    let chars = pattern.trim().chars().collect::<Vec<_>>();
    let mut state = GrammarState::default();
    let beats = parse_pattern_fragment(&chars, 0, &mut state)?;
    let jugglers = state.jugglers.unwrap_or(1);
    let sync = state.saw_sync && !state.saw_async;
    let vanilla_async = !state.explicit_switch && !sync && beats_are_vanilla_async(&beats);
    Ok(ParsedPattern {
        beats,
        sync,
        jugglers,
        vanilla_async,
    })
}

#[derive(Clone, Debug, Default)]
struct GrammarState {
    jugglers: Option<usize>,
    right_on_even: Vec<bool>,
    hand_fixed: Vec<bool>,
    saw_sync: bool,
    saw_async: bool,
    explicit_switch: bool,
}

impl GrammarState {
    fn require_jugglers(&mut self, jugglers: usize) -> Result<(), String> {
        if let Some(expected) = self.jugglers {
            if expected != jugglers {
                return Err("Inconsistent number of jugglers".to_string());
            }
        } else {
            self.jugglers = Some(jugglers);
            self.right_on_even = vec![true; jugglers];
            self.hand_fixed = vec![false; jugglers];
        }
        Ok(())
    }
}

fn parse_pattern_fragment(
    chars: &[char],
    base_beat: usize,
    state: &mut GrammarState,
) -> Result<Vec<Beat>, String> {
    let mut beats = Vec::<Beat>::new();
    let mut pos = 0usize;
    while pos < chars.len() {
        match chars[pos] {
            ch if ch.is_whitespace() => pos += 1,
            '*' => {
                if chars[pos + 1..].iter().any(|ch| !ch.is_whitespace()) {
                    return Err("Switch-repeat '*' must terminate its pattern".to_string());
                }
                let mut switched = beats.clone();
                for beat in &mut switched {
                    for throw in &mut beat.throws {
                        throw.hand = opposite_siteswap_hand(throw.hand);
                        throw.hand_fixed = true;
                    }
                }
                beats.extend(switched);
                state.explicit_switch = true;
                break;
            }
            '?' => {
                return Err("Wildcard transitions are not implemented by Juggling Lab".to_string());
            }
            '<' => {
                let close = matching_delimiter(chars, pos, '<', '>')
                    .ok_or_else(|| "Invalid passing group: missing '>'".to_string())?;
                let group = chars[pos + 1..close].iter().collect::<String>();
                let group_beats =
                    parse_general_passing_group(&group, base_beat + beats.len(), state)?;
                beats.extend(group_beats);
                pos = close + 1;
            }
            '(' => {
                let close = matching_delimiter(chars, pos, '(', ')')
                    .ok_or_else(|| "Invalid grouped pattern: missing ')'".to_string())?;
                let inner = &chars[pos + 1..close];
                if let Some((body_end, repeats)) = grouped_repeat(inner) {
                    for _ in 0..repeats {
                        let nested = parse_pattern_fragment(
                            &inner[..body_end],
                            base_beat + beats.len(),
                            state,
                        )?;
                        beats.extend(nested);
                    }
                    pos = close + 1;
                } else {
                    state.require_jugglers(1)?;
                    let group = inner.iter().collect::<String>();
                    let slots = split_top_level(&group, ',');
                    if slots.len() != 2 {
                        return Err(format!("Invalid synchronous group: ({group})"));
                    }
                    let mut throws = parse_sync_slot(slots[0].trim(), Hand::Left)?;
                    throws.extend(parse_sync_slot(slots[1].trim(), Hand::Right)?);
                    state.saw_sync = true;
                    beats.push(Beat { throws });
                    pos = close + 1;
                    if chars.get(pos) == Some(&'!') {
                        pos += 1;
                    } else {
                        beats.push(Beat { throws: Vec::new() });
                    }
                }
            }
            '[' => {
                state.require_jugglers(1)?;
                let close = matching_delimiter(chars, pos, '[', ']')
                    .ok_or_else(|| "Invalid multiplex: missing ']'".to_string())?;
                let beat_number = base_beat + beats.len();
                let hand = hand_for_async_beat(beat_number, state.right_on_even[0]);
                let context = ThrowContext::new(1, 1, hand, state.hand_fixed[0]);
                let throws = parse_multiplex_slice(&chars[pos + 1..close], context)?;
                state.saw_async = true;
                beats.push(Beat { throws });
                pos = close + 1;
            }
            'L' | 'R' => {
                state.require_jugglers(1)?;
                let beat_number = base_beat + beats.len();
                let left = chars[pos] == 'L';
                state.right_on_even[0] = if beat_number % 2 == 0 { !left } else { left };
                state.hand_fixed[0] = true;
                pos += 1;
            }
            ch if is_throw_value_start(ch) => {
                state.require_jugglers(1)?;
                let beat_number = base_beat + beats.len();
                let hand = hand_for_async_beat(beat_number, state.right_on_even[0]);
                let context = ThrowContext::new(1, 1, hand, state.hand_fixed[0]);
                let throw = parse_throw_from_slice(chars, &mut pos, context)?;
                state.saw_async = true;
                beats.push(Beat {
                    throws: vec![throw],
                });
            }
            ch => return Err(format!("Unexpected character in siteswap: {ch}")),
        }
    }
    Ok(beats)
}

fn parse_general_passing_group(
    group: &str,
    global_beat: usize,
    state: &mut GrammarState,
) -> Result<Vec<Beat>, String> {
    let parts = split_top_level(group, '|');
    if parts.is_empty() {
        return Err("Passing group contains no jugglers".to_string());
    }
    state.require_jugglers(parts.len())?;
    let mut parsed_parts = Vec::with_capacity(parts.len());
    let mut group_beats = None;
    for (juggler_index, part) in parts.iter().enumerate() {
        let parsed = parse_passing_throws(
            part,
            juggler_index + 1,
            global_beat,
            &mut state.right_on_even,
            &mut state.hand_fixed,
        )?;
        if group_beats.is_some_and(|expected| expected != parsed.beats) {
            return Err("Inconsistent number of beats between jugglers".to_string());
        }
        group_beats = Some(parsed.beats);
        parsed_parts.push(parsed);
    }

    let mut beats = vec![Beat { throws: Vec::new() }; group_beats.unwrap_or(0)];
    for parsed in parsed_parts {
        for entry in parsed.entries {
            if let Some(beat) = beats.get_mut(entry.beat_offset) {
                beat.throws.extend(entry.throws);
            }
        }
    }
    for throw in beats.iter().flat_map(|beat| &beat.throws) {
        if throw.sync {
            state.saw_sync = true;
        } else {
            state.saw_async = true;
        }
    }
    Ok(beats)
}

fn parse_multiplex_slice(chars: &[char], context: ThrowContext) -> Result<Vec<ThrowSpec>, String> {
    let mut throws = Vec::new();
    let mut pos = 0usize;
    while pos < chars.len() {
        if chars[pos].is_whitespace() {
            pos += 1;
            continue;
        }
        if !is_throw_value_start(chars[pos]) {
            return Err(format!("Unexpected character in multiplex: {}", chars[pos]));
        }
        throws.push(parse_throw_from_slice(chars, &mut pos, context)?);
    }
    if throws.is_empty() {
        return Err("A multiplex must contain at least one throw".to_string());
    }
    Ok(throws)
}

fn parse_throw_from_slice(
    chars: &[char],
    pos: &mut usize,
    context: ThrowContext,
) -> Result<ThrowSpec, String> {
    let first = chars[*pos];
    let mut tail = chars[*pos + 1..].iter().copied().peekable();
    let parsed = parse_throw_starting_with(first, &mut tail, context)?;
    let consumed_tail = chars[*pos + 1..].len() - tail.count();
    *pos += consumed_tail + 1;
    Ok(parsed)
}

fn is_throw_value_start(ch: char) -> bool {
    ch.is_ascii_digit()
        || ch == '{'
        || ch == 'x'
        || ch == 'p'
        || matches!(ch, 'a'..='q' | 'r'..='w' | 'y' | 'z')
}

fn opposite_siteswap_hand(hand: Hand) -> Hand {
    match hand {
        Hand::Left => Hand::Right,
        Hand::Right => Hand::Left,
    }
}

fn beats_are_vanilla_async(beats: &[Beat]) -> bool {
    beats.iter().all(|beat| {
        beat.throws
            .iter()
            .all(|throw| !throw.cross && !throw.hand_fixed)
    })
}

fn parse_sync_slot(slot: &str, hand: Hand) -> Result<Vec<ThrowSpec>, String> {
    if slot.is_empty() || slot == "-" {
        let mut zero = throw_spec(0, hand, false, 1, 1, None, true);
        zero.sync = true;
        return Ok(vec![zero]);
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
                ThrowContext::new(1, 1, hand, true).with_sync(true),
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
        ThrowContext::new(1, 1, hand, true).with_sync(true),
    )?])
}

#[derive(Clone, Copy, Debug)]
struct ThrowContext {
    source_juggler: usize,
    default_target_juggler: usize,
    hand: Hand,
    hand_fixed: bool,
    sync: bool,
    allow_pass: bool,
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
            sync: false,
            allow_pass: false,
        }
    }

    fn with_sync(mut self, sync: bool) -> Self {
        self.sync = sync;
        self
    }

    fn with_passing(mut self) -> Self {
        self.allow_pass = true;
        self
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

    if chars.peek() == Some(&'x') {
        chars.next();
        cross = true;
    }

    if context.allow_pass && chars.peek() == Some(&'p') {
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

    let mut modifier_text = String::new();
    if chars.peek().is_some_and(|ch| is_modifier_start(*ch)) {
        modifier_text.push(chars.next().expect("peeked modifier exists"));
        while chars.peek().is_some_and(|ch| ch.is_ascii_uppercase()) {
            modifier_text.push(chars.next().expect("peeked modifier exists"));
        }
    }
    if !modifier_text.is_empty() {
        modifier = Some(modifier_text);
    }
    if chars.peek() == Some(&'/') {
        chars.next();
    }

    let mut parsed = throw_spec(
        value,
        context.hand,
        cross,
        context.source_juggler,
        target_juggler,
        modifier,
        context.hand_fixed,
    );
    parsed.sync = context.sync;
    Ok(parsed)
}

fn is_modifier_start(ch: char) -> bool {
    ch.is_ascii_uppercase() && !matches!(ch, 'L' | 'R')
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
        sync: false,
    }
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
                    true,
                )?);
                throws.extend(parse_throw_slot_for_passing(
                    parts[1].trim(),
                    source_juggler,
                    Hand::Right,
                    true,
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
                    false,
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
                    )
                    .with_passing(),
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
    sync: bool,
) -> Result<Vec<ThrowSpec>, String> {
    if slot.is_empty() || slot == "-" {
        let mut zero = throw_spec(0, hand, false, source_juggler, source_juggler, None, true);
        zero.sync = sync;
        return Ok(vec![zero]);
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
            ThrowContext::new(source_juggler, source_juggler, hand, hand_fixed)
                .with_sync(sync)
                .with_passing(),
        )?);
    }
    Ok(throws)
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
    } else if ch.is_ascii_lowercase() {
        Some(ch as u32 - 'a' as u32 + 10)
    } else {
        None
    }
}

pub fn target_hand(spec: &SiteswapSpec, start: Hand, value: u32, cross: bool) -> Hand {
    let _ = spec;
    let toggles = (value % 2 == 1) ^ cross;
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
        assert_eq!(spec.beats.len(), 4);
        assert_eq!(display_title(&spec), "Chops");
    }

    #[test]
    fn parses_synchronous_star_as_mirrored_half() {
        let spec = parse_config("pattern=(2,6x)(2x,6)*;colors=orbits").unwrap();
        assert!(spec.sync);
        assert_eq!(spec.beats.len(), 8);
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
        let spec = parse_config("pattern=<3p|3p2><3BHL|1>").unwrap();
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

    #[test]
    fn matches_original_siteswap_pattern_grammar_cases() {
        use crate::layout::LaidoutPattern;
        use crate::mhn_matrix::MhnMatrix;

        let cases = [
            ("868671", 1, 6),
            ("(4,3x)!(2,0)!(3x,0)!", 1, 4),
            ("(0,6x)!(0,0)!(6x,0)!(0,0)!", 1, 3),
            ("4x1(4x,3x)*", 1, 3),
            ("([42],4x)*", 1, 5),
            ("(645^2)65x6x1x((6x,4)*^2)(7,5x)(4,1x)!", 1, 5),
            (
                "<([2xp/2x],[2xp/2])|(2,[2/2xp])><(2,[2p/2])|([2/2p],[2/2p])>",
                2,
                7,
            ),
            ("{49}1", 1, 25),
            ("3BB", 1, 3),
            ("R3R3xL3L3x", 1, 3),
            ("<R|L><4xp|3><3|4xp>", 2, 7),
            ("(4,5x)(4,1x)!R5x41x", 1, 4),
            ("0", 1, 0),
            ("<0|0>", 2, 0),
            ("[53] 22", 1, 4),
            ("[5 3  ] 2 2 ", 1, 4),
            (" (2,4x) ([4x 4] , 2) ", 1, 4),
            ("{5}{1}", 1, 3),
            ("5{1}", 1, 3),
            ("{5}1{5}1", 1, 3),
        ];

        for (pattern, jugglers, paths) in cases {
            let spec = parse_config(pattern).unwrap_or_else(|err| panic!("{pattern}: {err}"));
            assert_eq!(spec.jugglers, jugglers, "{pattern}");
            assert_eq!(spec.balls, paths, "{pattern}");
            let mut matrix = MhnMatrix::from_siteswap(&spec)
                .unwrap_or_else(|err| panic!("{pattern}: {err}\n{:#?}", spec.beats));
            let model = matrix
                .to_jml_pattern(&spec)
                .unwrap_or_else(|err| panic!("{pattern}: {err}"));
            LaidoutPattern::from_jml_pattern(&model)
                .unwrap_or_else(|err| panic!("{pattern}: {err}"));
        }
    }

    #[test]
    fn rejects_invalid_generalized_siteswap_syntax() {
        for pattern in [
            "",
            "(4,4",
            "[53",
            "<3|3",
            "<3|3><3|3|3>",
            "<3|33>",
            "3*3",
            "Q3",
            "52",
            "?",
        ] {
            assert!(parse_config(pattern).is_err(), "{pattern}");
        }
    }
}

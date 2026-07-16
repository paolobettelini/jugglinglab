use crate::layout::LaidoutPattern;
use crate::mhn_hands::Coordinate;
use crate::mhn_jml::{MhnJmlEvent, MhnJmlPattern, MhnJmlTransition, MhnJmlTransitionType};

const MUTATION_POSITION_CM: f64 = 40.0;
const MUTATION_MIN_EVENT_DELTA_SECS: f64 = 0.03;
const MUTATION_TIMING_SCALE: f64 = 0.5;
const MUTATION_NEW_EVENT_POSITION_CM: f64 = 40.0;
const MUTATION_FREQUENCIES: [f64; 5] = [0.4, 0.1, 0.1, 0.2, 0.2];
pub const MUTATION_RATES: [f64; 7] = [0.2, 0.4, 0.7, 1.0, 1.3, 1.6, 2.0];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MutationKind {
    EventPosition,
    EventTime,
    PatternTiming,
    AddEvent,
    RemoveEvent,
}

impl MutationKind {
    const ALL: [Self; 5] = [
        Self::EventPosition,
        Self::EventTime,
        Self::PatternTiming,
        Self::AddEvent,
        Self::RemoveEvent,
    ];

    const fn index(self) -> usize {
        match self {
            Self::EventPosition => 0,
            Self::EventTime => 1,
            Self::PatternTiming => 2,
            Self::AddEvent => 3,
            Self::RemoveEvent => 4,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MutatorOptions {
    pub enabled: [bool; 5],
    pub rate_index: usize,
}

impl Default for MutatorOptions {
    fn default() -> Self {
        Self {
            enabled: [true; 5],
            rate_index: 3,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MutationResult {
    pub pattern: MhnJmlPattern,
    pub kind: Option<MutationKind>,
}

pub fn mutate_pattern_with_random(
    pattern: &MhnJmlPattern,
    options: &MutatorOptions,
    random: &mut impl FnMut() -> f64,
) -> Result<MutationResult, String> {
    let frequency_sum = MutationKind::ALL
        .iter()
        .filter(|kind| options.enabled[kind.index()])
        .map(|kind| MUTATION_FREQUENCIES[kind.index()])
        .sum::<f64>();
    if frequency_sum <= 0.0 {
        return Ok(MutationResult {
            pattern: pattern.clone(),
            kind: None,
        });
    }

    let rate = MUTATION_RATES[options.rate_index.min(MUTATION_RATES.len() - 1)];
    for _ in 0..5 {
        let selection = unit_random(random) * frequency_sum;
        let mut cumulative = 0.0;
        let kind = MutationKind::ALL
            .into_iter()
            .filter(|kind| options.enabled[kind.index()])
            .find(|kind| {
                cumulative += MUTATION_FREQUENCIES[kind.index()];
                selection < cumulative
            })
            .unwrap_or(MutationKind::RemoveEvent);

        let candidate = match kind {
            MutationKind::EventPosition => Some(mutate_event_position(pattern, rate, random)?),
            MutationKind::EventTime => mutate_event_time(pattern, random)?,
            MutationKind::PatternTiming => Some(mutate_pattern_timing(pattern, rate, random)),
            MutationKind::AddEvent => mutate_add_event(pattern, rate, random)?,
            MutationKind::RemoveEvent => mutate_remove_event(pattern, random),
        };
        let Some(mut candidate) = candidate else {
            continue;
        };
        prepare_and_validate(&mut candidate)?;
        return Ok(MutationResult {
            pattern: candidate,
            kind: Some(kind),
        });
    }

    Ok(MutationResult {
        pattern: pattern.clone(),
        kind: None,
    })
}

fn mutate_event_position(
    pattern: &MhnJmlPattern,
    rate: f64,
    random: &mut impl FnMut() -> f64,
) -> Result<MhnJmlPattern, String> {
    let index = random_index(pattern.events.len(), random)
        .ok_or_else(|| "Mutator cannot select an event from an empty pattern".to_string())?;
    let mut result = pattern.clone();
    let event = &mut result.events[index];
    let position = pick_new_position(
        event.hand,
        rate * MUTATION_POSITION_CM,
        Coordinate {
            x: event.x,
            y: event.y,
            z: event.z,
        },
        random,
    );
    event.x = position.x;
    event.y = position.y;
    event.z = position.z;
    Ok(result)
}

fn mutate_event_time(
    pattern: &MhnJmlPattern,
    random: &mut impl FnMut() -> f64,
) -> Result<Option<MhnJmlPattern>, String> {
    let Some(index) = random_index(pattern.events.len(), random) else {
        return Ok(None);
    };
    let event = &pattern.events[index];
    let period = pattern.loop_end_time()? - pattern.loop_start_time();
    let images = pattern.event_images_between(event.t - period, event.t + period)?;
    let previous = images
        .iter()
        .filter(|image| {
            image.event.juggler == event.juggler
                && image.event.hand == event.hand
                && image.event.t < event.t - 1e-9
        })
        .max_by(|left, right| left.event.t.total_cmp(&right.event.t));
    let next = images
        .iter()
        .filter(|image| {
            image.event.juggler == event.juggler
                && image.event.hand == event.hand
                && image.event.t > event.t + 1e-9
        })
        .min_by(|left, right| left.event.t.total_cmp(&right.event.t));
    let (Some(previous), Some(next)) = (previous, next) else {
        return Ok(None);
    };
    let minimum = pattern.loop_start_time().max(previous.event.t) + MUTATION_MIN_EVENT_DELTA_SECS;
    let maximum = pattern.loop_end_time()?.min(next.event.t) - MUTATION_MIN_EVENT_DELTA_SECS;
    if maximum <= minimum {
        return Ok(None);
    }

    let now = event.t;
    let sample = unit_random(random);
    let time = if sample < 0.5 {
        minimum + (now - minimum) * (2.0 * sample).sqrt()
    } else {
        maximum - (maximum - now) * (2.0 * (1.0 - sample)).sqrt()
    };
    let mut result = pattern.clone();
    result.events[index].t = time;
    result.select_primary_events()?;
    Ok(Some(result))
}

fn mutate_pattern_timing(
    pattern: &MhnJmlPattern,
    rate: f64,
    random: &mut impl FnMut() -> f64,
) -> MhnJmlPattern {
    let minimum = 1.0 / (1.0 + rate * MUTATION_TIMING_SCALE);
    let maximum = 1.0 + rate * MUTATION_TIMING_SCALE;
    let sample = unit_random(random);
    let scale = if sample < 0.5 {
        minimum + (1.0 - minimum) * (2.0 * sample).sqrt()
    } else {
        maximum - (maximum - 1.0) * (2.0 * (1.0 - sample)).sqrt()
    };
    pattern.with_scaled_time(scale)
}

fn mutate_add_event(
    pattern: &MhnJmlPattern,
    rate: f64,
    random: &mut impl FnMut() -> f64,
) -> Result<Option<MhnJmlPattern>, String> {
    let period = pattern.loop_end_time()? - pattern.loop_start_time();
    let layout = LaidoutPattern::from_jml_pattern(pattern)?;
    let mut target = None;
    for _ in 0..5 {
        let juggler = random_index(pattern.number_of_jugglers, random).unwrap_or(0) + 1;
        let hand = usize::from(unit_random(random) < 0.5);
        let target_time = pattern.loop_start_time() + period * unit_random(random);
        let images = pattern.event_images_between(target_time - period, target_time + period)?;
        let previous = images
            .iter()
            .filter(|image| {
                image.event.juggler == juggler
                    && image.event.hand == hand
                    && image.event.t < target_time
            })
            .max_by(|left, right| left.event.t.total_cmp(&right.event.t));
        let next = images
            .iter()
            .filter(|image| {
                image.event.juggler == juggler
                    && image.event.hand == hand
                    && image.event.t >= target_time
            })
            .min_by(|left, right| left.event.t.total_cmp(&right.event.t));
        let (Some(previous), Some(next)) = (previous, next) else {
            continue;
        };
        let minimum = previous.event.t + MUTATION_MIN_EVENT_DELTA_SECS;
        let maximum = next.event.t - MUTATION_MIN_EVENT_DELTA_SECS;
        if minimum <= maximum {
            target = Some((juggler, hand, minimum, maximum));
            break;
        }
    }
    let Some((juggler, hand, minimum, maximum)) = target else {
        return Ok(None);
    };

    let sample = unit_random(random);
    let mut time = if sample < 0.5 {
        minimum + (maximum - minimum) * (0.5 * sample).sqrt()
    } else {
        maximum - (maximum - minimum) * (0.5 * (1.0 - sample)).sqrt()
    };
    while time < pattern.loop_start_time() {
        time += period;
    }
    while time > pattern.loop_end_time()? {
        time -= period;
    }

    let global = layout.hand_coordinate(juggler, hand, time)?;
    let local = layout.convert_global_to_local(global, juggler, time)?;
    let position = pick_new_position(hand, rate * MUTATION_NEW_EVENT_POSITION_CM, local, random);
    let transitions = (1..=pattern.number_of_paths)
        .filter(|path| layout.is_hand_holding_path(juggler, hand, time, *path))
        .map(|path| MhnJmlTransition {
            transition_type: MhnJmlTransitionType::Holding,
            path,
            throw_type: None,
            throw_mod: None,
        })
        .collect();
    let mut result = pattern.clone();
    result.events.push(MhnJmlEvent {
        x: position.x,
        y: position.y,
        z: position.z,
        t: time,
        juggler,
        hand,
        calcpos: false,
        transitions,
    });
    result.select_primary_events()?;
    Ok(Some(result))
}

fn mutate_remove_event(
    pattern: &MhnJmlPattern,
    random: &mut impl FnMut() -> f64,
) -> Option<MhnJmlPattern> {
    let eligible = pattern
        .events
        .iter()
        .enumerate()
        .filter(|(_, event)| {
            event
                .transitions
                .iter()
                .all(|transition| transition.transition_type == MhnJmlTransitionType::Holding)
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let selected = random_index(eligible.len(), random)?;
    let mut result = pattern.clone();
    result.events.remove(eligible[selected]);
    Some(result)
}

fn pick_new_position(
    hand: usize,
    scale_distance: f64,
    position: Coordinate,
    random: &mut impl FnMut() -> f64,
) -> Coordinate {
    loop {
        let result = Coordinate {
            x: position.x + 2.0 * scale_distance * (unit_random(random) - 0.5),
            y: position.y,
            z: position.z + 2.0 * scale_distance * (unit_random(random) - 0.5),
        };
        let outside = if hand == 1 {
            result.x < -75.0 || result.x > 40.0 || result.z < -20.0 || result.z > 80.0
        } else {
            result.x < -40.0 || result.x > 75.0 || result.z < -20.0 || result.z > 80.0
        };
        if !outside || unit_random(random) >= 0.5 {
            return result;
        }
    }
}

fn prepare_and_validate(pattern: &mut MhnJmlPattern) -> Result<(), String> {
    pattern.sort_events();
    pattern.rebuild_path_events();
    pattern.assert_valid()?;
    LaidoutPattern::from_jml_pattern(pattern)?;
    Ok(())
}

fn random_index(length: usize, random: &mut impl FnMut() -> f64) -> Option<usize> {
    (length > 0).then(|| (unit_random(random) * length as f64).floor() as usize)
}

fn unit_random(random: &mut impl FnMut() -> f64) -> f64 {
    random().clamp(0.0, 1.0 - f64::EPSILON)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mhn_matrix::MhnMatrix;
    use crate::siteswap;

    fn cascade() -> MhnJmlPattern {
        let spec = siteswap::parse_config("pattern=3").unwrap();
        let mut matrix = MhnMatrix::from_siteswap(&spec).unwrap();
        matrix.to_jml_pattern(&spec).unwrap()
    }

    #[test]
    fn mutates_event_position_without_changing_its_plane() {
        let pattern = cascade();
        let original = pattern.events[0].clone();
        let mut values = [0.0, 0.0, 0.75, 0.25].into_iter();
        let result = mutate_pattern_with_random(
            &pattern,
            &MutatorOptions {
                enabled: [true, false, false, false, false],
                rate_index: 3,
            },
            &mut || values.next().unwrap_or(0.75),
        )
        .unwrap();

        assert_eq!(result.kind, Some(MutationKind::EventPosition));
        assert_eq!(result.pattern.events[0].y, original.y);
        assert_ne!(result.pattern.events[0].x, original.x);
        assert_ne!(result.pattern.events[0].z, original.z);
    }

    #[test]
    fn pattern_timing_scales_events_positions_and_delay_together() {
        let mut pattern = cascade();
        pattern.positions.push(crate::mhn_body::BodyPosition {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            t: pattern.period_secs / 2.0,
            angle: 0.0,
            juggler: 1,
        });
        let old_period = pattern.period_secs;
        let old_event_time = pattern.events[1].t;
        let old_position_time = pattern.positions[0].t;
        let mut values = [0.0, 0.9].into_iter();
        let result = mutate_pattern_with_random(
            &pattern,
            &MutatorOptions {
                enabled: [false, false, true, false, false],
                rate_index: 3,
            },
            &mut || values.next().unwrap_or(0.9),
        )
        .unwrap();
        let scale = result.pattern.period_secs / old_period;

        assert_eq!(result.kind, Some(MutationKind::PatternTiming));
        assert!((result.pattern.events[1].t - old_event_time * scale).abs() < 1e-9);
        assert!((result.pattern.positions[0].t - old_position_time * scale).abs() < 1e-9);
        assert!((result.pattern.loop_end_time().unwrap() - old_period * scale).abs() < 1e-9);
    }

    #[test]
    fn disabled_mutations_return_the_original_pattern() {
        let pattern = cascade();
        let result = mutate_pattern_with_random(
            &pattern,
            &MutatorOptions {
                enabled: [false; 5],
                rate_index: 3,
            },
            &mut || 0.5,
        )
        .unwrap();

        assert_eq!(result.kind, None);
        assert_eq!(result.pattern, pattern);
    }

    #[test]
    fn default_mutator_keeps_generated_variants_physical() {
        let pattern = cascade();
        let mut state = 0x5eed_u64;
        let mut random = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            ((state >> 11) as f64) / ((1_u64 << 53) as f64)
        };

        for _ in 0..128 {
            let result =
                mutate_pattern_with_random(&pattern, &MutatorOptions::default(), &mut random)
                    .unwrap();
            result.pattern.assert_valid().unwrap();
            LaidoutPattern::from_jml_pattern(&result.pattern).unwrap();
        }
    }
}

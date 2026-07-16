use crate::animation::{JmlAnimation, TransitionKind};
use crate::mhn_symmetry::MhnSymmetryType;
use std::collections::BTreeMap;

const MIN_THROW_SEP_TIME: f64 = 0.03;
const MIN_EVENT_SEP_TIME: f64 = 0.01;
const MIN_POSITION_SEP_TIME: f64 = 0.02;
pub const MAX_JUGGLERS: usize = 8;
pub const MAX_PATHS: usize = 100;
pub const LADDER_BORDER_SIDES: f64 = 0.15;
pub const LADDER_JUGGLER_SEPARATION: f64 = 0.45;
const BASE_ITEM_RADIUS_PX: f64 = 5.0;
const BORDER_TOP_PX: f64 = 25.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LadderItemSizing {
    pub transition_radius_px: f64,
    pub position_radius_px: f64,
}

pub fn ladder_item_sizing(
    diagram: &LadderDiagram,
    width_px: f64,
    period_height_px: f64,
    mobile: bool,
) -> LadderItemSizing {
    let base = LadderItemSizing {
        transition_radius_px: BASE_ITEM_RADIUS_PX,
        position_radius_px: BASE_ITEM_RADIUS_PX,
    };
    if !mobile
        || !width_px.is_finite()
        || width_px <= 0.0
        || !period_height_px.is_finite()
        || period_height_px <= 0.0
    {
        return base;
    }

    let jugglers = diagram
        .tracks
        .iter()
        .map(|track| track.juggler)
        .max()
        .unwrap_or(1)
        .max(1);
    let width_units = 2.0 * LADDER_BORDER_SIDES
        + jugglers as f64
        + jugglers.saturating_sub(1) as f64 * LADDER_JUGGLER_SEPARATION;
    let scale = width_px / width_units;
    let left_x = (scale * LADDER_BORDER_SIDES).round();
    let right_x = (scale * (LADDER_BORDER_SIDES + 1.0)).round();
    let max_transitions = diagram
        .events
        .iter()
        .map(|event| event.transitions.len())
        .max()
        .unwrap_or(0);
    let horizontal_divisor = 4 * max_transitions + 4;
    let expanded_radius = ((right_x - left_x) / horizontal_divisor as f64).floor();
    let mut radius = BASE_ITEM_RADIUS_PX.max(expanded_radius);

    let mut events_by_hand = BTreeMap::<(usize, LadderHand), Vec<f64>>::new();
    for event in &diagram.events {
        events_by_hand
            .entry((event.juggler, event.hand))
            .or_default()
            .push(event.time);
    }
    let period = diagram.period_secs.max(0.1);
    let mut separations = Vec::new();
    for times in events_by_hand.values_mut() {
        times.sort_by(f64::total_cmp);
        for pair in times.windows(2) {
            let first_y = (period_height_px * pair[0] / period).round();
            let second_y = (period_height_px * pair[1] / period).round();
            let separation = second_y - first_y;
            if separation > 0.0 {
                separations.push(separation);
            }
        }
    }
    separations.sort_by(f64::total_cmp);
    if separations.len() > 2 {
        let minimum_separation = (separations[2] * 0.8).round();
        if minimum_separation < 2.0 * radius {
            radius = (minimum_separation / 2.0).round();
        }
    }

    radius = radius.clamp(BASE_ITEM_RADIUS_PX, BORDER_TOP_PX - 3.0);
    LadderItemSizing {
        transition_radius_px: radius,
        position_radius_px: radius,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LadderLimit {
    Jugglers { actual: usize, maximum: usize },
    Paths { actual: usize, maximum: usize },
}

pub fn ladder_limit(jugglers: usize, paths: usize) -> Option<LadderLimit> {
    if jugglers > MAX_JUGGLERS {
        Some(LadderLimit::Jugglers {
            actual: jugglers,
            maximum: MAX_JUGGLERS,
        })
    } else if paths > MAX_PATHS {
        Some(LadderLimit::Paths {
            actual: paths,
            maximum: MAX_PATHS,
        })
    } else {
        None
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LadderDiagram {
    pub period_secs: f64,
    pub has_switch_symmetry: bool,
    pub has_switch_delay_symmetry: bool,
    pub tracks: Vec<LadderTrack>,
    pub events: Vec<LadderEvent>,
    pub transitions: Vec<LadderTransition>,
    pub positions: Vec<LadderPosition>,
    pub edges: Vec<LadderEdge>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LadderTrack {
    pub index: usize,
    pub juggler: usize,
    pub hand: LadderHand,
    pub label: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LadderHand {
    Left,
    Right,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LadderEndpoint {
    pub event_index: usize,
    pub time: f64,
    pub juggler: usize,
    pub hand: LadderHand,
    pub track_index: usize,
    pub transition_index: usize,
    pub transition: TransitionKind,
    pub throw_type: Option<String>,
    pub throw_mod: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LadderEvent {
    pub id: String,
    pub event_index: usize,
    pub primary_time: f64,
    pub time: f64,
    pub juggler: usize,
    pub hand: LadderHand,
    pub track_index: usize,
    pub transitions: Vec<LadderEventTransition>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LadderEventTransition {
    pub path: usize,
    pub transition: TransitionKind,
    pub throw_type: Option<String>,
    pub throw_mod: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LadderTransition {
    pub id: String,
    pub event_index: usize,
    pub transition_index: usize,
    pub time: f64,
    pub juggler: usize,
    pub hand: LadderHand,
    pub track_index: usize,
    pub path: usize,
    pub transition: TransitionKind,
    pub throw_type: Option<String>,
    pub throw_mod: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LadderPosition {
    pub id: String,
    pub position_index: usize,
    pub time: f64,
    pub juggler: usize,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub angle: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LadderEdge {
    pub id: String,
    pub path: usize,
    pub start: LadderEndpoint,
    pub end: LadderEndpoint,
    pub end_time_absolute: f64,
    pub wraps_period: bool,
}

pub fn build_ladder_diagram(jml: &JmlAnimation) -> LadderDiagram {
    let period_secs = jml.period_secs.max(0.1);
    let tracks = build_tracks(jml);
    let track_index = tracks
        .iter()
        .map(|track| ((track.juggler, track.hand), track.index))
        .collect::<BTreeMap<_, _>>();
    let mut events = Vec::new();
    let mut transitions = Vec::new();
    let mut primary_occurrences = BTreeMap::<usize, usize>::new();

    for (image_index, image) in jml.loop_event_images.iter().enumerate() {
        let event = &image.event;
        let event_index = image.primary_index;
        let (juggler, hand) = parse_event_hand(&event.hand);
        let track = track_index.get(&(juggler, hand)).copied().unwrap_or(0);
        let event_number = image_index + 1;
        let occurrence = primary_occurrences.entry(event_index).or_insert(0);
        let occurrence_index = *occurrence;
        *occurrence += 1;
        events.push(LadderEvent {
            id: format!("event-{event_number}"),
            event_index,
            primary_time: image.primary_time,
            time: event.t.rem_euclid(period_secs),
            juggler,
            hand,
            track_index: track,
            transitions: event
                .transitions
                .iter()
                .map(|transition| LadderEventTransition {
                    path: transition.path,
                    transition: transition.kind,
                    throw_type: transition.throw_type.clone(),
                    throw_mod: transition.throw_mod.clone(),
                })
                .collect(),
        });
        for (transition_index, transition) in event.transitions.iter().enumerate() {
            let transition_id = if occurrence_index == 0 {
                format!("transition-{event_index}-{transition_index}")
            } else {
                format!("transition-{event_index}-{transition_index}-image-{occurrence_index}")
            };
            transitions.push(LadderTransition {
                id: transition_id,
                event_index,
                transition_index,
                time: event.t.rem_euclid(period_secs),
                juggler,
                hand,
                track_index: track,
                path: transition.path,
                transition: transition.kind,
                throw_type: transition.throw_type.clone(),
                throw_mod: transition.throw_mod.clone(),
            });
        }
    }

    let mut edges = Vec::new();
    for (start_image_index, start_image) in jml.all_event_images.iter().enumerate() {
        let start_event = &start_image.event;
        let (start_juggler, start_hand) = parse_event_hand(&start_event.hand);
        let Some(start_track) = track_index.get(&(start_juggler, start_hand)).copied() else {
            continue;
        };

        for (start_transition_index, start_transition) in start_event.transitions.iter().enumerate()
        {
            if start_transition.path == 0 || start_transition.path > jml.paths.max(1) {
                continue;
            }
            let Some(end_image) =
                jml.all_event_images[start_image_index + 1..]
                    .iter()
                    .find(|image| {
                        image
                            .event
                            .transitions
                            .iter()
                            .any(|transition| transition.path == start_transition.path)
                    })
            else {
                continue;
            };
            let Some((end_transition_index, end_transition)) = end_image
                .event
                .transitions
                .iter()
                .enumerate()
                .find(|(_, transition)| transition.path == start_transition.path)
            else {
                continue;
            };
            let end_event = &end_image.event;
            let end_time_absolute = end_event.t;
            if end_time_absolute - start_event.t <= 1e-9 {
                continue;
            }
            if end_time_absolute < -1e-9 || start_event.t > period_secs + 1e-9 {
                continue;
            }
            let (end_juggler, end_hand) = parse_event_hand(&end_event.hand);
            let Some(end_track) = track_index.get(&(end_juggler, end_hand)).copied() else {
                continue;
            };
            let wraps_period = start_event.t < 0.0 || end_time_absolute > period_secs;
            let edge_number = edges.len() + 1;
            edges.push(LadderEdge {
                id: format!("path-{}-{edge_number}", start_transition.path),
                path: start_transition.path,
                start: LadderEndpoint {
                    event_index: start_image.primary_index,
                    time: start_event.t,
                    juggler: start_juggler,
                    hand: start_hand,
                    track_index: start_track,
                    transition_index: start_transition_index,
                    transition: start_transition.kind,
                    throw_type: start_transition.throw_type.clone(),
                    throw_mod: start_transition.throw_mod.clone(),
                },
                end: LadderEndpoint {
                    event_index: end_image.primary_index,
                    time: end_event.t,
                    juggler: end_juggler,
                    hand: end_hand,
                    track_index: end_track,
                    transition_index: end_transition_index,
                    transition: end_transition.kind,
                    throw_type: end_transition.throw_type.clone(),
                    throw_mod: end_transition.throw_mod.clone(),
                },
                end_time_absolute,
                wraps_period,
            });
        }
    }

    LadderDiagram {
        period_secs,
        has_switch_symmetry: jml.symmetries.contains(&MhnSymmetryType::Switch),
        has_switch_delay_symmetry: jml.symmetries.contains(&MhnSymmetryType::SwitchDelay),
        tracks,
        events,
        transitions,
        positions: jml
            .positions
            .iter()
            .enumerate()
            .map(|(position_index, position)| LadderPosition {
                id: format!("position-{}", position_index + 1),
                position_index,
                time: position.t.rem_euclid(period_secs),
                juggler: position.juggler.max(1),
                x: position.x,
                y: position.y,
                z: position.z,
                angle: position.angle,
            })
            .collect(),
        edges,
    }
}

fn build_tracks(jml: &JmlAnimation) -> Vec<LadderTrack> {
    let mut tracks = (1..=jml.jugglers.max(1))
        .flat_map(|juggler| [(juggler, LadderHand::Left), (juggler, LadderHand::Right)])
        .collect::<Vec<_>>();

    for image in &jml.loop_event_images {
        let parsed = parse_event_hand(&image.event.hand);
        if !tracks.contains(&parsed) {
            tracks.push(parsed);
        }
    }

    tracks.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then(hand_order(left.1).cmp(&hand_order(right.1)))
    });
    tracks
        .into_iter()
        .enumerate()
        .map(|(index, (juggler, hand))| LadderTrack {
            index,
            juggler,
            hand,
            label: format!("J{juggler} {}", hand.short_label()),
        })
        .collect()
}

fn parse_event_hand(value: &str) -> (usize, LadderHand) {
    let mut parts = value.split(':');
    let juggler = parts
        .next()
        .and_then(|part| part.trim().parse::<usize>().ok())
        .unwrap_or(1)
        .max(1);
    let hand = match parts.next().unwrap_or("right").trim().to_ascii_lowercase() {
        value if value == "left" => LadderHand::Left,
        _ => LadderHand::Right,
    };
    (juggler, hand)
}

fn hand_order(hand: LadderHand) -> usize {
    match hand {
        LadderHand::Left => 0,
        LadderHand::Right => 1,
    }
}

impl LadderHand {
    pub fn short_label(self) -> &'static str {
        match self {
            Self::Left => "L",
            Self::Right => "R",
        }
    }

    pub fn long_label(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

impl LadderEndpoint {
    pub fn transition_label(&self) -> &'static str {
        transition_label(self.transition)
    }

    pub fn hand_label(&self) -> String {
        format!("J{} {}", self.juggler, self.hand.long_label())
    }
}

impl LadderEvent {
    pub fn hand_label(&self) -> String {
        format!("J{} {}", self.juggler, self.hand.long_label())
    }

    pub fn transition_summary(&self) -> String {
        if self.transitions.is_empty() {
            return "position".to_string();
        }

        self.transitions
            .iter()
            .map(|transition| {
                format!(
                    "{} path {}",
                    transition_label(transition.transition),
                    transition.path
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn has_throw(&self) -> bool {
        self.transitions
            .iter()
            .any(|transition| transition.transition == TransitionKind::Throw)
    }

    pub fn has_throw_or_catch(&self) -> bool {
        self.transitions
            .iter()
            .any(LadderEventTransition::is_throw_or_catch)
    }
}

impl LadderEventTransition {
    pub fn is_throw_or_catch(&self) -> bool {
        matches!(
            self.transition,
            TransitionKind::Throw
                | TransitionKind::Catch
                | TransitionKind::SoftCatch
                | TransitionKind::GrabCatch
        )
    }
}

impl LadderTransition {
    pub fn hand_label(&self) -> String {
        format!("J{} {}", self.juggler, self.hand.long_label())
    }

    pub fn transition_label(&self) -> &'static str {
        transition_label(self.transition)
    }

    pub fn label(&self) -> String {
        format!(
            "{} {} path {} at {:.3}s",
            self.hand_label(),
            self.transition_label(),
            self.path,
            self.time
        )
    }

    pub fn is_catch_style(&self) -> bool {
        matches!(
            self.transition,
            TransitionKind::Catch | TransitionKind::SoftCatch | TransitionKind::GrabCatch
        )
    }
}

impl LadderPosition {
    pub fn label(&self) -> String {
        format!(
            "J{} position at {:.3}s: x {:.2}, y {:.2}, z {:.2}, angle {:.2}",
            self.juggler, self.time, self.x, self.y, self.z, self.angle
        )
    }
}

impl LadderDiagram {
    pub fn constrain_event_time(&self, event_id: &str, requested_time: f64) -> Option<f64> {
        let event = self.events.iter().find(|event| event.id == event_id)?;
        let period_secs = self.period_secs.max(0.1);
        let requested_time = requested_time.clamp(0.0, period_secs - 0.0001);
        let event_paths = event
            .transitions
            .iter()
            .filter(|transition| transition.is_throw_or_catch())
            .map(|transition| transition.path)
            .collect::<Vec<_>>();
        let mut min_time: f64 = 0.0;
        let mut max_time: f64 = period_secs;

        for other in self
            .events
            .iter()
            .filter(|other| other.event_index != event.event_index)
            .filter(|other| {
                other.transitions.iter().any(|transition| {
                    transition.is_throw_or_catch() && event_paths.contains(&transition.path)
                })
            })
        {
            if other.time < event.time - MIN_EVENT_SEP_TIME {
                min_time = min_time.max(other.time + MIN_THROW_SEP_TIME);
            } else if other.time > event.time + MIN_THROW_SEP_TIME {
                max_time = max_time.min(other.time - MIN_THROW_SEP_TIME);
            }
        }

        let mut constrained = requested_time.clamp(min_time, max_time);
        let mut excl_min = constrained;
        let mut excl_max = constrained;

        loop {
            let mut changed = false;
            for other in self.events.iter().filter(|other| {
                other.event_index != event.event_index
                    && other.juggler == event.juggler
                    && other.hand == event.hand
            }) {
                let separation = if (other.has_throw() && event.has_throw_or_catch())
                    || (other.has_throw_or_catch() && event.has_throw())
                {
                    MIN_THROW_SEP_TIME
                } else {
                    MIN_EVENT_SEP_TIME
                };
                let other_excl_min = other.time - separation;
                let other_excl_max = other.time + separation;

                if excl_min > other_excl_min && excl_min <= other_excl_max {
                    excl_min = other_excl_min;
                    changed = true;
                }
                if excl_max >= other_excl_min && excl_max < other_excl_max {
                    excl_max = other_excl_max;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        let feasible_min = (min_time..=max_time).contains(&excl_min);
        let feasible_max = (min_time..=max_time).contains(&excl_max);
        if feasible_min && feasible_max {
            let midpoint = 0.5 * (excl_min + excl_max);
            constrained = if constrained <= midpoint {
                excl_min
            } else {
                excl_max
            };
        } else if feasible_min {
            constrained = excl_min;
        } else if feasible_max {
            constrained = excl_max;
        }

        Some(constrained.clamp(0.0, period_secs - 0.0001))
    }

    pub fn constrain_position_time(
        &self,
        position_index: usize,
        requested_time: f64,
    ) -> Option<f64> {
        let position = self
            .positions
            .iter()
            .find(|position| position.position_index == position_index)?;
        let period_secs = self.period_secs.max(0.1);
        let mut constrained = requested_time.clamp(0.0, period_secs - 0.0001);
        let mut excl_min = constrained;
        let mut excl_max = constrained;

        loop {
            let mut changed = false;
            for other in self.positions.iter().filter(|other| {
                other.position_index != position.position_index && other.juggler == position.juggler
            }) {
                let other_excl_min = other.time - MIN_POSITION_SEP_TIME;
                let other_excl_max = other.time + MIN_POSITION_SEP_TIME;

                if excl_max >= other_excl_min && excl_max < other_excl_max {
                    excl_max = other_excl_max;
                    changed = true;
                }
                if other_excl_min < excl_min && other_excl_max >= excl_min {
                    excl_min = other_excl_min;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        let feasible_min = (0.0..=period_secs).contains(&excl_min);
        let feasible_max = (0.0..=period_secs).contains(&excl_max);
        if feasible_min && feasible_max {
            let midpoint = 0.5 * (excl_min + excl_max);
            constrained = if constrained <= midpoint {
                excl_min
            } else {
                excl_max
            };
        } else if feasible_min {
            constrained = excl_min;
        } else if feasible_max {
            constrained = excl_max;
        }

        Some(constrained.clamp(0.0, period_secs - 0.0001))
    }
}

impl LadderEdge {
    pub fn duration_secs(&self) -> f64 {
        self.end_time_absolute - self.start.time
    }

    pub fn is_crossing(&self) -> bool {
        self.start.juggler == self.end.juggler && self.start.hand != self.end.hand
    }

    pub fn is_pass(&self) -> bool {
        self.start.juggler != self.end.juggler
    }

    pub fn is_self_throw(&self) -> bool {
        !self.includes_holding()
            && self.start.juggler == self.end.juggler
            && self.start.hand == self.end.hand
    }

    pub fn includes_holding(&self) -> bool {
        self.start.transition != TransitionKind::Throw
    }
}

pub fn transition_label(kind: TransitionKind) -> &'static str {
    match kind {
        TransitionKind::Throw => "throw",
        TransitionKind::Catch => "catch",
        TransitionKind::SoftCatch => "soft catch",
        TransitionKind::GrabCatch => "grab catch",
        TransitionKind::Holding => "holding",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ladder_limits_match_the_original_editor() {
        assert_eq!(ladder_limit(MAX_JUGGLERS, MAX_PATHS), None);
        assert_eq!(
            ladder_limit(MAX_JUGGLERS + 1, MAX_PATHS + 1),
            Some(LadderLimit::Jugglers {
                actual: MAX_JUGGLERS + 1,
                maximum: MAX_JUGGLERS,
            })
        );
        assert_eq!(
            ladder_limit(1, MAX_PATHS + 1),
            Some(LadderLimit::Paths {
                actual: MAX_PATHS + 1,
                maximum: MAX_PATHS,
            })
        );
    }

    #[test]
    fn ladder_mobile_item_sizing_matches_original_width_limits() {
        let xml = r#"
        <jml version="3">
        <pattern>
        <setup jugglers="1" paths="2" props="1,1"/>
        <symmetry type="delay" pperm="(1,2)" delay="1"/>
        <event x="0" y="0" z="0" t="0" hand="1:right">
          <throw path="1" type="toss"/>
          <holding path="2"/>
        </event>
        <event x="0" y="0" z="0" t="0.5" hand="1:left">
          <catch path="1"/>
          <holding path="2"/>
        </event>
        </pattern>
        </jml>
        "#;
        let diagram = build_ladder_diagram(&parse_jml_animation(xml).unwrap());

        assert_eq!(
            ladder_item_sizing(&diagram, 390.0, 500.0, false),
            LadderItemSizing {
                transition_radius_px: 5.0,
                position_radius_px: 5.0,
            }
        );
        assert_eq!(
            ladder_item_sizing(&diagram, 390.0, 500.0, true),
            LadderItemSizing {
                transition_radius_px: 22.0,
                position_radius_px: 22.0,
            }
        );
    }
    use crate::animation::{parse_jml_animation, siteswap_to_jml_animation};
    use crate::siteswap::parse_config;

    #[test]
    fn ladder_uses_declared_event_hands_instead_of_coordinates() {
        let xml = r#"
        <jml version="3">
        <pattern>
        <prop type="ball"/>
        <setup jugglers="1" paths="1" props="1"/>
        <symmetry type="delay" pperm="1" delay="1"/>
        <event x="0" y="0" z="0" t="0" hand="1:right">
          <throw path="1" type="toss"/>
        </event>
        <event x="0" y="0" z="0" t="0.5" hand="1:left">
          <catch path="1" type="soft"/>
        </event>
        </pattern>
        </jml>
        "#;
        let jml = parse_jml_animation(xml).unwrap();
        let diagram = build_ladder_diagram(&jml);
        let first = diagram
            .edges
            .iter()
            .find(|edge| edge.start.transition == TransitionKind::Throw)
            .unwrap();

        assert_eq!(first.start.hand, LadderHand::Right);
        assert_eq!(first.end.hand, LadderHand::Left);
        assert!(first.is_crossing());
    }

    #[test]
    fn ladder_marks_edges_that_wrap_the_period() {
        let xml = r#"
        <jml version="3">
        <pattern>
        <prop type="ball"/>
        <setup jugglers="1" paths="1" props="1"/>
        <symmetry type="delay" pperm="1" delay="1"/>
        <event x="0" y="0" z="0" t="0.25" hand="1:left">
          <catch path="1" type="soft"/>
        </event>
        <event x="0" y="0" z="0" t="0.75" hand="1:right">
          <throw path="1" type="toss"/>
        </event>
        </pattern>
        </jml>
        "#;
        let jml = parse_jml_animation(xml).unwrap();
        let diagram = build_ladder_diagram(&jml);

        assert!(diagram.edges.iter().any(|edge| edge.wraps_period));
    }

    #[test]
    fn ladder_exposes_switch_symmetry_guides_from_jml() {
        let xml = r#"
        <jml version="3">
        <pattern>
        <setup jugglers="1" paths="1" props="1"/>
        <symmetry type="delay" pperm="1" delay="1"/>
        <symmetry type="switch" jperm="(1,1*)" pperm="1"/>
        <symmetry type="switchdelay" jperm="(1,1*)" pperm="1"/>
        <event x="0" y="0" z="0" t="0" hand="1:right">
          <throw path="1" type="toss"/>
        </event>
        <event x="0" y="0" z="0" t="0.5" hand="1:left">
          <catch path="1"/>
        </event>
        </pattern>
        </jml>
        "#;
        let jml = parse_jml_animation(xml).unwrap();
        let diagram = build_ladder_diagram(&jml);

        assert!(diagram.has_switch_symmetry);
        assert!(diagram.has_switch_delay_symmetry);
    }

    #[test]
    fn ladder_exposes_selectable_event_nodes() {
        let xml = r#"
        <jml version="3">
        <pattern>
        <setup jugglers="1" paths="1" props="1"/>
        <symmetry type="delay" pperm="1" delay="1"/>
        <event x="0" y="0" z="0" t="0" hand="1:right">
          <throw path="1" type="toss"/>
        </event>
        <event x="0" y="0" z="0" t="0.5" hand="1:left">
          <catch path="1"/>
        </event>
        </pattern>
        </jml>
        "#;
        let jml = parse_jml_animation(xml).unwrap();
        let diagram = build_ladder_diagram(&jml);

        assert_eq!(diagram.events.len(), 2);
        assert_eq!(diagram.events[0].event_index, 0);
        assert_eq!(diagram.events[1].event_index, 1);
        assert_eq!(diagram.events[0].hand_label(), "J1 right");
        assert_eq!(diagram.events[0].transition_summary(), "throw path 1");
        assert_eq!(diagram.events[1].transition_summary(), "catch path 1");
    }

    #[test]
    fn ladder_event_time_constraint_avoids_same_hand_overlap() {
        let xml = r#"
        <jml version="3">
        <pattern>
        <setup jugglers="1" paths="2" props="1,1"/>
        <symmetry type="delay" pperm="(1,2)" delay="1"/>
        <event x="0" y="0" z="0" t="0" hand="1:right">
          <throw path="1" type="toss"/>
        </event>
        <event x="0" y="0" z="0" t="0.5" hand="1:right">
          <throw path="2" type="toss"/>
        </event>
        </pattern>
        </jml>
        "#;
        let jml = parse_jml_animation(xml).unwrap();
        let diagram = build_ladder_diagram(&jml);

        let constrained = diagram.constrain_event_time("event-1", 0.5).unwrap();

        assert!((constrained - 0.47).abs() < 1e-9);
    }

    #[test]
    fn ladder_exposes_body_position_nodes() {
        let xml = r#"
        <jml version="3">
        <pattern>
        <setup jugglers="1" paths="1" props="1"/>
        <symmetry type="delay" pperm="1" delay="1"/>
        <position x="10" y="20" z="30" t="0.25" angle="15" juggler="1"/>
        <event x="0" y="0" z="0" t="0" hand="1:right">
          <throw path="1" type="toss"/>
        </event>
        <event x="0" y="0" z="0" t="0.5" hand="1:left">
          <catch path="1"/>
        </event>
        </pattern>
        </jml>
        "#;
        let jml = parse_jml_animation(xml).unwrap();
        let diagram = build_ladder_diagram(&jml);

        assert_eq!(diagram.positions.len(), 1);
        assert_eq!(diagram.positions[0].position_index, 0);
        assert_eq!(
            diagram.positions[0].label(),
            "J1 position at 0.250s: x 10.00, y 20.00, z 30.00, angle 15.00"
        );
    }

    #[test]
    fn ladder_exposes_transition_nodes_separate_from_events() {
        let xml = r#"
        <jml version="3">
        <pattern>
        <setup jugglers="1" paths="2" props="1,1"/>
        <symmetry type="delay" pperm="(1,2)" delay="1"/>
        <event x="0" y="0" z="0" t="0" hand="1:right">
          <throw path="1" type="toss"/>
          <holding path="2"/>
        </event>
        <event x="0" y="0" z="0" t="0.5" hand="1:left">
          <catch path="1" type="soft"/>
        </event>
        </pattern>
        </jml>
        "#;
        let jml = parse_jml_animation(xml).unwrap();
        let diagram = build_ladder_diagram(&jml);

        assert_eq!(diagram.events.len(), 2);
        assert_eq!(diagram.transitions.len(), 3);
        assert_eq!(diagram.transitions[0].id, "transition-0-0");
        assert_eq!(
            diagram.transitions[0].label(),
            "J1 right throw path 1 at 0.000s"
        );
        assert_eq!(diagram.transitions[2].transition_label(), "soft catch");
        assert!(diagram.transitions[2].is_catch_style());
        let first_throw = diagram
            .edges
            .iter()
            .find(|edge| {
                edge.start.event_index == 0
                    && edge.start.transition_index == 0
                    && (edge.start.time - 0.0).abs() < 1e-9
            })
            .unwrap();
        assert_eq!(first_throw.end.event_index, 1);
    }

    #[test]
    fn ladder_creates_an_edge_for_every_transition_like_original_layout() {
        let xml = r#"
        <jml version="3">
        <pattern>
        <setup jugglers="1" paths="1" props="1"/>
        <symmetry type="delay" pperm="1" delay="1"/>
        <event x="0" y="0" z="0" t="0" hand="1:right">
          <catch path="1"/>
          <throw path="1" type="toss"/>
        </event>
        <event x="0" y="0" z="0" t="0.5" hand="1:left">
          <catch path="1"/>
        </event>
        </pattern>
        </jml>
        "#;
        let jml = parse_jml_animation(xml).unwrap();
        let diagram = build_ladder_diagram(&jml);
        let starts = diagram
            .edges
            .iter()
            .filter(|edge| {
                edge.start.event_index == 0 && edge.start.time.abs() < 1e-9 && edge.path == 1
            })
            .collect::<Vec<_>>();

        assert_eq!(starts.len(), 2);
        assert_eq!(starts[0].start.transition_index, 0);
        assert_eq!(starts[1].start.transition_index, 1);
        assert!(starts[0].includes_holding());
        assert!(!starts[1].includes_holding());
        assert_eq!(starts[0].end.event_index, 1);
        assert_eq!(starts[0].end.transition_index, 0);
        assert_eq!(starts[0].end.time, starts[1].end.time);
    }

    #[test]
    fn ladder_uses_symmetry_expanded_loop_event_images() {
        let spec = parse_config("pattern=3").unwrap();
        let jml = siteswap_to_jml_animation(&spec).unwrap();
        let diagram = build_ladder_diagram(&jml);

        assert_eq!(diagram.events.len(), 4);
        assert_eq!(diagram.events.len(), jml.loop_event_images.len());
        assert!(diagram.events.iter().any(|event| {
            (event.time - event.primary_time).abs() > 1e-9
                || diagram
                    .events
                    .iter()
                    .filter(|other| other.event_index == event.event_index)
                    .count()
                    > 1
        }));
        assert!(diagram.edges.iter().any(|edge| edge.start.time < 0.0));
        assert!(
            diagram
                .edges
                .iter()
                .any(|edge| edge.end_time_absolute > diagram.period_secs)
        );
        for transition in &diagram.transitions {
            assert!(diagram.edges.iter().any(|edge| {
                edge.start.event_index == transition.event_index
                    && edge.start.transition_index == transition.transition_index
                    && (edge.start.time - transition.time).abs() < 1e-9
            }));
        }
    }

    #[test]
    fn ladder_classifies_same_hand_self_throw_edges() {
        let xml = r#"
        <jml version="3">
        <pattern>
        <setup jugglers="1" paths="1" props="1"/>
        <symmetry type="delay" pperm="(1)" delay="2"/>
        <event x="0" y="0" z="0" t="0" hand="1:right">
          <throw path="1" type="toss"/>
        </event>
        <event x="0" y="0" z="0" t="1" hand="1:right">
          <catch path="1"/>
        </event>
        </pattern>
        </jml>
        "#;
        let jml = parse_jml_animation(xml).unwrap();
        let diagram = build_ladder_diagram(&jml);

        let self_throw = diagram
            .edges
            .iter()
            .find(|edge| {
                edge.start.transition == TransitionKind::Throw
                    && edge.start.juggler == edge.end.juggler
                    && edge.start.hand == edge.end.hand
            })
            .unwrap();
        assert!(self_throw.is_self_throw());
        assert!(!self_throw.is_crossing());
    }
}

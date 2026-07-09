use crate::curve::Curve;
use crate::mhn_hands::Coordinate;
use crate::mhn_jml::{MhnJmlEvent, MhnJmlPattern, MhnJmlTransition, MhnJmlTransitionType};
use crate::parameter_list::ParameterList;

pub const PATTERN_Y: f64 = 30.0;
pub const TOSS_GRAVITY_DEFAULT: f64 = 980.0;

#[derive(Clone, Debug, PartialEq)]
pub struct LaidoutPattern {
    pub number_of_jugglers: usize,
    pub number_of_paths: usize,
    pub loop_start_time: f64,
    pub loop_end_time: f64,
    pub events: Vec<LayoutEvent>,
    pub path_links: Vec<Vec<PathLink>>,
    pub hand_links: Vec<Vec<Vec<HandLink>>>,
    pub juggler_position_curves: Vec<Curve>,
    pub juggler_angle_curves: Vec<Curve>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutEvent {
    pub event: MhnJmlEvent,
    pub primary_index: usize,
    pub is_primary: bool,
    pub global_coordinate: Coordinate,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PathLink {
    pub start_global_coordinate: Coordinate,
    pub start_event_index: usize,
    pub end_global_coordinate: Coordinate,
    pub end_event_index: usize,
    pub kind: PathLinkKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PathLinkKind {
    Toss(TossPath),
    Bounce(BouncePath),
    InHand { juggler: usize, hand: usize },
}

#[derive(Clone, Debug, PartialEq)]
pub struct HandLink {
    pub juggler: usize,
    pub hand: usize,
    pub start_event_index: usize,
    pub end_event_index: usize,
    pub start_velocity_ref: Option<VelocityRef>,
    pub end_velocity_ref: Option<VelocityRef>,
    pub hand_curve: Option<Curve>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VelocityRefSource {
    Throw,
    Catch,
    SoftCatch,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VelocityRef {
    pub source: VelocityRefSource,
    pub velocity: Coordinate,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TossPath {
    pub start: Coordinate,
    pub end: Coordinate,
    pub start_time: f64,
    pub end_time: f64,
    pub gravity: f64,
    bx: f64,
    cx: f64,
    by: f64,
    cy: f64,
    az: f64,
    bz: f64,
    cz: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BouncePath {
    pub start: Coordinate,
    pub end: Coordinate,
    pub start_time: f64,
    pub end_time: f64,
    pub bounces: usize,
    pub forced: bool,
    pub hyper: bool,
    pub bounceplane: f64,
    pub bouncefrac: f64,
    pub gravity: f64,
    bouncefracsqrt: f64,
    numbounces: usize,
    bx: f64,
    cx: f64,
    by: f64,
    cy: f64,
    az: Vec<f64>,
    bz: Vec<f64>,
    cz: Vec<f64>,
    endtime: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JugglerFrame {
    pub left_hand: Coordinate,
    pub right_hand: Coordinate,
    pub left_shoulder: Coordinate,
    pub right_shoulder: Coordinate,
    pub left_elbow: Option<Coordinate>,
    pub right_elbow: Option<Coordinate>,
    pub left_waist: Coordinate,
    pub right_waist: Coordinate,
    pub left_head_bottom: Coordinate,
    pub left_head_top: Coordinate,
    pub right_head_bottom: Coordinate,
    pub right_head_top: Coordinate,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutBounds {
    pub min: Coordinate,
    pub max: Coordinate,
    pub zoom_center: Coordinate,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct JugglerVector {
    x: f64,
    y: f64,
    z: f64,
}

const SHOULDER_HW: f64 = 23.0;
const SHOULDER_H: f64 = 40.0;
const WAIST_HW: f64 = 17.0;
const WAIST_H: f64 = -5.0;
const HAND_OUT: f64 = 5.0;
const HAND_IN: f64 = 5.0;
const HEAD_HW: f64 = 10.0;
const HEAD_H: f64 = 26.0;
const NECK_H: f64 = 5.0;
const SHOULDER_Y: f64 = 0.0;
const UPPER_LENGTH: f64 = 41.0;
const LOWER_LENGTH: f64 = 40.0;
const LOWER_GAP_WRIST: f64 = 1.0;
const LOWER_GAP_ELBOW: f64 = 0.0;
const LOWER_HAND_HEIGHT: f64 = 0.0;
const UPPER_GAP_ELBOW: f64 = 0.0;
const UPPER_GAP_SHOULDER: f64 = 0.0;
const LOWER_TOTAL: f64 = LOWER_LENGTH + LOWER_GAP_WRIST + LOWER_GAP_ELBOW;
const UPPER_TOTAL: f64 = UPPER_LENGTH + UPPER_GAP_ELBOW + UPPER_GAP_SHOULDER;
const DEFAULT_BALL_RADIUS: f64 = 5.0;
const EVENT_BOX_HW_CM: f64 = 5.0;

impl LaidoutPattern {
    pub fn from_jml_pattern(pattern: &MhnJmlPattern) -> Result<Self, String> {
        pattern.assert_valid()?;
        Self::from_jml_pattern_unchecked(pattern)
    }

    pub fn from_jml_pattern_unchecked(pattern: &MhnJmlPattern) -> Result<Self, String> {
        let loop_start_time = pattern.loop_start_time();
        let loop_end_time = pattern.loop_end_time()?;
        let (juggler_position_curves, juggler_angle_curves) =
            build_juggler_curves(pattern, loop_start_time, loop_end_time)?;
        let mut events = pattern
            .all_event_images()?
            .into_iter()
            .map(|image| LayoutEvent {
                global_coordinate: convert_event_local_to_global(
                    &image.event,
                    loop_start_time,
                    loop_end_time,
                    &juggler_position_curves,
                    &juggler_angle_curves,
                ),
                event: image.event,
                primary_index: image.primary_index,
                is_primary: image.is_primary_image,
            })
            .collect::<Vec<_>>();
        events.sort_by(|left, right| {
            left.event
                .t
                .total_cmp(&right.event.t)
                .then(left.event.juggler.cmp(&right.event.juggler))
                .then(left.event.hand.cmp(&right.event.hand))
        });
        let events = merge_coincident_layout_events(events);

        let mut layout = Self {
            number_of_jugglers: pattern.number_of_jugglers,
            number_of_paths: pattern.number_of_paths,
            loop_start_time,
            loop_end_time,
            events,
            path_links: vec![Vec::new(); pattern.number_of_paths],
            hand_links: vec![vec![Vec::new(); 2]; pattern.number_of_jugglers],
            juggler_position_curves,
            juggler_angle_curves,
        };
        layout.build_path_links()?;
        layout.build_hand_links();
        layout.layout_hand_paths()?;
        Ok(layout)
    }

    pub fn path_coordinate(&self, path: usize, time: f64) -> Result<Coordinate, String> {
        if path == 0 || path > self.number_of_paths {
            return Err(format!("Path {path} out of range"));
        }
        let time = self.loop_time(time);
        for link in &self.path_links[path - 1] {
            let start_t = self.events[link.start_event_index].event.t;
            let end_t = self.events[link.end_event_index].event.t;
            if time < start_t || time > end_t {
                continue;
            }

            return match &link.kind {
                PathLinkKind::Toss(path) => path.coordinate_at(time),
                PathLinkKind::Bounce(path) => path.coordinate_at(time),
                PathLinkKind::InHand { juggler, hand } => {
                    self.hand_coordinate(*juggler, *hand, time)
                }
            };
        }

        Err(format!("Time t={time} is out of range for path {path}"))
    }

    pub fn path_trail_coordinates(
        &self,
        path: usize,
        time: f64,
        duration: f64,
        samples: usize,
    ) -> Result<Vec<Coordinate>, String> {
        if path == 0 || path > self.number_of_paths {
            return Err(format!("Path {path} out of range"));
        }
        let time = self.loop_time(time);
        let link = self
            .path_links
            .get(path - 1)
            .and_then(|links| {
                links.iter().find(|link| {
                    let start_t = self.events[link.start_event_index].event.t;
                    let end_t = self.events[link.end_event_index].event.t;
                    time >= start_t && time <= end_t
                })
            })
            .ok_or_else(|| format!("Time t={time} is out of range for path {path}"))?;

        let start_t = self.events[link.start_event_index].event.t;
        let trail_start = (time - duration.max(0.0)).max(start_t);
        let steps = samples.max(1);
        let mut result = Vec::with_capacity(steps + 1);
        for i in 0..=steps {
            let u = i as f64 / steps as f64;
            let sample_time = trail_start + (time - trail_start) * u;
            result.push(self.path_link_coordinate(link, sample_time)?);
        }
        Ok(result)
    }

    pub fn hand_coordinate(
        &self,
        juggler: usize,
        hand: usize,
        time: f64,
    ) -> Result<Coordinate, String> {
        if juggler == 0 || juggler > self.number_of_jugglers || hand > 1 {
            return Err("Hand out of range".to_string());
        }
        let time = self.loop_time(time);
        for link in &self.hand_links[juggler - 1][hand] {
            let start = &self.events[link.start_event_index];
            let end = &self.events[link.end_event_index];
            if time >= start.event.t && time <= end.event.t {
                if let Some(curve) = &link.hand_curve {
                    if let Some(point) = curve.coordinate_at(time) {
                        return Ok(point);
                    }
                }
                return linear_coordinate(
                    start.global_coordinate,
                    start.event.t,
                    end.global_coordinate,
                    end.event.t,
                    time,
                );
            }
        }
        Err(format!(
            "Time t={time} is out of range for hand {juggler}:{}",
            if hand == 0 { "right" } else { "left" }
        ))
    }

    pub fn juggler_position(&self, juggler: usize, time: f64) -> Result<Coordinate, String> {
        if juggler == 0 || juggler > self.number_of_jugglers {
            return Err(format!("Juggler {juggler} out of range"));
        }
        juggler_position_at(
            juggler,
            self.loop_time(time),
            self.loop_start_time,
            self.loop_end_time,
            &self.juggler_position_curves,
        )
        .ok_or_else(|| format!("Juggler {juggler} position is not available"))
    }

    pub fn juggler_angle(&self, juggler: usize, time: f64) -> Result<f64, String> {
        if juggler == 0 || juggler > self.number_of_jugglers {
            return Err(format!("Juggler {juggler} out of range"));
        }
        juggler_angle_at(
            juggler,
            self.loop_time(time),
            self.loop_start_time,
            self.loop_end_time,
            &self.juggler_angle_curves,
        )
        .ok_or_else(|| format!("Juggler {juggler} angle is not available"))
    }

    pub fn convert_global_to_local(
        &self,
        coordinate: Coordinate,
        juggler: usize,
        time: f64,
    ) -> Result<Coordinate, String> {
        let origin = self.juggler_position(juggler, time)?;
        let angle = self.juggler_angle(juggler, time)?.to_radians();
        let relative_x = coordinate.x - origin.x;
        let relative_y = coordinate.y - origin.y;
        Ok(Coordinate {
            x: relative_x * angle.cos() + relative_y * angle.sin(),
            y: -relative_x * angle.sin() + relative_y * angle.cos() - PATTERN_Y,
            z: coordinate.z - origin.z,
        })
    }

    pub fn juggler_frame(&self, juggler: usize, time: f64) -> Result<JugglerFrame, String> {
        let time = self.loop_time(time);
        let left_hand = self.hand_coordinate(juggler, 1, time)?;
        let right_hand = self.hand_coordinate(juggler, 0, time)?;
        let origin = self.juggler_position(juggler, time)?;
        let angle = self.juggler_angle(juggler, time)?.to_radians();
        let s = angle.sin();
        let c = angle.cos();

        let left_hand = coordinate_to_juggler_vector(left_hand);
        let right_hand = coordinate_to_juggler_vector(right_hand);
        let left_shoulder = JugglerVector {
            x: origin.x - SHOULDER_HW * c - SHOULDER_Y * s,
            y: origin.z + SHOULDER_H,
            z: origin.y - SHOULDER_HW * s + SHOULDER_Y * c,
        };
        let right_shoulder = JugglerVector {
            x: origin.x + SHOULDER_HW * c - SHOULDER_Y * s,
            y: origin.z + SHOULDER_H,
            z: origin.y + SHOULDER_HW * s + SHOULDER_Y * c,
        };
        let left_waist = JugglerVector {
            x: origin.x - WAIST_HW * c - SHOULDER_Y * s,
            y: origin.z + WAIST_H,
            z: origin.y - WAIST_HW * s + SHOULDER_Y * c,
        };
        let right_waist = JugglerVector {
            x: origin.x + WAIST_HW * c - SHOULDER_Y * s,
            y: origin.z + WAIST_H,
            z: origin.y + WAIST_HW * s + SHOULDER_Y * c,
        };
        let left_head_bottom = JugglerVector {
            x: origin.x - HEAD_HW * c - SHOULDER_Y * s,
            y: origin.z + SHOULDER_H + NECK_H,
            z: origin.y - HEAD_HW * s + SHOULDER_Y * c,
        };
        let left_head_top = JugglerVector {
            x: origin.x - HEAD_HW * c - SHOULDER_Y * s,
            y: origin.z + SHOULDER_H + NECK_H + HEAD_H,
            z: origin.y - HEAD_HW * s + SHOULDER_Y * c,
        };
        let right_head_bottom = JugglerVector {
            x: origin.x + HEAD_HW * c - SHOULDER_Y * s,
            y: origin.z + SHOULDER_H + NECK_H,
            z: origin.y + HEAD_HW * s + SHOULDER_Y * c,
        };
        let right_head_top = JugglerVector {
            x: origin.x + HEAD_HW * c - SHOULDER_Y * s,
            y: origin.z + SHOULDER_H + NECK_H + HEAD_H,
            z: origin.y + HEAD_HW * s + SHOULDER_Y * c,
        };

        Ok(JugglerFrame {
            left_hand: juggler_vector_to_coordinate(left_hand),
            right_hand: juggler_vector_to_coordinate(right_hand),
            left_shoulder: juggler_vector_to_coordinate(left_shoulder),
            right_shoulder: juggler_vector_to_coordinate(right_shoulder),
            left_elbow: juggler_elbow(left_hand, left_shoulder).map(juggler_vector_to_coordinate),
            right_elbow: juggler_elbow(right_hand, right_shoulder)
                .map(juggler_vector_to_coordinate),
            left_waist: juggler_vector_to_coordinate(left_waist),
            right_waist: juggler_vector_to_coordinate(right_waist),
            left_head_bottom: juggler_vector_to_coordinate(left_head_bottom),
            left_head_top: juggler_vector_to_coordinate(left_head_top),
            right_head_bottom: juggler_vector_to_coordinate(right_head_bottom),
            right_head_top: juggler_vector_to_coordinate(right_head_top),
        })
    }

    pub fn overall_bounds(&self) -> Option<LayoutBounds> {
        let mut pattern_max = None;
        let mut pattern_min = None;
        for path in 1..=self.number_of_paths {
            if let Some(bounds) = self.path_bounds(path) {
                pattern_max = coordinate_option_max(pattern_max, Some(bounds.max));
                pattern_min = coordinate_option_min(pattern_min, Some(bounds.min));
            }
        }

        if pattern_max.is_some() && pattern_min.is_some() {
            let prop_max = Coordinate {
                x: DEFAULT_BALL_RADIUS,
                y: 0.0,
                z: DEFAULT_BALL_RADIUS,
            };
            let prop_min = Coordinate {
                x: -DEFAULT_BALL_RADIUS,
                y: 0.0,
                z: -DEFAULT_BALL_RADIUS,
            };
            pattern_max = pattern_max.map(|max| coordinate_add(max, prop_max));
            pattern_min = pattern_min.map(|min| coordinate_add(min, prop_min));
        }

        let mut hand_max = None;
        let mut hand_min = None;
        for juggler in 1..=self.number_of_jugglers {
            for hand in 0..2 {
                hand_max = coordinate_option_max(hand_max, self.hand_max(juggler, hand));
                hand_min = coordinate_option_min(hand_min, self.hand_min(juggler, hand));
            }
        }
        if hand_max.is_some() && hand_min.is_some() {
            let hand_window_span = HAND_OUT.max(HAND_IN);
            hand_max = hand_max.map(|max| {
                coordinate_add(
                    max,
                    Coordinate {
                        x: hand_window_span,
                        y: hand_window_span,
                        z: 1.0,
                    },
                )
            });
            hand_min = hand_min.map(|min| {
                coordinate_add(
                    min,
                    Coordinate {
                        x: -hand_window_span,
                        y: -hand_window_span,
                        z: -1.0,
                    },
                )
            });
        }

        let juggler_max = self.juggler_window_max();
        let juggler_min = self.juggler_window_min();
        let max = coordinate_option_max(pattern_max, coordinate_option_max(hand_max, juggler_max))?;
        let min = coordinate_option_min(pattern_min, coordinate_option_min(hand_min, juggler_min))?;
        let zoom_center = self.zoom_center_for_bounds(min, max);
        Some(LayoutBounds {
            min,
            max,
            zoom_center,
        })
    }

    pub fn path_bounds(&self, path: usize) -> Option<LayoutBounds> {
        if path == 0 || path > self.number_of_paths {
            return None;
        }
        let mut max = None;
        let mut min = None;
        for link in &self.path_links[path - 1] {
            let (link_min, link_max) = self.path_link_bounds(link)?;
            max = coordinate_option_max(max, Some(link_max));
            min = coordinate_option_min(min, Some(link_min));
        }
        let min = min?;
        let max = max?;
        Some(LayoutBounds {
            min,
            max,
            zoom_center: coordinate_midpoint(min, max),
        })
    }

    fn path_link_bounds(&self, link: &PathLink) -> Option<(Coordinate, Coordinate)> {
        let start_t = self.events[link.start_event_index].event.t;
        let end_t = self.events[link.end_event_index].event.t;
        match &link.kind {
            PathLinkKind::Toss(path) => Some((
                path.min_between(start_t, end_t)?,
                path.max_between(start_t, end_t)?,
            )),
            PathLinkKind::Bounce(path) => Some((
                path.min_between(start_t, end_t)?,
                path.max_between(start_t, end_t)?,
            )),
            PathLinkKind::InHand { juggler, hand } => Some((
                self.hand_min(*juggler, *hand)?,
                self.hand_max(*juggler, *hand)?,
            )),
        }
    }

    fn path_link_coordinate(&self, link: &PathLink, time: f64) -> Result<Coordinate, String> {
        match &link.kind {
            PathLinkKind::Toss(path) => path.coordinate_at(time),
            PathLinkKind::Bounce(path) => path.coordinate_at(time),
            PathLinkKind::InHand { juggler, hand } => self.hand_coordinate(*juggler, *hand, time),
        }
    }

    fn hand_max(&self, juggler: usize, hand: usize) -> Option<Coordinate> {
        let mut result = None;
        for link in self.hand_links.get(juggler.checked_sub(1)?)?.get(hand)? {
            if let Some(curve) = &link.hand_curve {
                result = coordinate_option_max(
                    result,
                    curve.max_between(self.loop_start_time, self.loop_end_time),
                );
            } else {
                result = coordinate_option_max(
                    result,
                    Some(self.events[link.start_event_index].global_coordinate),
                );
                result = coordinate_option_max(
                    result,
                    Some(self.events[link.end_event_index].global_coordinate),
                );
            }
        }
        result
    }

    fn hand_min(&self, juggler: usize, hand: usize) -> Option<Coordinate> {
        let mut result = None;
        for link in self.hand_links.get(juggler.checked_sub(1)?)?.get(hand)? {
            if let Some(curve) = &link.hand_curve {
                result = coordinate_option_min(
                    result,
                    curve.min_between(self.loop_start_time, self.loop_end_time),
                );
            } else {
                result = coordinate_option_min(
                    result,
                    Some(self.events[link.start_event_index].global_coordinate),
                );
                result = coordinate_option_min(
                    result,
                    Some(self.events[link.end_event_index].global_coordinate),
                );
            }
        }
        result
    }

    fn juggler_window_max(&self) -> Option<Coordinate> {
        let mut max = None;
        for curve in &self.juggler_position_curves {
            max = coordinate_option_max(
                max,
                curve.max_between(self.loop_start_time, self.loop_end_time),
            );
        }
        max.map(|max| {
            coordinate_add(
                max,
                Coordinate {
                    x: SHOULDER_HW,
                    y: SHOULDER_HW,
                    z: SHOULDER_H + NECK_H + HEAD_H,
                },
            )
        })
    }

    fn juggler_window_min(&self) -> Option<Coordinate> {
        let mut min = None;
        for curve in &self.juggler_position_curves {
            min = coordinate_option_min(
                min,
                curve.min_between(self.loop_start_time, self.loop_end_time),
            );
        }
        min.map(|min| {
            coordinate_add(
                min,
                Coordinate {
                    x: -SHOULDER_HW,
                    y: -SHOULDER_HW,
                    z: 0.0,
                },
            )
        })
    }

    fn zoom_center_for_bounds(
        &self,
        overall_min: Coordinate,
        overall_max: Coordinate,
    ) -> Coordinate {
        if self.events.is_empty() {
            return Coordinate {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            };
        }

        let mut min_y = f64::INFINITY;
        let mut min_z = f64::INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        let mut max_z = f64::NEG_INFINITY;
        for layout_event in &self.events {
            let mut event = layout_event.event.clone();
            event.x = 0.0;
            event.y = -24.0;
            let global = convert_event_local_to_global(
                &event,
                self.loop_start_time,
                self.loop_end_time,
                &self.juggler_position_curves,
                &self.juggler_angle_curves,
            );
            min_y = min_y.min(global.y);
            max_y = max_y.max(global.y);
            min_z = min_z.min(global.z);
            max_z = max_z.max(global.z);
        }

        let cy = 0.5 * (min_y + max_y);
        let pattern_center_z = 0.5 * (overall_min.z + overall_max.z);
        let event_center_z = 0.5 * (min_z + max_z);
        let cz = if pattern_center_z > event_center_z {
            min_z - 2.0 * EVENT_BOX_HW_CM
        } else {
            max_z + 2.0 * EVENT_BOX_HW_CM
        };
        Coordinate {
            x: 0.0,
            y: cy,
            z: cz,
        }
    }

    fn build_path_links(&mut self) -> Result<(), String> {
        let mut last_event = vec![None; self.number_of_paths];
        let mut last_transition = vec![None; self.number_of_paths];

        for event_index in 0..self.events.len() {
            let mut processed_paths = Vec::new();
            let transitions = self.events[event_index].event.transitions.clone();
            for transition in transitions {
                if transition.path == 0 || transition.path > self.number_of_paths {
                    return Err(format!("Path {} out of range", transition.path));
                }
                if processed_paths.contains(&transition.path) {
                    continue;
                }
                processed_paths.push(transition.path);

                let path_index = transition.path - 1;
                if let (Some(start_event_index), Some(last_transition)) =
                    (last_event[path_index], last_transition[path_index].clone())
                {
                    let link = self.make_path_link(
                        start_event_index,
                        &last_transition,
                        event_index,
                        &transition,
                    )?;
                    self.path_links[path_index].push(link);
                }

                last_event[path_index] = Some(event_index);
                last_transition[path_index] = Some(transition);
            }
        }

        for path in 1..=self.number_of_paths {
            if self.path_links[path - 1].is_empty() {
                return Err(format!("No event found for path {path}"));
            }
        }
        Ok(())
    }

    fn make_path_link(
        &self,
        start_event_index: usize,
        start_transition: &MhnJmlTransition,
        end_event_index: usize,
        end_transition: &MhnJmlTransition,
    ) -> Result<PathLink, String> {
        let start_event = &self.events[start_event_index];
        let end_event = &self.events[end_event_index];
        let kind = match end_transition.transition_type {
            MhnJmlTransitionType::Throw | MhnJmlTransitionType::Holding => {
                if start_transition.transition_type == MhnJmlTransitionType::Throw {
                    return Err(format!(
                        "Due throw successivi sul path {}",
                        end_transition.path
                    ));
                }
                if start_event.event.juggler != end_event.event.juggler {
                    return Err(format!(
                        "The juggler changes while path {} is in hand",
                        end_transition.path
                    ));
                }
                if start_event.event.hand != end_event.event.hand {
                    return Err(format!(
                        "La mano cambia mentre il path {} e' in mano",
                        end_transition.path
                    ));
                }
                PathLinkKind::InHand {
                    juggler: end_event.event.juggler,
                    hand: end_event.event.hand,
                }
            }
            MhnJmlTransitionType::Catch
            | MhnJmlTransitionType::SoftCatch
            | MhnJmlTransitionType::GrabCatch => {
                if start_transition.transition_type != MhnJmlTransitionType::Throw {
                    return Err(format!(
                        "Due catch successivi sul path {}",
                        end_transition.path
                    ));
                }
                let throw_type = start_transition
                    .throw_type
                    .clone()
                    .unwrap_or_else(|| "toss".to_string());
                if throw_type.eq_ignore_ascii_case("toss") {
                    PathLinkKind::Toss(TossPath::new(
                        start_event.global_coordinate,
                        start_event.event.t,
                        end_event.global_coordinate,
                        end_event.event.t,
                        start_transition.throw_mod.as_deref(),
                    )?)
                } else if throw_type.eq_ignore_ascii_case("bounce") {
                    PathLinkKind::Bounce(BouncePath::new(
                        start_event.global_coordinate,
                        start_event.event.t,
                        end_event.global_coordinate,
                        end_event.event.t,
                        start_transition.throw_mod.as_deref(),
                    )?)
                } else {
                    return Err(format!("Unrecognized path type '{throw_type}'"));
                }
            }
        };

        Ok(PathLink {
            start_global_coordinate: start_event.global_coordinate,
            start_event_index,
            end_global_coordinate: end_event.global_coordinate,
            end_event_index,
            kind,
        })
    }

    fn build_hand_links(&mut self) {
        for juggler in 1..=self.number_of_jugglers {
            for hand in 0..2 {
                let mut last_event_index = None;
                let mut last_velocity_ref = None;
                for event_index in 0..self.events.len() {
                    let event = &self.events[event_index].event;
                    if event.juggler != juggler || event.hand != hand {
                        continue;
                    }
                    let velocity_ref = self.velocity_ref_for_event(event_index);
                    if let Some(start_event_index) = last_event_index {
                        self.hand_links[juggler - 1][hand].push(HandLink {
                            juggler,
                            hand,
                            start_event_index,
                            end_event_index: event_index,
                            start_velocity_ref: last_velocity_ref,
                            end_velocity_ref: velocity_ref,
                            hand_curve: None,
                        });
                    }
                    last_event_index = Some(event_index);
                    last_velocity_ref = velocity_ref;
                }
            }
        }
    }

    fn velocity_ref_for_event(&self, event_index: usize) -> Option<VelocityRef> {
        let mut velocity_ref = None;
        for transition in &self.events[event_index].event.transitions {
            match transition.transition_type {
                MhnJmlTransitionType::Throw => {
                    if let Some(velocity) = self
                        .path_link_kind_starting_at(transition.path, event_index)
                        .and_then(path_start_velocity)
                    {
                        velocity_ref = Some(VelocityRef {
                            source: VelocityRefSource::Throw,
                            velocity,
                        });
                    }
                }
                MhnJmlTransitionType::Catch => {
                    if let Some(velocity) = self
                        .path_link_kind_ending_at(transition.path, event_index)
                        .and_then(path_end_velocity)
                    {
                        velocity_ref = Some(VelocityRef {
                            source: VelocityRefSource::Catch,
                            velocity,
                        });
                    }
                }
                MhnJmlTransitionType::SoftCatch => {
                    if let Some(velocity) = self
                        .path_link_kind_ending_at(transition.path, event_index)
                        .and_then(path_end_velocity)
                    {
                        velocity_ref = Some(VelocityRef {
                            source: VelocityRefSource::SoftCatch,
                            velocity,
                        });
                    }
                }
                MhnJmlTransitionType::GrabCatch => {}
                MhnJmlTransitionType::Holding => {}
            }
        }
        velocity_ref
    }

    fn path_link_kind_starting_at(&self, path: usize, event_index: usize) -> Option<&PathLinkKind> {
        self.path_links
            .get(path.checked_sub(1)?)?
            .iter()
            .find(|link| link.start_event_index == event_index)
            .map(|link| &link.kind)
    }

    fn path_link_kind_ending_at(&self, path: usize, event_index: usize) -> Option<&PathLinkKind> {
        self.path_links
            .get(path.checked_sub(1)?)?
            .iter()
            .find(|link| link.end_event_index == event_index)
            .map(|link| &link.kind)
    }

    fn layout_hand_paths(&mut self) -> Result<(), String> {
        for juggler_index in 0..self.number_of_jugglers {
            for hand in 0..2 {
                let has_velocity_defining_transition =
                    self.hand_links[juggler_index][hand].iter().any(|link| {
                        is_velocity_defining(link.start_velocity_ref)
                            || is_velocity_defining(link.end_velocity_ref)
                    });

                if has_velocity_defining_transition {
                    self.layout_hand_paths_with_velocity(juggler_index, hand)?;
                } else {
                    self.layout_hand_paths_without_velocity(juggler_index, hand)?;
                }
            }
        }
        Ok(())
    }

    fn layout_hand_paths_with_velocity(
        &mut self,
        juggler_index: usize,
        hand: usize,
    ) -> Result<(), String> {
        let link_count = self.hand_links[juggler_index][hand].len();
        let mut start_link_index = None;
        let mut chain_len = 0usize;

        for link_index in 0..link_count {
            let start_velocity_ref =
                self.hand_links[juggler_index][hand][link_index].start_velocity_ref;
            if is_velocity_defining(start_velocity_ref) {
                start_link_index = Some(link_index);
                chain_len = 1;
            }

            let end_velocity_ref =
                self.hand_links[juggler_index][hand][link_index].end_velocity_ref;
            if let Some(start_index) = start_link_index {
                if is_velocity_defining(end_velocity_ref) {
                    let end_index = link_index;
                    let curve = self.build_hand_spline_with_velocity(
                        juggler_index,
                        hand,
                        start_index,
                        end_index,
                    )?;
                    self.assign_hand_curve(juggler_index, hand, start_index, end_index, curve);
                    start_link_index = None;
                }
            }

            chain_len += 1;
            if chain_len > link_count + 1 {
                return Err("layoutHandPaths(): catena mano non valida".to_string());
            }
        }
        Ok(())
    }

    fn build_hand_spline_with_velocity(
        &self,
        juggler_index: usize,
        hand: usize,
        start_index: usize,
        end_index: usize,
    ) -> Result<Curve, String> {
        let num = end_index - start_index + 1;
        let mut times = Vec::with_capacity(num + 1);
        let mut positions = Vec::with_capacity(num + 1);
        let mut velocities = vec![None; num + 1];

        for offset in 0..num {
            let link = &self.hand_links[juggler_index][hand][start_index + offset];
            let start_event = &self.events[link.start_event_index];
            times.push(start_event.event.t);
            positions.push(start_event.global_coordinate);
            if offset > 0 {
                if let Some(velocity_ref) = link.start_velocity_ref {
                    if velocity_ref.source == VelocityRefSource::Catch {
                        velocities[offset] = Some(velocity_ref.velocity);
                    }
                }
            }
        }

        let end_link = &self.hand_links[juggler_index][hand][end_index];
        let end_event = &self.events[end_link.end_event_index];
        times.push(end_event.event.t);
        positions.push(end_event.global_coordinate);
        velocities[0] = self.hand_links[juggler_index][hand][start_index]
            .start_velocity_ref
            .map(|velocity_ref| velocity_ref.velocity);
        velocities[num] = end_link
            .end_velocity_ref
            .map(|velocity_ref| velocity_ref.velocity);

        Curve::spline(times, positions, velocities)
    }

    fn layout_hand_paths_without_velocity(
        &mut self,
        juggler_index: usize,
        hand: usize,
    ) -> Result<(), String> {
        let link_count = self.hand_links[juggler_index][hand].len();
        if link_count == 0 {
            return Ok(());
        }

        let Some(mut link_index) = self.hand_links[juggler_index][hand]
            .iter()
            .position(|link| self.events[link.end_event_index].event.t > self.loop_start_time)
        else {
            return Ok(());
        };

        for _chain in 0..2 {
            if link_index >= link_count {
                break;
            }

            let start_index = link_index;
            let start_event_index =
                self.hand_links[juggler_index][hand][start_index].start_event_index;
            while !self.is_delay_of(
                self.hand_links[juggler_index][hand][link_index].end_event_index,
                start_event_index,
            ) {
                link_index += 1;
                if link_index >= link_count {
                    return Err("layoutHandPaths(): delay event mano non trovato".to_string());
                }
            }

            let end_index = link_index;
            let curve = self.build_hand_spline_without_velocity(
                juggler_index,
                hand,
                start_index,
                end_index,
            )?;
            self.assign_hand_curve(juggler_index, hand, start_index, end_index, curve);
            link_index += 1;
        }

        Ok(())
    }

    fn build_hand_spline_without_velocity(
        &self,
        juggler_index: usize,
        hand: usize,
        start_index: usize,
        end_index: usize,
    ) -> Result<Curve, String> {
        let num = end_index - start_index + 1;
        let mut times = Vec::with_capacity(num + 1);
        let mut positions = Vec::with_capacity(num + 1);

        for offset in 0..num {
            let link = &self.hand_links[juggler_index][hand][start_index + offset];
            let start_event = &self.events[link.start_event_index];
            times.push(start_event.event.t);
            positions.push(start_event.global_coordinate);
        }

        let end_link = &self.hand_links[juggler_index][hand][end_index];
        let end_event = &self.events[end_link.end_event_index];
        times.push(end_event.event.t);
        positions.push(end_event.global_coordinate);
        let velocities = vec![None; times.len()];

        Curve::spline(times, positions, velocities)
    }

    fn assign_hand_curve(
        &mut self,
        juggler_index: usize,
        hand: usize,
        start_index: usize,
        end_index: usize,
        curve: Curve,
    ) {
        for link in &mut self.hand_links[juggler_index][hand][start_index..=end_index] {
            link.hand_curve = Some(curve.clone());
        }
    }

    fn is_delay_of(&self, event_index: usize, other_event_index: usize) -> bool {
        let event = &self.events[event_index];
        let other = &self.events[other_event_index];
        event.primary_index == other.primary_index
            && event.event.juggler == other.event.juggler
            && event.event.hand == other.event.hand
    }

    fn loop_time(&self, mut time: f64) -> f64 {
        let period = self.loop_end_time - self.loop_start_time;
        while time < self.loop_start_time {
            time += period;
        }
        while time >= self.loop_end_time {
            time -= period;
        }
        time
    }
}

fn merge_coincident_layout_events(events: Vec<LayoutEvent>) -> Vec<LayoutEvent> {
    let mut merged: Vec<LayoutEvent> = Vec::with_capacity(events.len());

    for event in events {
        if let Some(last) = merged
            .last_mut()
            .filter(|last| layout_events_are_coincident(last, &event))
        {
            if last.event.calcpos && !event.event.calcpos {
                last.event.x = event.event.x;
                last.event.y = event.event.y;
                last.event.z = event.event.z;
                last.event.calcpos = false;
                last.global_coordinate = event.global_coordinate;
            }
            if !last.is_primary && event.is_primary {
                last.primary_index = event.primary_index;
                last.is_primary = true;
            }
            last.event.transitions.extend(event.event.transitions);
        } else {
            merged.push(event);
        }
    }

    merged
}

fn layout_events_are_coincident(left: &LayoutEvent, right: &LayoutEvent) -> bool {
    left.event.juggler == right.event.juggler
        && left.event.hand == right.event.hand
        && (left.event.t - right.event.t).abs() < 1e-9
}

fn coordinate_to_juggler_vector(coordinate: Coordinate) -> JugglerVector {
    JugglerVector {
        x: coordinate.x,
        y: coordinate.z + LOWER_HAND_HEIGHT,
        z: coordinate.y,
    }
}

fn juggler_vector_to_coordinate(vector: JugglerVector) -> Coordinate {
    Coordinate {
        x: vector.x,
        y: vector.z,
        z: vector.y,
    }
}

fn juggler_vector_sub(left: JugglerVector, right: JugglerVector) -> JugglerVector {
    JugglerVector {
        x: left.x - right.x,
        y: left.y - right.y,
        z: left.z - right.z,
    }
}

fn juggler_vector_scale(factor: f64, vector: JugglerVector) -> JugglerVector {
    JugglerVector {
        x: factor * vector.x,
        y: factor * vector.y,
        z: factor * vector.z,
    }
}

fn juggler_vector_length(vector: JugglerVector) -> f64 {
    (vector.x * vector.x + vector.y * vector.y + vector.z * vector.z).sqrt()
}

fn juggler_elbow(hand: JugglerVector, shoulder: JugglerVector) -> Option<JugglerVector> {
    let lower = LOWER_TOTAL;
    let upper = UPPER_TOTAL;
    let delta = juggler_vector_sub(hand, shoulder);
    let distance = juggler_vector_length(delta);
    if distance > lower + upper || distance <= f64::EPSILON {
        return None;
    }

    let radius_numerator = 4.0 * upper * upper * lower * lower
        - (upper * upper + lower * lower - distance * distance).powi(2);
    let radius = (radius_numerator / (4.0 * distance * distance)).sqrt();
    if !radius.is_finite() {
        return None;
    }

    let mut factor = (upper * upper - radius * radius).sqrt() / distance;
    if !factor.is_finite() || factor.abs() <= f64::EPSILON {
        return None;
    }

    let scaled = juggler_vector_scale(factor, delta);
    let alpha = (delta.y / distance).asin();
    if !alpha.is_finite() {
        return None;
    }

    factor = 1.0 + radius * alpha.tan() / (factor * distance);
    Some(JugglerVector {
        x: shoulder.x + scaled.x * factor,
        y: shoulder.y + scaled.y - radius * alpha.cos(),
        z: shoulder.z + scaled.z * factor,
    })
}

fn coordinate_add(left: Coordinate, right: Coordinate) -> Coordinate {
    Coordinate {
        x: left.x + right.x,
        y: left.y + right.y,
        z: left.z + right.z,
    }
}

fn coordinate_midpoint(left: Coordinate, right: Coordinate) -> Coordinate {
    Coordinate {
        x: 0.5 * (left.x + right.x),
        y: 0.5 * (left.y + right.y),
        z: 0.5 * (left.z + right.z),
    }
}

fn coordinate_max(left: Coordinate, right: Coordinate) -> Coordinate {
    Coordinate {
        x: left.x.max(right.x),
        y: left.y.max(right.y),
        z: left.z.max(right.z),
    }
}

fn coordinate_min(left: Coordinate, right: Coordinate) -> Coordinate {
    Coordinate {
        x: left.x.min(right.x),
        y: left.y.min(right.y),
        z: left.z.min(right.z),
    }
}

fn coordinate_option_max(
    left: Option<Coordinate>,
    right: Option<Coordinate>,
) -> Option<Coordinate> {
    match (left, right) {
        (Some(left), Some(right)) => Some(coordinate_max(left, right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn coordinate_option_min(
    left: Option<Coordinate>,
    right: Option<Coordinate>,
) -> Option<Coordinate> {
    match (left, right) {
        (Some(left), Some(right)) => Some(coordinate_min(left, right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

impl TossPath {
    pub fn new(
        start: Coordinate,
        start_time: f64,
        end: Coordinate,
        end_time: f64,
        modifier: Option<&str>,
    ) -> Result<Self, String> {
        let gravity = parse_gravity(modifier)?;
        let duration = end_time - start_time;
        if duration <= 0.0 {
            return Err("TossPath duration non positiva".to_string());
        }
        let az = -0.5 * gravity;
        let cx = start.x;
        let bx = (end.x - start.x) / duration;
        let cy = start.y;
        let by = (end.y - start.y) / duration;
        let cz = start.z;
        let bz = (end.z - start.z) / duration - az * duration;

        Ok(Self {
            start,
            end,
            start_time,
            end_time,
            gravity,
            bx,
            cx,
            by,
            cy,
            az,
            bz,
            cz,
        })
    }

    pub fn coordinate_at(&self, time: f64) -> Result<Coordinate, String> {
        if time < self.start_time || time > self.end_time {
            return Err(format!("time t={time} fuori TossPath"));
        }
        let t = time - self.start_time;
        Ok(Coordinate {
            x: self.cx + self.bx * t,
            y: self.cy + self.by * t,
            z: self.cz + t * (self.bz + self.az * t),
        })
    }

    pub fn max_between(&self, time1: f64, time2: f64) -> Option<Coordinate> {
        self.extreme_between(time1, time2, true)
    }

    pub fn min_between(&self, time1: f64, time2: f64) -> Option<Coordinate> {
        self.extreme_between(time1, time2, false)
    }

    fn extreme_between(&self, time1: f64, time2: f64, find_max: bool) -> Option<Coordinate> {
        if time2 < self.start_time || time1 > self.end_time {
            return None;
        }
        let tlow = self.start_time.max(time1);
        let thigh = self.end_time.min(time2);
        let mut result = self.check_extreme(None, tlow, find_max);
        result = self.check_extreme(result, thigh, find_max);

        if self.az.abs() > f64::EPSILON {
            let vertex_time = self.start_time - self.bz / (2.0 * self.az);
            if vertex_time >= tlow && vertex_time <= thigh {
                result = self.check_extreme(result, vertex_time, find_max);
            }
        }
        result
    }

    fn check_extreme(
        &self,
        result: Option<Coordinate>,
        time: f64,
        find_max: bool,
    ) -> Option<Coordinate> {
        let coordinate = self.coordinate_at(time).ok()?;
        Some(match result {
            None => coordinate,
            Some(result) if find_max => coordinate_max(result, coordinate),
            Some(result) => coordinate_min(result, coordinate),
        })
    }

    pub fn start_velocity(&self) -> Coordinate {
        Coordinate {
            x: self.bx,
            y: self.by,
            z: self.bz,
        }
    }

    pub fn end_velocity(&self) -> Coordinate {
        Coordinate {
            x: self.bx,
            y: self.by,
            z: self.bz + 2.0 * self.az * (self.end_time - self.start_time),
        }
    }
}

impl BouncePath {
    const BOUNCES_DEFAULT: usize = 1;
    const FORCED_DEFAULT: bool = false;
    const HYPER_DEFAULT: bool = false;
    const BOUNCEPLANE_DEFAULT: f64 = 0.0;
    const BOUNCEFRAC_DEFAULT: f64 = 0.9;
    const GRAVITY_DEFAULT: f64 = 980.0;

    pub fn new(
        start: Coordinate,
        start_time: f64,
        end: Coordinate,
        end_time: f64,
        modifier: Option<&str>,
    ) -> Result<Self, String> {
        let params = BouncePathParameters::parse(modifier)?;
        let mut path = Self {
            start,
            end,
            start_time,
            end_time,
            bounces: params.bounces,
            forced: params.forced,
            hyper: params.hyper,
            bounceplane: params.bounceplane,
            bouncefrac: params.bouncefrac,
            gravity: params.gravity,
            bouncefracsqrt: params.bouncefrac.sqrt(),
            numbounces: 0,
            bx: 0.0,
            cx: 0.0,
            by: 0.0,
            cy: 0.0,
            az: vec![-0.5 * params.gravity; params.bounces + 1],
            bz: vec![0.0; params.bounces + 1],
            cz: vec![0.0; params.bounces + 1],
            endtime: vec![0.0; params.bounces + 1],
        };
        path.calc_path()?;
        Ok(path)
    }

    fn calc_path(&mut self) -> Result<(), String> {
        let duration = self.end_time - self.start_time;
        if duration <= 0.0 {
            return Err("BouncePath duration non positiva".to_string());
        }

        for n in (1..=self.bounces).rev() {
            let roots = self.solve_bounce_equation(n, duration);
            if roots.is_empty() {
                continue;
            }

            let mut chosen = None;
            for (root, liftcatch) in &roots {
                if self.forced ^ (*root < 0.0) {
                    continue;
                }
                if self.hyper ^ *liftcatch ^ self.forced {
                    continue;
                }
                chosen = Some(*root);
                break;
            }
            if chosen.is_none() {
                for (root, _) in &roots {
                    if self.forced ^ (*root < 0.0) {
                        continue;
                    }
                    chosen = Some(*root);
                    break;
                }
            }
            if chosen.is_none() {
                for (root, liftcatch) in &roots {
                    if self.hyper ^ *liftcatch ^ (*root < 0.0) {
                        continue;
                    }
                    chosen = Some(*root);
                    break;
                }
            }

            let v0 = chosen.unwrap_or(roots[0].0);
            self.numbounces = n;
            self.bz[0] = v0;
            self.cz[0] = self.start.z;
            let disc = v0 * v0 - 4.0 * self.az[0] * (self.cz[0] - self.bounceplane);
            self.endtime[0] = if self.az[0] < 0.0 {
                (-v0 - disc.sqrt()) / (2.0 * self.az[0])
            } else {
                (-v0 + disc.sqrt()) / (2.0 * self.az[0])
            };
            let mut vrebound = (-v0 - 2.0 * self.az[0] * self.endtime[0]) * self.bouncefracsqrt;

            for i in 1..=n {
                self.bz[i] = vrebound - 2.0 * self.az[i] * self.endtime[i - 1];
                self.cz[i] = self.bounceplane
                    - self.az[i] * self.endtime[i - 1] * self.endtime[i - 1]
                    - self.bz[i] * self.endtime[i - 1];
                self.endtime[i] = self.endtime[i - 1] - vrebound / self.az[i];
                vrebound *= self.bouncefracsqrt;
            }
            self.endtime[n] = duration;

            self.cx = self.start.x;
            self.bx = (self.end.x - self.start.x) / duration;
            self.cy = self.start.y;
            self.by = (self.end.y - self.start.y) / duration;
            return Ok(());
        }

        Err("No root found in BouncePath".to_string())
    }

    fn solve_bounce_equation(&self, n: usize, duration: f64) -> Vec<(f64, bool)> {
        let mut f1 = 1.0;
        for _ in 0..n {
            f1 *= self.bouncefracsqrt;
        }
        let k = if self.bouncefracsqrt == 1.0 {
            2.0 * n as f64
        } else {
            1.0 + f1
                + 2.0 * self.bouncefracsqrt * (1.0 - f1 / self.bouncefracsqrt)
                    / (1.0 - self.bouncefracsqrt)
        };
        let u = 2.0 * self.gravity * (self.start.z - self.bounceplane);
        let l = 2.0 * self.gravity * (self.end.z - self.bounceplane);
        let f2 = f1 * f1;
        let c = u - l / f2;
        let kk = k * k;
        let gt = self.gravity * duration;

        let mut coef = vec![0.0; 5];
        coef[4] = 1.0 + kk * kk + f2 * f2 - 2.0 * kk - 2.0 * f2 - 2.0 * kk * f2;
        coef[3] = -4.0 * gt + 4.0 * f2 * gt + 4.0 * kk * gt;
        coef[2] = (6.0 * gt * gt + 2.0 * kk * kk * u + 2.0 * f2 * f2 * c)
            - 2.0 * f2 * c
            - 2.0 * f2 * gt * gt
            - 2.0 * kk * gt * gt
            - 2.0 * kk * u
            - 2.0 * kk * f2 * c
            - 2.0 * kk * f2 * u;
        coef[1] = -4.0 * gt * gt * gt + 4.0 * f2 * gt * c + 4.0 * kk * gt * u;
        coef[0] = (gt * gt * gt * gt + kk * kk * u * u + f2 * f2 * c * c)
            - 2.0 * gt * gt * f2 * c
            - 2.0 * kk * gt * gt * u
            - 2.0 * kk * f2 * u * c;

        let real_roots = if n > 1 {
            let leading = coef[4];
            for value in coef.iter_mut().take(4) {
                *value /= leading;
            }
            find_real_roots_polynomial(&coef, 4)
        } else {
            let leading = coef[3];
            for value in coef.iter_mut().take(3) {
                *value /= leading;
            }
            find_real_roots_polynomial(&coef, 3)
        };

        let mut roots = Vec::new();
        for v0 in real_roots {
            if v0 * v0 + c >= 0.0 {
                let liftcatch = gt - v0 - k * (v0 * v0 + u).sqrt() > 0.0;
                roots.push((v0, liftcatch));
            }
        }
        roots
    }

    pub fn coordinate_at(&self, time: f64) -> Result<Coordinate, String> {
        if time < self.start_time || time > self.end_time {
            return Err(format!("time t={time} fuori BouncePath"));
        }
        let t = time - self.start_time;
        let mut z = 0.0;
        for i in 0..=self.numbounces {
            if t < self.endtime[i] || i == self.numbounces {
                z = self.cz[i] + t * (self.bz[i] + self.az[i] * t);
                break;
            }
        }
        Ok(Coordinate {
            x: self.cx + self.bx * t,
            y: self.cy + self.by * t,
            z,
        })
    }

    pub fn max_between(&self, time1: f64, time2: f64) -> Option<Coordinate> {
        self.extreme_between(time1, time2, true)
    }

    pub fn min_between(&self, time1: f64, time2: f64) -> Option<Coordinate> {
        self.extreme_between(time1, time2, false)
    }

    fn extreme_between(&self, time1: f64, time2: f64, find_max: bool) -> Option<Coordinate> {
        if time2 < self.start_time || time1 > self.end_time {
            return None;
        }
        let tlow = self.start_time.max(time1);
        let thigh = self.end_time.min(time2);
        let mut result = self.check_extreme(None, tlow, find_max);
        result = self.check_extreme(result, thigh, find_max);

        for segment in 0..=self.numbounces {
            let segment_start = if segment == 0 {
                0.0
            } else {
                self.endtime[segment - 1]
            };
            let segment_end = self.endtime[segment];
            for local_time in [segment_start, segment_end] {
                let global_time = self.start_time + local_time;
                if global_time >= tlow && global_time <= thigh {
                    result = self.check_extreme(result, global_time, find_max);
                }
            }

            if self.az[segment].abs() > f64::EPSILON {
                let vertex_local_time = -self.bz[segment] / (2.0 * self.az[segment]);
                let vertex_global_time = self.start_time + vertex_local_time;
                if vertex_local_time >= segment_start
                    && vertex_local_time <= segment_end
                    && vertex_global_time >= tlow
                    && vertex_global_time <= thigh
                {
                    result = self.check_extreme(result, vertex_global_time, find_max);
                }
            }
        }
        result
    }

    fn check_extreme(
        &self,
        result: Option<Coordinate>,
        time: f64,
        find_max: bool,
    ) -> Option<Coordinate> {
        let coordinate = self.coordinate_at(time).ok()?;
        Some(match result {
            None => coordinate,
            Some(result) if find_max => coordinate_max(result, coordinate),
            Some(result) => coordinate_min(result, coordinate),
        })
    }

    pub fn start_velocity(&self) -> Coordinate {
        Coordinate {
            x: self.bx,
            y: self.by,
            z: self.bz[0],
        }
    }

    pub fn end_velocity(&self) -> Coordinate {
        Coordinate {
            x: self.bx,
            y: self.by,
            z: self.bz[self.numbounces]
                + 2.0 * self.az[self.numbounces] * (self.end_time - self.start_time),
        }
    }

    pub fn bounce_volume(&self, time1: f64, time2: f64) -> f64 {
        if time2 < self.start_time || time1 > self.end_time {
            return 0.0;
        }
        let t1 = time1 - self.start_time;
        let t2 = time2 - self.start_time;
        for i in 0..self.numbounces {
            if t1 < self.endtime[i] {
                return if t2 > self.endtime[i] { 1.0 } else { 0.0 };
            }
        }
        0.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct BouncePathParameters {
    bounces: usize,
    forced: bool,
    hyper: bool,
    bounceplane: f64,
    bouncefrac: f64,
    gravity: f64,
}

impl BouncePathParameters {
    fn parse(modifier: Option<&str>) -> Result<Self, String> {
        let mut params = Self {
            bounces: BouncePath::BOUNCES_DEFAULT,
            forced: BouncePath::FORCED_DEFAULT,
            hyper: BouncePath::HYPER_DEFAULT,
            bounceplane: BouncePath::BOUNCEPLANE_DEFAULT,
            bouncefrac: BouncePath::BOUNCEFRAC_DEFAULT,
            gravity: BouncePath::GRAVITY_DEFAULT,
        };
        let list = ParameterList::parse(modifier)?;

        for (name, value) in list.iter() {
            if name.eq_ignore_ascii_case("bounces") {
                params.bounces = value
                    .parse::<usize>()
                    .map_err(|_| "Invalid number for bounces".to_string())?;
            } else if name.eq_ignore_ascii_case("forced") {
                params.forced = value.eq_ignore_ascii_case("true");
            } else if name.eq_ignore_ascii_case("hyper") {
                params.hyper = value.eq_ignore_ascii_case("true");
            } else if name.eq_ignore_ascii_case("bounceplane") {
                params.bounceplane = parse_finite_parameter(value, "bounceplane")?;
            } else if name.eq_ignore_ascii_case("bouncefrac") {
                params.bouncefrac = parse_finite_parameter(value, "bouncefrac")?;
            } else if name.eq_ignore_ascii_case("g") {
                params.gravity = parse_finite_parameter(value, "g")?;
            } else {
                return Err(format!("Unrecognized path modifier: '{name}'"));
            }
        }

        Ok(params)
    }
}

fn build_juggler_curves(
    pattern: &MhnJmlPattern,
    loop_start_time: f64,
    loop_end_time: f64,
) -> Result<(Vec<Curve>, Vec<Curve>), String> {
    let mut position_curves = Vec::with_capacity(pattern.number_of_jugglers);
    let mut angle_curves = Vec::with_capacity(pattern.number_of_jugglers);
    let loop_duration = loop_end_time - loop_start_time;

    for juggler in 1..=pattern.number_of_jugglers {
        let mut body_positions = pattern
            .positions
            .iter()
            .filter(|position| position.juggler == juggler)
            .copied()
            .collect::<Vec<_>>();
        body_positions.sort_by(|left, right| left.t.total_cmp(&right.t));

        if body_positions.is_empty() {
            let position = default_juggler_position(juggler, pattern.number_of_jugglers);
            let angle = default_juggler_angle(juggler, pattern.number_of_jugglers);
            let times = vec![loop_start_time, loop_end_time];
            position_curves.push(Curve::spline(
                times.clone(),
                vec![position, position],
                vec![None, None],
            )?);
            angle_curves.push(Curve::line(
                times,
                vec![
                    Coordinate {
                        x: angle,
                        y: 0.0,
                        z: 0.0,
                    },
                    Coordinate {
                        x: angle,
                        y: 0.0,
                        z: 0.0,
                    },
                ],
                vec![None, None],
            )?);
            continue;
        }

        let mut times = Vec::with_capacity(body_positions.len() + 1);
        let mut positions = Vec::with_capacity(body_positions.len() + 1);
        let mut angles = Vec::with_capacity(body_positions.len() + 1);
        for position in &body_positions {
            times.push(position.t);
            positions.push(Coordinate {
                x: position.x,
                y: position.y,
                z: position.z,
            });
            angles.push(Coordinate {
                x: position.angle,
                y: 0.0,
                z: 0.0,
            });
        }

        times.push(times[0] + loop_duration);
        positions.push(positions[0]);
        angles.push(angles[0]);

        for index in 1..angles.len() {
            while angles[index].x - angles[index - 1].x > 180.0 {
                angles[index].x -= 360.0;
            }
            while angles[index].x - angles[index - 1].x < -180.0 {
                angles[index].x += 360.0;
            }
        }

        let velocities = vec![None; times.len()];
        position_curves.push(Curve::spline(times.clone(), positions, velocities.clone())?);
        angle_curves.push(Curve::line(times, angles, velocities)?);
    }

    Ok((position_curves, angle_curves))
}

fn convert_event_local_to_global(
    event: &MhnJmlEvent,
    loop_start_time: f64,
    loop_end_time: f64,
    position_curves: &[Curve],
    angle_curves: &[Curve],
) -> Coordinate {
    let origin = juggler_position_at(
        event.juggler,
        event.t,
        loop_start_time,
        loop_end_time,
        position_curves,
    )
    .unwrap_or_else(|| default_juggler_position(event.juggler, position_curves.len()));
    let angle = juggler_angle_at(
        event.juggler,
        event.t,
        loop_start_time,
        loop_end_time,
        angle_curves,
    )
    .unwrap_or_else(|| default_juggler_angle(event.juggler, angle_curves.len()))
    .to_radians();
    let local_y = event.y + PATTERN_Y;
    Coordinate {
        x: origin.x + event.x * angle.cos() - local_y * angle.sin(),
        y: origin.y + event.x * angle.sin() + local_y * angle.cos(),
        z: origin.z + event.z,
    }
}

fn juggler_position_at(
    juggler: usize,
    time: f64,
    loop_start_time: f64,
    loop_end_time: f64,
    curves: &[Curve],
) -> Option<Coordinate> {
    curves
        .get(juggler.checked_sub(1)?)?
        .coordinate_at(curve_loop_time(
            time,
            curves[juggler - 1].start_time(),
            curves[juggler - 1].end_time(),
            loop_end_time - loop_start_time,
        ))
}

fn juggler_angle_at(
    juggler: usize,
    time: f64,
    loop_start_time: f64,
    loop_end_time: f64,
    curves: &[Curve],
) -> Option<f64> {
    curves
        .get(juggler.checked_sub(1)?)?
        .coordinate_at(curve_loop_time(
            time,
            curves[juggler - 1].start_time(),
            curves[juggler - 1].end_time(),
            loop_end_time - loop_start_time,
        ))
        .map(|coordinate| coordinate.x)
}

fn curve_loop_time(mut time: f64, start_time: f64, end_time: f64, period: f64) -> f64 {
    if period <= 0.0 {
        return time;
    }
    while time < start_time {
        time += period;
    }
    while time > end_time {
        time -= period;
    }
    time
}

fn is_velocity_defining(velocity_ref: Option<VelocityRef>) -> bool {
    matches!(
        velocity_ref.map(|reference| reference.source),
        Some(VelocityRefSource::Throw | VelocityRefSource::SoftCatch)
    )
}

fn path_start_velocity(kind: &PathLinkKind) -> Option<Coordinate> {
    match kind {
        PathLinkKind::Toss(path) => Some(path.start_velocity()),
        PathLinkKind::Bounce(path) => Some(path.start_velocity()),
        PathLinkKind::InHand { .. } => None,
    }
}

fn path_end_velocity(kind: &PathLinkKind) -> Option<Coordinate> {
    match kind {
        PathLinkKind::Toss(path) => Some(path.end_velocity()),
        PathLinkKind::Bounce(path) => Some(path.end_velocity()),
        PathLinkKind::InHand { .. } => None,
    }
}

fn parse_finite_parameter(value: &str, name: &str) -> Result<f64, String> {
    let parsed = value
        .parse::<f64>()
        .map_err(|_| format!("Invalid number for {name}"))?;
    if parsed.is_finite() {
        Ok(parsed)
    } else {
        Err(format!("Non-finite number for {name}"))
    }
}

fn eval_polynomial(coef: &[f64], degree: usize, x: f64) -> f64 {
    let mut result = coef[0];
    let mut term = x;
    for value in coef.iter().take(degree).skip(1) {
        result += *value * term;
        term *= x;
    }
    result + term
}

fn bracket_open_interval(coef: &[f64], degree: usize, endpoint: f64, pinf: bool) -> f64 {
    let endpoint_positive = eval_polynomial(coef, degree, endpoint) > 0.0;
    let mut result = endpoint;
    let mut adder = if pinf { 1.0 } else { -1.0 };

    while (eval_polynomial(coef, degree, result) > 0.0) == endpoint_positive {
        result += adder;
        adder *= 2.0;
    }

    result
}

fn find_root(coef: &[f64], degree: usize, mut xlow: f64, mut xhigh: f64) -> f64 {
    let mut val1 = eval_polynomial(coef, degree, xlow);
    let val2 = eval_polynomial(coef, degree, xhigh);
    if val1 * val2 > 0.0 {
        return 0.5 * (xlow + xhigh);
    }

    while (xlow - xhigh).abs() > 1e-6 {
        let t = 0.5 * (xlow + xhigh);
        let val_temp = eval_polynomial(coef, degree, t);
        if val_temp * val1 > 0.0 {
            xlow = t;
            val1 = val_temp;
        } else {
            xhigh = t;
        }
    }
    xlow
}

fn find_real_roots_polynomial(coef: &[f64], degree: usize) -> Vec<f64> {
    if degree == 0 {
        return Vec::new();
    }
    if degree == 1 {
        return vec![-coef[0]];
    }
    if degree == 2 {
        let disc = coef[1] * coef[1] - 4.0 * coef[0];
        if disc < 0.0 {
            return Vec::new();
        }
        if disc == 0.0 {
            return vec![-0.5 * coef[1]];
        }
        let t = disc.sqrt();
        return vec![-0.5 * (coef[1] + t), -0.5 * (coef[1] - t)];
    }
    if degree == 3 {
        let q = coef[2] * coef[2] / 9.0 - coef[1] / 3.0;
        let r = coef[2] * coef[2] * coef[2] / 27.0 - coef[1] * coef[2] / 6.0 + coef[0] / 2.0;
        let disc = r * r - q * q * q;

        if disc > 0.0 {
            let k = (disc.sqrt() + r.abs()).powf(1.0 / 3.0);
            return vec![(if r > 0.0 { -(k + q / k) } else { k + q / k }) - coef[2] / 3.0];
        }

        let theta = (r / (q * q * q).sqrt()).acos() / 3.0;
        let k = -2.0 * q.sqrt();
        let p = 2.0 * std::f64::consts::PI / 3.0;
        return vec![
            k * theta.cos() - coef[2] / 3.0,
            k * (theta + p).cos() - coef[2] / 3.0,
            k * (theta + 2.0 * p).cos() - coef[2] / 3.0,
        ];
    }

    let mut dcoef = vec![0.0; degree - 1];
    for i in 0..(degree - 1) {
        dcoef[i] = (i + 1) as f64 * coef[i + 1] / degree as f64;
    }
    let mut extrema = find_real_roots_polynomial(&dcoef, degree - 1);

    let pinf_positive = true;
    let minf_positive = degree % 2 == 0;
    let mut roots = Vec::new();

    if extrema.is_empty() {
        let zero_positive = coef[0] > 0.0;
        if zero_positive != pinf_positive {
            let endpoint2 = bracket_open_interval(coef, degree, 0.0, true);
            roots.push(find_root(coef, degree, 0.0, endpoint2));
        }
        if zero_positive != minf_positive {
            let endpoint2 = bracket_open_interval(coef, degree, 0.0, false);
            roots.push(find_root(coef, degree, endpoint2, 0.0));
        }
        return roots;
    }

    extrema.sort_by(f64::total_cmp);
    let extremum_positive = extrema
        .iter()
        .map(|extremum| eval_polynomial(coef, degree, *extremum) > 0.0)
        .collect::<Vec<_>>();

    if minf_positive != extremum_positive[0] {
        let endpoint2 = bracket_open_interval(coef, degree, extrema[0], false);
        roots.push(find_root(coef, degree, endpoint2, extrema[0]));
    }

    for i in 0..(extrema.len() - 1) {
        if extremum_positive[i] != extremum_positive[i + 1] {
            roots.push(find_root(coef, degree, extrema[i], extrema[i + 1]));
        }
    }

    if pinf_positive != extremum_positive[extrema.len() - 1] {
        let endpoint2 = bracket_open_interval(coef, degree, extrema[extrema.len() - 1], true);
        roots.push(find_root(
            coef,
            degree,
            extrema[extrema.len() - 1],
            endpoint2,
        ));
    }

    roots
}

fn parse_gravity(modifier: Option<&str>) -> Result<f64, String> {
    let Some(modifier) = modifier else {
        return Ok(TOSS_GRAVITY_DEFAULT);
    };
    let params = ParameterList::parse(Some(modifier))?;
    Ok(params
        .get_parameter("g")
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite())
        .unwrap_or(TOSS_GRAVITY_DEFAULT))
}

fn linear_coordinate(
    start: Coordinate,
    start_time: f64,
    end: Coordinate,
    end_time: f64,
    time: f64,
) -> Result<Coordinate, String> {
    let duration = (end_time - start_time).max(0.001);
    let u = ((time - start_time) / duration).clamp(0.0, 1.0);
    Ok(Coordinate {
        x: start.x + (end.x - start.x) * u,
        y: start.y + (end.y - start.y) * u,
        z: start.z + (end.z - start.z) * u,
    })
}

fn default_juggler_position(juggler: usize, number_of_jugglers: usize) -> Coordinate {
    if number_of_jugglers <= 1 {
        return Coordinate {
            x: 0.0,
            y: 0.0,
            z: 100.0,
        };
    }

    let theta = 360.0 / number_of_jugglers as f64;
    let half_angle = (0.5 * theta).to_radians();
    let mut radius = 70.0;
    if radius * half_angle.sin() < 65.0 {
        radius = 65.0 / half_angle.sin();
    }
    let angle = (theta * (juggler.saturating_sub(1)) as f64).to_radians();
    Coordinate {
        x: radius * angle.cos(),
        y: radius * angle.sin(),
        z: 100.0,
    }
}

fn default_juggler_angle(juggler: usize, number_of_jugglers: usize) -> f64 {
    if number_of_jugglers <= 1 {
        0.0
    } else {
        90.0 + 360.0 / number_of_jugglers as f64 * (juggler.saturating_sub(1)) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mhn_jml::{MhnJmlProp, MhnJmlSymmetry};
    use crate::mhn_matrix::MhnMatrix;
    use crate::mhn_symmetry::MhnSymmetryType;
    use crate::permutation::Permutation;
    use crate::siteswap::parse_config;

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-6,
            "actual={actual}, expected={expected}"
        );
    }

    #[test]
    fn toss_path_matches_parabolic_endpoints() {
        let start = Coordinate {
            x: 0.0,
            y: 0.0,
            z: 100.0,
        };
        let end = Coordinate {
            x: 20.0,
            y: 0.0,
            z: 100.0,
        };
        let path = TossPath::new(start, 0.0, end, 1.0, None).unwrap();

        let start_point = path.coordinate_at(0.0).unwrap();
        assert_close(start_point.x, start.x);
        assert_close(start_point.y, start.y);
        assert_close(start_point.z, start.z);
        let end_point = path.coordinate_at(1.0).unwrap();
        assert_close(end_point.x, end.x);
        assert_close(end_point.y, end.y);
        assert_close(end_point.z, end.z);
        assert!(path.coordinate_at(0.5).unwrap().z > start.z);
    }

    #[test]
    fn bounce_path_matches_endpoints_and_bounce_plane() {
        let start = Coordinate {
            x: 0.0,
            y: 0.0,
            z: 100.0,
        };
        let end = Coordinate {
            x: 20.0,
            y: 0.0,
            z: 100.0,
        };
        let path = BouncePath::new(start, 0.0, end, 1.0, None).unwrap();

        let start_point = path.coordinate_at(0.0).unwrap();
        assert_close(start_point.x, start.x);
        assert_close(start_point.y, start.y);
        assert_close(start_point.z, start.z);
        let end_point = path.coordinate_at(1.0).unwrap();
        assert_close(end_point.x, end.x);
        assert_close(end_point.y, end.y);
        assert_close(end_point.z, end.z);
        assert_close(
            path.coordinate_at(path.start_time + path.endtime[0])
                .unwrap()
                .z,
            path.bounceplane,
        );
        assert_eq!(
            path.bounce_volume(
                path.start_time + path.endtime[0] - 0.01,
                path.start_time + path.endtime[0] + 0.01,
            ),
            1.0
        );
    }

    #[test]
    fn softcatch_creates_softcatch_velocity_ref() {
        let mut pattern = MhnJmlPattern::new(1, 1, 1.0);
        pattern.symmetries.push(MhnJmlSymmetry {
            symmetry_type: MhnSymmetryType::Delay,
            number_of_jugglers: 1,
            number_of_paths: 1,
            juggler_perm: Permutation::identity(1),
            path_perm: Permutation::identity(1),
            delay: 1.0,
        });
        pattern.props.push(MhnJmlProp::new("ball", None));
        pattern.prop_assignment = vec![1];
        pattern.events.push(
            MhnJmlEvent::new(-20.0, 0.0, 0.0, 0.0, 1, 1).with_transition(MhnJmlTransition {
                transition_type: MhnJmlTransitionType::Throw,
                path: 1,
                throw_type: Some("toss".to_string()),
                throw_mod: None,
            }),
        );
        pattern.events.push(
            MhnJmlEvent::new(-20.0, 0.0, 0.0, 0.5, 1, 1).with_transition(MhnJmlTransition {
                transition_type: MhnJmlTransitionType::SoftCatch,
                path: 1,
                throw_type: None,
                throw_mod: None,
            }),
        );

        let layout = LaidoutPattern::from_jml_pattern(&pattern).unwrap();

        assert!(layout.hand_links[0][1].iter().any(|link| {
            link.end_velocity_ref
                .is_some_and(|reference| reference.source == VelocityRefSource::SoftCatch)
        }));
    }

    #[test]
    fn builds_path_links_from_generated_jml() {
        let spec = parse_config("pattern=3;bps=3").unwrap();
        let mut matrix = MhnMatrix::from_siteswap(&spec).unwrap();
        let model = matrix.to_jml_pattern(&spec).unwrap();
        let layout = LaidoutPattern::from_jml_pattern(&model).unwrap();

        assert_eq!(layout.number_of_paths, 3);
        assert_eq!(layout.path_links.len(), 3);
        assert!(layout.path_links.iter().all(|links| !links.is_empty()));
        assert!(
            layout
                .path_links
                .iter()
                .flatten()
                .any(|link| matches!(link.kind, PathLinkKind::Toss(_)))
        );
    }

    #[test]
    fn returns_path_coordinate_inside_loop() {
        let spec = parse_config("pattern=3;bps=3").unwrap();
        let mut matrix = MhnMatrix::from_siteswap(&spec).unwrap();
        let model = matrix.to_jml_pattern(&spec).unwrap();
        let layout = LaidoutPattern::from_jml_pattern(&model).unwrap();

        let point = layout.path_coordinate(1, 0.1).unwrap();
        assert!(point.x.is_finite());
        assert!(point.y.is_finite());
        assert!(point.z.is_finite());
    }

    #[test]
    fn juggler_frame_uses_layout_hand_coordinates() {
        let spec = parse_config("pattern=3;bps=3").unwrap();
        let mut matrix = MhnMatrix::from_siteswap(&spec).unwrap();
        let model = matrix.to_jml_pattern(&spec).unwrap();
        let layout = LaidoutPattern::from_jml_pattern(&model).unwrap();

        let time = 0.1;
        let frame = layout.juggler_frame(1, time).unwrap();
        let left_hand = layout.hand_coordinate(1, 1, time).unwrap();
        let right_hand = layout.hand_coordinate(1, 0, time).unwrap();

        assert_close(frame.left_hand.x, left_hand.x);
        assert_close(frame.left_hand.y, left_hand.y);
        assert_close(frame.left_hand.z, left_hand.z);
        assert_close(frame.right_hand.x, right_hand.x);
        assert_close(frame.right_hand.y, right_hand.y);
        assert_close(frame.right_hand.z, right_hand.z);
        assert!(layout.juggler_position(1, time).unwrap().x.is_finite());
        assert!(layout.juggler_angle(1, time).unwrap().is_finite());
        assert!(frame.left_shoulder.z > frame.left_waist.z);
        assert!(frame.right_shoulder.z > frame.right_waist.z);
    }

    #[test]
    fn converts_global_coordinates_back_to_jml_local_frame() {
        let spec = parse_config("pattern=3;bps=3").unwrap();
        let mut matrix = MhnMatrix::from_siteswap(&spec).unwrap();
        let model = matrix.to_jml_pattern(&spec).unwrap();
        let layout = LaidoutPattern::from_jml_pattern(&model).unwrap();
        let event = &model.events[0];
        let global = layout.events[0].global_coordinate;

        let local = layout
            .convert_global_to_local(global, event.juggler, event.t)
            .unwrap();

        assert_close(local.x, event.x);
        assert_close(local.y, event.y);
        assert_close(local.z, event.z);
    }

    #[test]
    fn overall_bounds_include_paths_hands_and_juggler_window() {
        let spec = parse_config("pattern=3;bps=3").unwrap();
        let mut matrix = MhnMatrix::from_siteswap(&spec).unwrap();
        let model = matrix.to_jml_pattern(&spec).unwrap();
        let layout = LaidoutPattern::from_jml_pattern(&model).unwrap();

        let bounds = layout.overall_bounds().unwrap();

        assert!(bounds.min.x < bounds.max.x);
        assert!(bounds.min.y < bounds.max.y);
        assert!(bounds.min.z < bounds.max.z);
        assert!(bounds.zoom_center.x.is_finite());
        assert!(bounds.zoom_center.y.is_finite());
        assert!(bounds.zoom_center.z.is_finite());
        assert!(bounds.max.z >= SHOULDER_H + NECK_H + HEAD_H);
    }
}

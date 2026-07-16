use crate::layout::LaidoutPattern;
use crate::mhn_hands::Coordinate;
use crate::mhn_jml::{
    MhnJmlEvent, MhnJmlPattern, MhnJmlProp, MhnJmlSymmetry, MhnJmlTransition, MhnJmlTransitionType,
};
use crate::mhn_symmetry::{MhnSymmetry, MhnSymmetryType};
use crate::mhn_throw::{LEFT_HAND, MhnThrow, MhnThrowLink, MhnThrowRef, RIGHT_HAND};
use crate::permutation::{Permutation, lcm};
use crate::siteswap::{Hand, SiteswapSpec, ThrowSpec, target_hand};

pub const DWELL_DEFAULT: f64 = 1.3;
pub const SQUEEZEBEATS_DEFAULT: f64 = 0.4;
pub const SECS_EVENT_GAP_MAX: f64 = 0.5;
pub const PROPDIAM_DEFAULT: f64 = 10.0;
pub const BEATS_AIRTIME_MIN: f64 = 0.3;
pub const BEATS_THROW_CATCH_MIN: f64 = 0.05;
pub const BEATS_CATCH_THROW_MIN: f64 = 0.02;
pub const RESTINGX: f64 = 25.0;
pub const GRAVITY_DEFAULT: f64 = 980.0;
pub const BOUNCEFRAC_DEFAULT: f64 = 0.9;

const SAME_THROW_X: [f64; 9] = [0.0, 20.0, 25.0, 12.0, 7.0, 7.5, 5.0, 5.0, 5.0];
const CROSSING_THROW_X: [f64; 9] = [0.0, 17.0, 17.0, 7.0, 10.0, 14.0, 25.0, 24.0, 30.0];
const CATCH_X: [f64; 9] = [0.0, 17.0, 25.0, 30.0, 40.0, 45.0, 45.0, 50.0, 50.0];

#[derive(Clone, Debug, PartialEq)]
pub struct TimingConfig {
    pub bps: f64,
    pub dwell: f64,
    pub squeezebeats: f64,
    pub dwell_array: Option<Vec<f64>>,
}

impl Default for TimingConfig {
    fn default() -> Self {
        Self {
            bps: 3.0,
            dwell: DWELL_DEFAULT,
            squeezebeats: SQUEEZEBEATS_DEFAULT,
            dwell_array: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MhnMatrix {
    pub number_of_jugglers: usize,
    pub number_of_paths: usize,
    pub period: usize,
    pub max_occupancy: usize,
    pub max_throw: usize,
    pub indexes: usize,
    pub odd_period_switchdelay: bool,
    pub throws: Vec<Vec<Vec<Vec<Option<MhnThrow>>>>>,
    pub external_throws: Vec<MhnThrow>,
    pub symmetries: Vec<MhnSymmetry>,
}

impl MhnMatrix {
    pub fn from_siteswap(spec: &SiteswapSpec) -> Result<Self, String> {
        if spec.beats.is_empty() {
            return Err("The siteswap pattern contains no beats".to_string());
        }

        let number_of_jugglers = spec.jugglers.max(1);
        let hands_period = spec
            .hands
            .as_ref()
            .map(|hands| {
                (1..=number_of_jugglers)
                    .fold(1usize, |acc, juggler| lcm(acc, hands.get_period(juggler)))
            })
            .unwrap_or(1);
        let body_period = spec
            .body
            .as_ref()
            .map(|body| {
                (1..=number_of_jugglers)
                    .fold(1usize, |acc, juggler| lcm(acc, body.get_period(juggler)))
            })
            .unwrap_or(1);
        let pattern_period = spec.beats.len();
        let norep_period = lcm(lcm(pattern_period, hands_period), body_period).max(1);
        let odd_period_switchdelay = spec.vanilla_async && !spec.sync && norep_period % 2 == 1;
        // Juggling Lab only retains a root-level explicit switch-delay when the
        // hand/body periods do not force the parsed pattern to be wrapped and
        // repeated. The switched throws are already present in `spec.beats`;
        // this flag preserves the corresponding JML symmetry metadata.
        let explicit_switchdelay = spec.switch_repeat && norep_period == pattern_period;
        let period = if odd_period_switchdelay {
            norep_period * 2
        } else {
            norep_period
        };
        let max_throw = spec.max_throw as usize;
        let indexes = max_throw + period + 1;
        let max_occupancy = max_occupancy(spec).max(1);
        let number_of_paths = spec.balls;
        let mut throws =
            vec![vec![vec![vec![None; max_occupancy]; indexes]; 2]; number_of_jugglers];

        for (first_index, beat_index) in indexed_beats_for_period(spec, period) {
            let beat = &spec.beats[beat_index];
            let switch_hands = odd_period_switchdelay && first_index >= period / 2;
            let mut index = first_index;

            while index < indexes {
                let mut slots_by_hand = vec![[0usize; 2]; number_of_jugglers];
                for throw in &beat.throws {
                    let source_juggler = throw.source_juggler.clamp(1, number_of_jugglers);
                    let source_hand = source_hand_for_throw(spec, throw, first_index, switch_hands);
                    let hand_index = hand_to_index(source_hand);
                    let slot = slots_by_hand[source_juggler - 1][hand_index];
                    slots_by_hand[source_juggler - 1][hand_index] += 1;
                    if slot >= max_occupancy {
                        continue;
                    }

                    let target = target_hand(spec, source_hand, throw.value, throw.cross);
                    let target_hand_index = hand_to_index(target);
                    let target_juggler = wrap_juggler(throw.target_juggler, number_of_jugglers);
                    let mut throw_mod = default_throw_mod(throw, source_hand, target);
                    let mut mhn_throw = MhnThrow::new(
                        source_juggler,
                        hand_index,
                        index as isize,
                        slot,
                        target_juggler,
                        target_hand_index,
                        index as isize + throw.value as isize,
                        -1,
                        throw_mod.take(),
                    );
                    if let Some(hands) = &spec.hands {
                        let mut hands_beat = index;
                        if throw.sync && hand_index == RIGHT_HAND {
                            hands_beat += 1;
                        }
                        mhn_throw.hands_beat =
                            (hands_beat % hands.get_period(source_juggler)) as isize;
                    }
                    throws[source_juggler - 1][hand_index][index][slot] = Some(mhn_throw);
                }

                index += period;
            }
        }

        resolve_modifiers(&mut throws, indexes, max_occupancy);

        let mut symmetries = vec![MhnSymmetry::new(
            MhnSymmetryType::Delay,
            number_of_jugglers,
            None,
            period as isize,
        )?];
        if odd_period_switchdelay || explicit_switchdelay {
            let jug_perm = (1..=number_of_jugglers)
                .map(|juggler| format!("({juggler},{juggler}*)"))
                .collect::<String>();
            symmetries.push(MhnSymmetry::new(
                MhnSymmetryType::SwitchDelay,
                number_of_jugglers,
                Some(jug_perm),
                (if odd_period_switchdelay {
                    period / 2
                } else {
                    pattern_period / 2
                }) as isize,
            )?);
        }

        let mut matrix = Self {
            number_of_jugglers,
            number_of_paths,
            period,
            max_occupancy,
            max_throw,
            indexes,
            odd_period_switchdelay,
            throws,
            external_throws: Vec::new(),
            symmetries,
        };
        matrix.build_juggling_matrix()?;
        Ok(matrix)
    }

    pub fn get(&self, juggler: usize, hand: usize, index: usize, slot: usize) -> Option<&MhnThrow> {
        self.throws
            .get(juggler.saturating_sub(1))?
            .get(hand)?
            .get(index)?
            .get(slot)?
            .as_ref()
    }

    pub fn build_juggling_matrix(&mut self) -> Result<(), String> {
        self.find_primary_throws()?;
        self.assign_paths()?;
        self.add_throw_sources()?;
        self.set_catch_order()?;
        self.find_dwell_windows();
        Ok(())
    }

    pub fn find_catch_throw_times(&mut self, timing: &TimingConfig) -> Result<(), String> {
        let bps = timing.bps.max(0.001);
        let beats_one_throw_early = (timing.dwell + BEATS_AIRTIME_MIN - 1.0).max(0.0);
        let hss_active = timing.dwell_array.is_some();

        for index in 0..self.indexes {
            for juggler_index in 0..self.number_of_jugglers {
                for hand in 0..2 {
                    let first_ref = MhnThrowRef {
                        juggler: juggler_index + 1,
                        hand,
                        index: index as isize,
                        slot: 0,
                    };
                    let Some(first_throw) = self.get_by_ref(first_ref) else {
                        continue;
                    };
                    let dwell_window = first_throw.dwell_window;

                    let onethrown = self.hand_slot_refs(first_ref).iter().any(|throw_ref| {
                        self.get_by_ref(*throw_ref)
                            .is_some_and(MhnThrow::is_thrown_one)
                    });

                    let throw_time = if hss_active || !onethrown {
                        index as f64 / bps
                    } else {
                        (index as f64 - beats_one_throw_early) / bps
                    };
                    for throw_ref in self.hand_slot_refs(first_ref) {
                        if let Some(throw) = self.get_mut_by_ref(throw_ref) {
                            throw.throw_time = throw_time;
                        }
                    }

                    let mut num_catches = 0usize;
                    let mut onecaught = false;
                    for throw_ref in self.hand_slot_refs(first_ref) {
                        let Some(throw) = self.get_by_ref(throw_ref) else {
                            continue;
                        };
                        if throw.catching {
                            num_catches += 1;
                            let source_one = throw
                                .source
                                .and_then(|link| self.get_link(link))
                                .is_some_and(MhnThrow::is_thrown_one);
                            if source_one {
                                onecaught = true;
                            }
                        }
                    }

                    let mut temp_index = index as isize - dwell_window as isize;
                    while temp_index < 0 {
                        temp_index += self.period as isize;
                    }
                    let prev_onethrown = (0..self.max_occupancy).any(|slot| {
                        let throw_ref = MhnThrowRef {
                            juggler: juggler_index + 1,
                            hand,
                            index: temp_index,
                            slot,
                        };
                        self.get_by_ref(throw_ref)
                            .is_some_and(MhnThrow::is_thrown_one)
                    });

                    let first_throw_time = self
                        .get_by_ref(first_ref)
                        .map(|throw| throw.throw_time)
                        .unwrap_or(throw_time);
                    let mut first_catch_time = (index as f64 - timing.dwell) / bps;
                    first_catch_time = first_catch_time.max(
                        (index as f64
                            - dwell_window as f64
                            - if prev_onethrown {
                                beats_one_throw_early
                            } else {
                                0.0
                            }
                            + BEATS_THROW_CATCH_MIN)
                            / bps,
                    );
                    if onecaught {
                        first_catch_time = first_catch_time.max(
                            (index as f64 - 1.0 - beats_one_throw_early + BEATS_AIRTIME_MIN) / bps,
                        );
                    }
                    first_catch_time =
                        first_catch_time.min(first_throw_time - BEATS_CATCH_THROW_MIN / bps);

                    let refs = self.hand_slot_refs(first_ref);
                    for throw_ref in refs {
                        let Some(throw) = self.get_by_ref(throw_ref) else {
                            continue;
                        };
                        let mut catch_time = first_catch_time;
                        if num_catches > 1 {
                            catch_time += (throw.catch_num as f64 / (num_catches - 1) as f64)
                                * (timing.squeezebeats / bps);
                        }

                        if let Some(dwell_array) = &timing.dwell_array {
                            if !dwell_array.is_empty() {
                                let new_index = index % dwell_array.len();
                                catch_time = (index as f64 - dwell_array[new_index]) / bps;
                                if num_catches > 1 {
                                    catch_time += (throw.catch_num as f64
                                        / (num_catches - 1) as f64)
                                        * (timing.squeezebeats / bps);
                                }
                            }
                        }

                        catch_time = catch_time.min(throw.throw_time - BEATS_CATCH_THROW_MIN / bps);
                        if let Some(throw) = self.get_mut_by_ref(throw_ref) {
                            throw.catch_time = catch_time;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub fn to_jml_pattern(&mut self, spec: &SiteswapSpec) -> Result<MhnJmlPattern, String> {
        let timing = TimingConfig {
            bps: spec.bps,
            dwell: spec.dwell,
            squeezebeats: spec.squeezebeats,
            dwell_array: spec.dwell_array.clone(),
        };
        self.find_catch_throw_times(&timing)?;

        let mut model = MhnJmlPattern::new(
            self.number_of_jugglers,
            self.number_of_paths,
            self.period as f64 / timing.bps.max(0.001),
        );
        model.title = spec.title.clone();
        model.base_pattern_notation = Some("siteswap".to_string());
        model.base_pattern_config = Some(spec.raw_config.clone());

        if self.number_of_paths > 0 {
            self.add_props_to_jml(spec, &mut model);
        }
        self.add_symmetries_to_jml(&mut model, timing.bps)?;
        let mut hand_touched = vec![vec![false; 2]; self.number_of_jugglers];
        let mut path_touched = vec![false; self.number_of_paths];
        self.add_primary_events_to_jml(
            spec,
            &timing,
            &mut model,
            &mut hand_touched,
            &mut path_touched,
        )?;
        self.add_juggler_positions_to_jml(spec, &timing, &mut model);
        self.add_events_for_untouched_hands_to_jml(&mut model, &hand_touched);
        self.add_events_for_untouched_paths_to_jml(&mut model, &mut path_touched);
        let model_before_gap_events = (spec.hands.is_none()).then(|| model.clone());
        if spec.hands.is_none() {
            model.add_events_for_gaps(SECS_EVENT_GAP_MAX);
        }
        Self::finish_generated_jml(&mut model)?;

        if !spec.bps_explicit {
            let layout = LaidoutPattern::from_jml_pattern_unchecked(&model)?;
            let scale_factor = layout.time_scale_to_fit_throws(1.01);
            if scale_factor > 1.0 {
                if let Some(mut model_without_gaps) = model_before_gap_events {
                    Self::finish_generated_jml(&mut model_without_gaps)?;
                    let layout = LaidoutPattern::from_jml_pattern_unchecked(&model_without_gaps)?;
                    let final_scale = layout.time_scale_to_fit_throws(1.01);
                    model = model_without_gaps.with_scaled_time(final_scale);
                    model.add_events_for_gaps(SECS_EVENT_GAP_MAX);
                    Self::finish_generated_jml(&mut model)?;
                } else {
                    model = model.with_scaled_time(scale_factor);
                }
            }
        }

        if let Some(colors) = &spec.colors {
            model.apply_prop_colors(colors)?;
        }
        model.sort_events();
        model.rebuild_path_events();
        Ok(model)
    }

    fn finish_generated_jml(model: &mut MhnJmlPattern) -> Result<(), String> {
        model.add_locations_for_incomplete_events(RESTINGX)?;
        model.merge_coincident_events();
        model.fix_holds()?;
        model.select_primary_events()?;
        model.merge_coincident_events();
        Ok(())
    }

    fn add_props_to_jml(&self, spec: &SiteswapSpec, model: &mut MhnJmlPattern) {
        let modifier = ((spec.prop_diam - PROPDIAM_DEFAULT).abs() > f64::EPSILON)
            .then(|| format!("diam={:?}", spec.prop_diam));
        model
            .props
            .push(MhnJmlProp::new(spec.prop_name.clone(), modifier));
        model.prop_assignment = vec![1; self.number_of_paths];
    }

    fn add_symmetries_to_jml(&self, model: &mut MhnJmlPattern, bps: f64) -> Result<(), String> {
        for symmetry in &self.symmetries {
            let mut path_map = vec![0usize; self.number_of_paths + 1];
            match symmetry.symmetry_type {
                MhnSymmetryType::Delay => {
                    for index in 0..self.indexes.saturating_sub(symmetry.delay as usize) {
                        for juggler_index in 0..self.number_of_jugglers {
                            for hand in 0..2 {
                                for slot in 0..self.max_occupancy {
                                    let throw_ref = MhnThrowRef {
                                        juggler: juggler_index + 1,
                                        hand,
                                        index: index as isize,
                                        slot,
                                    };
                                    let Some(throw) = self.get_by_ref(throw_ref) else {
                                        continue;
                                    };
                                    if throw.path_num == -1 {
                                        continue;
                                    }
                                    let image_ref = MhnThrowRef {
                                        index: index as isize + symmetry.delay,
                                        ..throw_ref
                                    };
                                    let image = self.get_by_ref(image_ref).ok_or_else(|| {
                                        "Invalid pattern: path symmetry".to_string()
                                    })?;
                                    self.record_path_map(
                                        &mut path_map,
                                        throw.path_num,
                                        image.path_num,
                                    )?;
                                }
                            }
                        }
                    }
                }
                MhnSymmetryType::SwitchDelay => {
                    let jug_perm = &symmetry.juggler_perm;
                    for index in 0..self.indexes.saturating_sub(symmetry.delay as usize) {
                        for juggler_index in 0..self.number_of_jugglers {
                            for hand in 0..2 {
                                for slot in 0..self.max_occupancy {
                                    let throw_ref = MhnThrowRef {
                                        juggler: juggler_index + 1,
                                        hand,
                                        index: index as isize,
                                        slot,
                                    };
                                    let Some(throw) = self.get_by_ref(throw_ref) else {
                                        continue;
                                    };
                                    if throw.path_num == -1 {
                                        continue;
                                    }
                                    let mapped = jug_perm.map((juggler_index + 1) as i32);
                                    let image_ref = MhnThrowRef {
                                        juggler: mapped.unsigned_abs() as usize,
                                        hand: if mapped > 0 { hand } else { 1 - hand },
                                        index: index as isize + symmetry.delay,
                                        slot,
                                    };
                                    let image = self.get_by_ref(image_ref).ok_or_else(|| {
                                        "Invalid pattern: switchdelay path symmetry".to_string()
                                    })?;
                                    self.record_path_map(
                                        &mut path_map,
                                        throw.path_num,
                                        image.path_num,
                                    )?;
                                }
                            }
                        }
                    }
                }
                MhnSymmetryType::Switch => {
                    for path in 1..=self.number_of_paths {
                        path_map[path] = path;
                    }
                }
            }

            for path in 1..=self.number_of_paths {
                if path_map[path] == 0 {
                    path_map[path] = path;
                }
            }
            let mapping = path_map[1..]
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",");
            model.symmetries.push(MhnJmlSymmetry {
                symmetry_type: symmetry.symmetry_type,
                number_of_jugglers: symmetry.number_of_jugglers,
                number_of_paths: self.number_of_paths,
                juggler_perm: symmetry.juggler_perm.clone(),
                path_perm: Permutation::parse(self.number_of_paths, &mapping, false)?,
                delay: symmetry.delay as f64 / bps.max(0.001),
            });
        }
        Ok(())
    }

    fn record_path_map(
        &self,
        path_map: &mut [usize],
        from_path: isize,
        to_path: isize,
    ) -> Result<(), String> {
        if from_path <= 0 || to_path <= 0 {
            return Err("Invalid pattern: null path".to_string());
        }
        let from_path = from_path as usize;
        let to_path = to_path as usize;
        if path_map[from_path] == 0 {
            path_map[from_path] = to_path;
        } else if path_map[from_path] != to_path {
            return Err("Invalid pattern: inconsistent path symmetry".to_string());
        }
        Ok(())
    }

    fn add_primary_events_to_jml(
        &self,
        spec: &SiteswapSpec,
        timing: &TimingConfig,
        model: &mut MhnJmlPattern,
        hand_touched: &mut [Vec<bool>],
        path_touched: &mut [bool],
    ) -> Result<(), String> {
        for index in 0..self.period {
            for juggler_index in 0..self.number_of_jugglers {
                for hand in 0..2 {
                    let primary_ref = MhnThrowRef {
                        juggler: juggler_index + 1,
                        hand,
                        index: index as isize,
                        slot: 0,
                    };
                    let Some(primary_throw) = self.get_by_ref(primary_ref) else {
                        continue;
                    };
                    if primary_throw.primary != Some(primary_ref) {
                        continue;
                    }

                    let mut throw_transitions = Vec::new();
                    let mut throw_x_sum = 0.0;
                    let mut num_throws = 0usize;
                    for throw_ref in self.hand_slot_refs(primary_ref) {
                        let throw = self.get_by_ref(throw_ref).expect("hand_slot_refs is valid");
                        let (throw_type, throw_mod) = transition_style(
                            throw.throw_mod.as_deref().unwrap_or("T"),
                            GRAVITY_DEFAULT,
                            BOUNCEFRAC_DEFAULT,
                        );
                        if !throw.throw_mod.as_deref().unwrap_or("T").starts_with('H') {
                            if throw.is_zero() {
                                return Err("Invalid modifier on throw 0".to_string());
                            }
                            throw_transitions.push(MhnJmlTransition {
                                transition_type: MhnJmlTransitionType::Throw,
                                path: throw.path_num as usize,
                                throw_type,
                                throw_mod,
                            });
                            let throw_value = throw.target_index - index as isize;
                            let throw_index = throw_value.clamp(0, 8) as usize;
                            throw_x_sum += if throw.target_hand == hand {
                                SAME_THROW_X[throw_index]
                            } else {
                                CROSSING_THROW_X[throw_index]
                            };
                            num_throws += 1;
                        } else if spec.hands.is_some() && !throw.is_zero() {
                            throw_transitions.push(MhnJmlTransition {
                                transition_type: MhnJmlTransitionType::Holding,
                                path: throw.path_num as usize,
                                throw_type,
                                throw_mod,
                            });
                            mark_path(path_touched, throw.path_num);
                        }
                    }

                    if spec.hands.is_some() || num_throws != 0 {
                        let (coord, calcpos) = if let Some(hands) = &spec.hands {
                            let coord = hands
                                .get_coordinate(
                                    primary_throw.juggler,
                                    primary_throw.hands_beat as usize,
                                    0,
                                )
                                .ok_or_else(|| "Missing hands coordinate".to_string())?;
                            (mirror_hand_coordinate(coord, hand), false)
                        } else if num_throws > 0 {
                            let mut x = throw_x_sum / num_throws as f64;
                            if hand == LEFT_HAND {
                                x = -x;
                            }
                            (Coordinate { x, y: 0.0, z: 0.0 }, false)
                        } else {
                            (
                                Coordinate {
                                    x: 0.0,
                                    y: 0.0,
                                    z: 0.0,
                                },
                                true,
                            )
                        };

                        model.events.push(MhnJmlEvent {
                            x: coord.x,
                            y: coord.y,
                            z: coord.z,
                            t: primary_throw.throw_time,
                            juggler: juggler_index + 1,
                            hand,
                            calcpos,
                            transitions: throw_transitions,
                        });

                        for throw_ref in self.all_throw_refs() {
                            if self
                                .get_by_ref(throw_ref)
                                .is_some_and(|throw| throw.primary == Some(primary_ref))
                            {
                                hand_touched[throw_ref.juggler - 1][throw_ref.hand] = true;
                            }
                        }
                    }

                    self.add_catch_events_to_jml(
                        spec,
                        timing,
                        primary_ref,
                        primary_throw,
                        model,
                        path_touched,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn add_catch_events_to_jml(
        &self,
        spec: &SiteswapSpec,
        timing: &TimingConfig,
        primary_ref: MhnThrowRef,
        primary_throw: &MhnThrow,
        model: &mut MhnJmlPattern,
        path_touched: &mut [bool],
    ) -> Result<(), String> {
        let mut catch_x_sum = 0.0;
        let mut num_catches = 0usize;
        for throw_ref in self.hand_slot_refs(primary_ref) {
            let throw = self.get_by_ref(throw_ref).expect("hand_slot_refs is valid");
            if !throw.catching {
                continue;
            }
            let source = throw
                .source
                .and_then(|link| self.get_link(link))
                .ok_or_else(|| "Catch missing source".to_string())?;
            mark_path(path_touched, throw.path_num);
            let catch_value = primary_ref.index - source.index;
            catch_x_sum += CATCH_X[catch_value.clamp(0, 8) as usize];
            num_catches += 1;
        }

        if spec.hands.is_none() && num_catches == 0 {
            return Ok(());
        }

        let mut last_catch_time = 0.0;
        if timing.squeezebeats == 0.0 || num_catches < 2 {
            let coord =
                self.catch_coordinate(spec, primary_ref, primary_throw, catch_x_sum, num_catches)?;
            last_catch_time = primary_throw.catch_time;
            let mut event = MhnJmlEvent::new(
                coord.x,
                coord.y,
                coord.z,
                primary_throw.catch_time,
                primary_ref.juggler,
                primary_ref.hand,
            );

            for throw_ref in self.hand_slot_refs(primary_ref) {
                let throw = self.get_by_ref(throw_ref).expect("hand_slot_refs is valid");
                if throw.catching {
                    event = event.with_transition(MhnJmlTransition {
                        transition_type: MhnJmlTransitionType::Catch,
                        path: throw.path_num as usize,
                        throw_type: None,
                        throw_mod: None,
                    });
                } else if spec.hands.is_some() && throw.path_num != -1 {
                    event = event.with_transition(MhnJmlTransition {
                        transition_type: MhnJmlTransitionType::Holding,
                        path: throw.path_num as usize,
                        throw_type: None,
                        throw_mod: None,
                    });
                    mark_path(path_touched, throw.path_num);
                }
            }
            model.events.push(event);
        } else {
            for throw_ref in self.hand_slot_refs(primary_ref) {
                let throw = self.get_by_ref(throw_ref).expect("hand_slot_refs is valid");
                if !throw.catching {
                    continue;
                }
                let coord = self.catch_coordinate(
                    spec,
                    primary_ref,
                    primary_throw,
                    catch_x_sum,
                    num_catches,
                )?;
                if throw.catch_num == (num_catches - 1) as isize {
                    last_catch_time = throw.catch_time;
                }
                model.events.push(
                    MhnJmlEvent::new(
                        coord.x,
                        coord.y,
                        coord.z,
                        throw.catch_time,
                        primary_ref.juggler,
                        primary_ref.hand,
                    )
                    .with_transition(MhnJmlTransition {
                        transition_type: MhnJmlTransitionType::Catch,
                        path: throw.path_num as usize,
                        throw_type: None,
                        throw_mod: None,
                    }),
                );
            }
        }

        if spec.hands.is_some() {
            self.add_intermediate_hand_events(
                spec,
                primary_ref,
                primary_throw,
                last_catch_time,
                model,
            )?;
        }

        Ok(())
    }

    fn catch_coordinate(
        &self,
        spec: &SiteswapSpec,
        primary_ref: MhnThrowRef,
        primary_throw: &MhnThrow,
        catch_x_sum: f64,
        num_catches: usize,
    ) -> Result<Coordinate, String> {
        if let Some(hands) = &spec.hands {
            let mut pos = primary_throw.hands_beat - 2;
            while pos < 0 {
                pos += hands.get_period(primary_throw.juggler) as isize;
            }
            let catch_index = hands.get_catch_index(primary_throw.juggler, pos as usize);
            let coord = hands
                .get_coordinate(primary_throw.juggler, pos as usize, catch_index)
                .ok_or_else(|| "Missing catch hands coordinate".to_string())?;
            Ok(mirror_hand_coordinate(coord, primary_ref.hand))
        } else if num_catches > 0 {
            let mut x = catch_x_sum / num_catches as f64;
            if primary_ref.hand == LEFT_HAND {
                x = -x;
            }
            Ok(Coordinate { x, y: 0.0, z: 0.0 })
        } else {
            Ok(Coordinate {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            })
        }
    }

    fn add_intermediate_hand_events(
        &self,
        spec: &SiteswapSpec,
        primary_ref: MhnThrowRef,
        primary_throw: &MhnThrow,
        last_catch_time: f64,
        model: &mut MhnJmlPattern,
    ) -> Result<(), String> {
        let Some(hands) = &spec.hands else {
            return Ok(());
        };

        let mut pos = primary_throw.hands_beat - 2;
        while pos < 0 {
            pos += hands.get_period(primary_throw.juggler) as isize;
        }
        let catch_index = hands.get_catch_index(primary_throw.juggler, pos as usize);
        let num_coords =
            hands.get_number_of_coordinates(primary_throw.juggler, pos as usize) - catch_index;
        for di in 1..num_coords {
            let Some(coord) =
                hands.get_coordinate(primary_throw.juggler, pos as usize, catch_index + di)
            else {
                continue;
            };
            let coord = mirror_hand_coordinate(coord, primary_ref.hand);
            model.events.push(MhnJmlEvent::new(
                coord.x,
                coord.y,
                coord.z,
                last_catch_time
                    + di as f64 * (primary_throw.throw_time - last_catch_time) / num_coords as f64,
                primary_ref.juggler,
                primary_ref.hand,
            ));
        }

        let mut next_catch_time = last_catch_time;
        let mut k2 = primary_ref.index as usize + 1;
        while next_catch_time == last_catch_time {
            let mut temp_index = k2;
            let mut wrap = 0usize;
            while temp_index >= self.indexes {
                temp_index -= self.indexes;
                wrap += 1;
            }
            if wrap > 1 {
                return Err("Couldn't find next catch/hold".to_string());
            }
            for temp_slot in 0..self.max_occupancy {
                let throw_ref = MhnThrowRef {
                    index: temp_index as isize,
                    slot: temp_slot,
                    ..primary_ref
                };
                let Some(throw) = self.get_by_ref(throw_ref) else {
                    break;
                };
                let catch_time = throw.catch_time + (wrap * self.indexes) as f64 / spec.bps;
                next_catch_time = if temp_slot == 0 {
                    catch_time
                } else {
                    next_catch_time.min(catch_time)
                };
            }
            k2 += 1;
        }

        let pos = primary_throw.hands_beat as usize;
        let num_coords = hands.get_catch_index(primary_throw.juggler, pos);
        for di in 1..num_coords {
            let Some(coord) = hands.get_coordinate(primary_throw.juggler, pos, di) else {
                continue;
            };
            let coord = mirror_hand_coordinate(coord, primary_ref.hand);
            model.events.push(MhnJmlEvent::new(
                coord.x,
                coord.y,
                coord.z,
                primary_throw.throw_time
                    + di as f64 * (next_catch_time - primary_throw.throw_time) / num_coords as f64,
                primary_ref.juggler,
                primary_ref.hand,
            ));
        }

        Ok(())
    }

    fn add_juggler_positions_to_jml(
        &self,
        spec: &SiteswapSpec,
        timing: &TimingConfig,
        model: &mut MhnJmlPattern,
    ) {
        let Some(body) = &spec.body else {
            return;
        };
        for index in 0..self.period {
            for juggler in 1..=self.number_of_jugglers {
                let body_index = index % body.get_period(juggler);
                let coords = body.get_number_of_positions(juggler, body_index);
                for coord_index in 0..coords {
                    if let Some(mut position) = body.get_position(juggler, body_index, coord_index)
                    {
                        position.t =
                            (index as f64 + coord_index as f64 / coords as f64) / timing.bps;
                        model.positions.push(position);
                    }
                }
            }
        }
    }

    fn add_events_for_untouched_hands_to_jml(
        &self,
        model: &mut MhnJmlPattern,
        hand_touched: &[Vec<bool>],
    ) {
        for juggler_index in 0..self.number_of_jugglers {
            for hand in 0..2 {
                if !hand_touched[juggler_index][hand] {
                    model.events.push(MhnJmlEvent::new(
                        if hand == RIGHT_HAND {
                            RESTINGX
                        } else {
                            -RESTINGX
                        },
                        0.0,
                        0.0,
                        -1.0,
                        juggler_index + 1,
                        hand,
                    ));
                }
            }
        }
    }

    fn add_events_for_untouched_paths_to_jml(
        &self,
        model: &mut MhnJmlPattern,
        path_touched: &mut [bool],
    ) {
        for symmetry in &model.symmetries {
            let perm = &symmetry.path_perm;
            for path in 1..=self.number_of_paths {
                if path_touched[path - 1] {
                    for power in 1..perm.order_of(path as i32) {
                        let mapped = perm.map_power(path as i32, power as i32);
                        if mapped > 0 {
                            path_touched[mapped as usize - 1] = true;
                        }
                    }
                }
            }
        }

        for path in 1..=self.number_of_paths {
            if path_touched[path - 1] {
                continue;
            }

            let mut hand = LEFT_HAND;
            let mut juggler = 1usize;
            'outer: for throw_ref in self.all_throw_refs() {
                if self
                    .get_by_ref(throw_ref)
                    .is_some_and(|throw| throw.path_num == path as isize)
                {
                    hand = throw_ref.hand;
                    juggler = throw_ref.juggler;
                    break 'outer;
                }
            }

            for event in &mut model.events {
                if event.hand == hand && event.juggler == juggler {
                    event.transitions.push(MhnJmlTransition {
                        transition_type: MhnJmlTransitionType::Holding,
                        path,
                        throw_type: None,
                        throw_mod: None,
                    });
                    path_touched[path - 1] = true;
                }
            }
        }
    }

    fn find_primary_throws(&mut self) -> Result<(), String> {
        for throw_ref in self.all_throw_refs() {
            if let Some(throw) = self.get_mut_by_ref(throw_ref) {
                throw.primary = Some(throw_ref);
                throw.source = None;
            }
        }

        let mut changed = true;
        while changed {
            changed = false;

            let symmetries = self.symmetries.clone();
            for sym in &symmetries {
                let refs = self.all_throw_refs();
                for throw_ref in refs {
                    let image_index = throw_ref.index + sym.delay;
                    if image_index >= self.indexes as isize {
                        continue;
                    }

                    let image_juggler = sym.juggler_perm.map(throw_ref.juggler as i32);
                    let image_hand = if image_juggler > 0 {
                        throw_ref.hand
                    } else {
                        1 - throw_ref.hand
                    };
                    let image_ref = MhnThrowRef {
                        juggler: image_juggler.unsigned_abs() as usize,
                        hand: image_hand,
                        index: image_index,
                        slot: throw_ref.slot,
                    };

                    let current_primary = self
                        .get_by_ref(throw_ref)
                        .and_then(|throw| throw.primary)
                        .ok_or_else(|| "Problem finding primary throws".to_string())?;
                    let image_primary = self
                        .get_by_ref(image_ref)
                        .and_then(|throw| throw.primary)
                        .ok_or_else(|| "Problem finding primary throws".to_string())?;

                    if current_primary == image_primary {
                        continue;
                    }

                    let new_primary = self.min_primary(current_primary, image_primary)?;
                    self.set_primary(throw_ref, new_primary);
                    self.set_primary(image_ref, new_primary);
                    changed = true;
                }
            }
        }

        Ok(())
    }

    fn assign_paths(&mut self) -> Result<(), String> {
        let refs = self.all_throw_refs();
        for throw_ref in &refs {
            let Some(primary_throw) = self.get_by_ref(*throw_ref) else {
                continue;
            };
            if primary_throw.primary != Some(*throw_ref) || primary_throw.is_zero() {
                continue;
            }

            let mut target_slot = 0usize;
            while target_slot < self.max_occupancy {
                let mut works = true;

                for image_ref in &refs {
                    let Some(image_throw) = self.get_by_ref(*image_ref) else {
                        continue;
                    };
                    if image_throw.primary != Some(*throw_ref)
                        || image_throw.target_index >= self.indexes as isize
                    {
                        continue;
                    }

                    let target_ref = MhnThrowRef {
                        juggler: image_throw.target_juggler,
                        hand: image_throw.target_hand,
                        index: image_throw.target_index,
                        slot: target_slot,
                    };
                    works = self
                        .get_by_ref(target_ref)
                        .is_some_and(|target| target.source.is_none());
                    if !works {
                        break;
                    }
                }

                if works {
                    break;
                }
                target_slot += 1;
            }

            if target_slot == self.max_occupancy {
                return Err("Invalid pattern: too many objects land on the same beat".to_string());
            }

            let mut links = Vec::new();
            for image_ref in &refs {
                let Some(image_throw) = self.get_by_ref(*image_ref) else {
                    continue;
                };
                if image_throw.primary != Some(*throw_ref)
                    || image_throw.target_index >= self.indexes as isize
                {
                    continue;
                }

                links.push((
                    *image_ref,
                    MhnThrowRef {
                        juggler: image_throw.target_juggler,
                        hand: image_throw.target_hand,
                        index: image_throw.target_index,
                        slot: target_slot,
                    },
                ));
            }

            for (source_ref, target_ref) in links {
                self.set_target(source_ref, MhnThrowLink::Matrix(target_ref));
                self.set_source(target_ref, MhnThrowLink::Matrix(source_ref));
            }
        }

        let mut current_path = 1isize;
        for throw_ref in refs {
            let Some(throw) = self.get_by_ref(throw_ref) else {
                continue;
            };
            let source = throw.source;
            let path_num = if let Some(source) = source {
                self.get_link(source)
                    .ok_or_else(|| "Problem assigning path numbers".to_string())?
                    .path_num
            } else if throw.is_zero() {
                -1
            } else {
                if current_path > self.number_of_paths as isize {
                    return Err(format!(
                        "Invalid pattern: path assignment problem at {:?} value={} target=({}, {}, {}) current_path={} paths={}",
                        throw_ref,
                        throw.throw_value(),
                        throw.target_juggler,
                        throw.target_hand,
                        throw.target_index,
                        current_path,
                        self.number_of_paths
                    ));
                }
                let path_num = current_path;
                current_path += 1;
                path_num
            };
            if let Some(throw) = self.get_mut_by_ref(throw_ref) {
                throw.path_num = path_num;
            }
        }

        if current_path <= self.number_of_paths as isize {
            return Err("Problem assigning path numbers 2".to_string());
        }

        Ok(())
    }

    fn add_throw_sources(&mut self) -> Result<(), String> {
        for index in (0..self.indexes).rev() {
            for juggler_index in 0..self.number_of_jugglers {
                for hand in 0..2 {
                    for slot in 0..self.max_occupancy {
                        let throw_ref = MhnThrowRef {
                            juggler: juggler_index + 1,
                            hand,
                            index: index as isize,
                            slot,
                        };
                        let Some(current_throw) = self.get_by_ref(throw_ref) else {
                            continue;
                        };
                        if current_throw.source.is_some() {
                            continue;
                        }
                        if current_throw.is_zero() {
                            continue;
                        }
                        if index + self.period >= self.indexes {
                            return Err("Could not get throw source 2".to_string());
                        }

                        let future_ref = MhnThrowRef {
                            index: (index + self.period) as isize,
                            ..throw_ref
                        };
                        let future_source = self
                            .get_by_ref(future_ref)
                            .and_then(|throw| throw.source)
                            .ok_or_else(|| "Could not get throw source 1".to_string())?;
                        let source_throw = self
                            .get_link(future_source)
                            .ok_or_else(|| "Could not get throw source 1".to_string())?
                            .clone();

                        let mut external = MhnThrow::new(
                            source_throw.juggler,
                            source_throw.hand,
                            source_throw.index - self.period as isize,
                            source_throw.slot,
                            juggler_index + 1,
                            hand,
                            index as isize,
                            slot as isize,
                            source_throw.throw_mod.clone(),
                        );
                        external.hands_beat = -1;
                        external.path_num = current_throw.path_num;
                        external.primary = source_throw.primary;
                        external.source = None;
                        external.target = Some(MhnThrowLink::Matrix(throw_ref));

                        let external_id = self.external_throws.len();
                        self.external_throws.push(external);
                        self.set_source(throw_ref, MhnThrowLink::External(external_id));
                    }
                }
            }
        }

        Ok(())
    }

    fn set_catch_order(&mut self) -> Result<(), String> {
        for index in 0..self.indexes {
            for juggler_index in 0..self.number_of_jugglers {
                for hand in 0..2 {
                    let mut slot_catches = 0isize;

                    for slot in 0..self.max_occupancy {
                        let throw_ref = MhnThrowRef {
                            juggler: juggler_index + 1,
                            hand,
                            index: index as isize,
                            slot,
                        };
                        let Some(throw) = self.get_by_ref(throw_ref) else {
                            break;
                        };
                        let Some(source) = throw.source.and_then(|link| self.get_link(link)) else {
                            if throw.is_zero() {
                                if let Some(throw) = self.get_mut_by_ref(throw_ref) {
                                    throw.catching = false;
                                }
                                continue;
                            }
                            return Err("Catch order missing source".to_string());
                        };
                        let catching = source
                            .throw_mod
                            .as_deref()
                            .is_some_and(|value| !value.starts_with('H'));

                        if let Some(throw) = self.get_mut_by_ref(throw_ref) {
                            throw.catching = catching;
                            if catching {
                                throw.catch_num = slot_catches;
                            }
                        }
                        if catching {
                            slot_catches += 1;
                        }
                    }

                    if slot_catches < 2 {
                        continue;
                    }

                    for slot1 in 0..self.max_occupancy {
                        let ref1 = MhnThrowRef {
                            juggler: juggler_index + 1,
                            hand,
                            index: index as isize,
                            slot: slot1,
                        };
                        let Some(throw1) = self.get_by_ref(ref1) else {
                            break;
                        };
                        let primary1 = throw1.primary;
                        let catching1 = throw1.catching;
                        let catch1 = throw1.catch_num;
                        if primary1 != Some(ref1) {
                            break;
                        }
                        if !catching1 {
                            continue;
                        }

                        for slot2 in slot1 + 1..self.max_occupancy {
                            let ref2 = MhnThrowRef {
                                slot: slot2,
                                ..ref1
                            };
                            let Some(throw2) = self.get_by_ref(ref2) else {
                                break;
                            };
                            let primary2 = throw2.primary;
                            let catching2 = throw2.catching;
                            let catch2 = throw2.catch_num;
                            if primary2 != Some(ref2) {
                                break;
                            }
                            if !catching2 {
                                continue;
                            }

                            let switch_catches = if catch1 < catch2 {
                                self.is_catch_order_incorrect(ref1, ref2)?
                            } else {
                                self.is_catch_order_incorrect(ref2, ref1)?
                            };
                            if switch_catches {
                                self.set_catch_num(ref1, catch2);
                                self.set_catch_num(ref2, catch1);
                            }
                        }
                    }
                }
            }
        }

        for throw_ref in self.all_throw_refs() {
            let Some(primary) = self.get_by_ref(throw_ref).and_then(|throw| throw.primary) else {
                continue;
            };
            if primary == throw_ref {
                continue;
            }
            let catch_num = self
                .get_by_ref(primary)
                .ok_or_else(|| "Missing primary throw for catch order".to_string())?
                .catch_num;
            self.set_catch_num(throw_ref, catch_num);
        }

        Ok(())
    }

    fn find_dwell_windows(&mut self) {
        for throw_ref in self.all_throw_refs() {
            let prev_index = if throw_ref.index <= 0 {
                self.period - 1
            } else {
                throw_ref.index as usize - 1
            };
            let mut prev_beat_throw = false;
            for slot in 0..self.max_occupancy {
                let prev_ref = MhnThrowRef {
                    index: prev_index as isize,
                    slot,
                    ..throw_ref
                };
                if self
                    .get_by_ref(prev_ref)
                    .is_some_and(|throw| !throw.is_zero())
                {
                    prev_beat_throw = true;
                }
            }

            if let Some(throw) = self.get_mut_by_ref(throw_ref) {
                throw.dwell_window = if prev_beat_throw { 1 } else { 2 };
            }
        }
    }

    fn is_catch_order_incorrect(
        &self,
        ref1: MhnThrowRef,
        ref2: MhnThrowRef,
    ) -> Result<bool, String> {
        let throw1 = self
            .get_by_ref(ref1)
            .ok_or_else(|| "Missing catch order throw".to_string())?;
        let throw2 = self
            .get_by_ref(ref2)
            .ok_or_else(|| "Missing catch order throw".to_string())?;
        let source1 = throw1
            .source
            .and_then(|link| self.get_link(link))
            .ok_or_else(|| "Missing catch order source".to_string())?;
        let source2 = throw2
            .source
            .and_then(|link| self.get_link(link))
            .ok_or_else(|| "Missing catch order source".to_string())?;

        if source1.index > source2.index {
            return Ok(true);
        }
        if source1.index < source2.index {
            return Ok(false);
        }

        let jdiff1 = throw1.juggler.abs_diff(source1.juggler);
        let jdiff2 = throw2.juggler.abs_diff(source2.juggler);
        if jdiff1 < jdiff2 {
            return Ok(true);
        }
        if jdiff1 > jdiff2 {
            return Ok(false);
        }

        let hdiff1 = throw1.hand.abs_diff(source1.hand);
        let hdiff2 = throw2.hand.abs_diff(source2.hand);
        Ok(hdiff1 > hdiff2)
    }

    fn all_throw_refs(&self) -> Vec<MhnThrowRef> {
        let mut refs = Vec::new();
        for index in 0..self.indexes {
            for juggler_index in 0..self.number_of_jugglers {
                for hand in 0..2 {
                    for slot in 0..self.max_occupancy {
                        if self.throws[juggler_index][hand][index][slot].is_some() {
                            refs.push(MhnThrowRef {
                                juggler: juggler_index + 1,
                                hand,
                                index: index as isize,
                                slot,
                            });
                        }
                    }
                }
            }
        }
        refs
    }

    fn get_by_ref(&self, throw_ref: MhnThrowRef) -> Option<&MhnThrow> {
        if throw_ref.index < 0 {
            return None;
        }
        self.throws
            .get(throw_ref.juggler.checked_sub(1)?)?
            .get(throw_ref.hand)?
            .get(throw_ref.index as usize)?
            .get(throw_ref.slot)?
            .as_ref()
    }

    fn get_mut_by_ref(&mut self, throw_ref: MhnThrowRef) -> Option<&mut MhnThrow> {
        if throw_ref.index < 0 {
            return None;
        }
        self.throws
            .get_mut(throw_ref.juggler.checked_sub(1)?)?
            .get_mut(throw_ref.hand)?
            .get_mut(throw_ref.index as usize)?
            .get_mut(throw_ref.slot)?
            .as_mut()
    }

    fn get_link(&self, link: MhnThrowLink) -> Option<&MhnThrow> {
        match link {
            MhnThrowLink::Matrix(throw_ref) => self.get_by_ref(throw_ref),
            MhnThrowLink::External(index) => self.external_throws.get(index),
        }
    }

    fn min_primary(&self, left: MhnThrowRef, right: MhnThrowRef) -> Result<MhnThrowRef, String> {
        let left_throw = self
            .get_by_ref(left)
            .ok_or_else(|| "Missing primary throw".to_string())?;
        let right_throw = self
            .get_by_ref(right)
            .ok_or_else(|| "Missing primary throw".to_string())?;
        if left_throw <= right_throw {
            Ok(left)
        } else {
            Ok(right)
        }
    }

    fn set_primary(&mut self, throw_ref: MhnThrowRef, primary: MhnThrowRef) {
        if let Some(throw) = self.get_mut_by_ref(throw_ref) {
            throw.primary = Some(primary);
        }
    }

    fn set_source(&mut self, throw_ref: MhnThrowRef, source: MhnThrowLink) {
        if let Some(throw) = self.get_mut_by_ref(throw_ref) {
            throw.source = Some(source);
        }
    }

    fn set_target(&mut self, throw_ref: MhnThrowRef, target: MhnThrowLink) {
        if let Some(throw) = self.get_mut_by_ref(throw_ref) {
            throw.target = Some(target);
        }
    }

    fn set_catch_num(&mut self, throw_ref: MhnThrowRef, catch_num: isize) {
        if let Some(throw) = self.get_mut_by_ref(throw_ref) {
            throw.catch_num = catch_num;
        }
    }

    fn hand_slot_refs(&self, first_ref: MhnThrowRef) -> Vec<MhnThrowRef> {
        let mut refs = Vec::new();
        for slot in 0..self.max_occupancy {
            let throw_ref = MhnThrowRef { slot, ..first_ref };
            if self.get_by_ref(throw_ref).is_none() {
                break;
            }
            refs.push(throw_ref);
        }
        refs
    }
}

fn max_occupancy(spec: &SiteswapSpec) -> usize {
    spec.beats
        .iter()
        .map(|beat| {
            let mut counts = vec![[0usize; 2]; spec.jugglers.max(1)];
            for throw in &beat.throws {
                let juggler = throw.source_juggler.clamp(1, spec.jugglers.max(1));
                let hand = hand_to_index(throw.hand);
                counts[juggler - 1][hand] += 1;
            }
            counts
                .iter()
                .flat_map(|hands| hands.iter())
                .copied()
                .max()
                .unwrap_or(0)
        })
        .max()
        .unwrap_or(1)
}

fn indexed_beats_for_period(spec: &SiteswapSpec, period: usize) -> Vec<(usize, usize)> {
    (0..period)
        .map(|index| (index, index % spec.beats.len()))
        .collect()
}

fn source_hand_for_throw(
    _spec: &SiteswapSpec,
    throw: &ThrowSpec,
    index: usize,
    switch_hands: bool,
) -> Hand {
    if throw.hand_fixed {
        if switch_hands {
            opposite_hand(throw.hand)
        } else {
            throw.hand
        }
    } else if index % 2 == 0 {
        Hand::Right
    } else {
        Hand::Left
    }
}

fn opposite_hand(hand: Hand) -> Hand {
    match hand {
        Hand::Left => Hand::Right,
        Hand::Right => Hand::Left,
    }
}

fn hand_to_index(hand: Hand) -> usize {
    match hand {
        Hand::Right => RIGHT_HAND,
        Hand::Left => LEFT_HAND,
    }
}

fn default_throw_mod(throw: &ThrowSpec, source_hand: Hand, target: Hand) -> Option<String> {
    if let Some(modifier) = &throw.modifier {
        return Some(modifier.clone());
    }

    let mut throw_mod = "T";
    if throw.source_juggler == throw.target_juggler && source_hand == target {
        if throw.value <= 1 {
            throw_mod = "H";
        } else if throw.value == 2 {
            throw_mod = "?";
        }
    }
    Some(throw_mod.to_string())
}

fn wrap_juggler(juggler: usize, number_of_jugglers: usize) -> usize {
    if number_of_jugglers == 0 {
        return 1;
    }
    if juggler == 0 {
        return 1;
    }
    1 + (juggler - 1) % number_of_jugglers
}

fn transition_style(
    throw_mod: &str,
    gravity: f64,
    bouncefrac: f64,
) -> (Option<String>, Option<String>) {
    match throw_mod.chars().next().unwrap_or('T') {
        'B' => {
            let mut params = Vec::new();
            if throw_mod.contains('F') {
                params.push("forced=true".to_string());
            }
            if throw_mod.contains('H') {
                params.push("hyper=true".to_string());
            }
            let bounces = throw_mod.chars().filter(|ch| *ch == 'B').count();
            if bounces > 1 {
                params.push(format!("bounces={bounces}"));
            }
            if (bouncefrac - BOUNCEFRAC_DEFAULT).abs() > f64::EPSILON {
                params.push(format!("bouncefrac={bouncefrac}"));
            }
            if (gravity - GRAVITY_DEFAULT).abs() > f64::EPSILON {
                params.push(format!("g={gravity}"));
            }
            (
                Some("bounce".to_string()),
                (!params.is_empty()).then(|| params.join(";")),
            )
        }
        'F' => {
            let mut params = vec!["forced=true".to_string()];
            if (bouncefrac - BOUNCEFRAC_DEFAULT).abs() > f64::EPSILON {
                params.push(format!("bouncefrac={bouncefrac}"));
            }
            if (gravity - GRAVITY_DEFAULT).abs() > f64::EPSILON {
                params.push(format!("g={gravity}"));
            }
            (Some("bounce".to_string()), Some(params.join(";")))
        }
        'H' => (Some("hold".to_string()), None),
        _ => {
            let throw_mod = if (gravity - GRAVITY_DEFAULT).abs() > f64::EPSILON {
                Some(format!("g={gravity}"))
            } else {
                None
            };
            (Some("toss".to_string()), throw_mod)
        }
    }
}

fn mirror_hand_coordinate(mut coord: Coordinate, hand: usize) -> Coordinate {
    if hand == LEFT_HAND {
        coord.x = -coord.x;
    }
    coord
}

fn mark_path(path_touched: &mut [bool], path_num: isize) {
    if path_num > 0 {
        let index = path_num as usize - 1;
        if index < path_touched.len() {
            path_touched[index] = true;
        }
    }
}

fn resolve_modifiers(
    throws: &mut [Vec<Vec<Vec<Option<MhnThrow>>>>],
    indexes: usize,
    max_occupancy: usize,
) {
    for juggler in 0..throws.len() {
        for hand in 0..2 {
            for index in 0..indexes {
                for slot in 0..max_occupancy {
                    let needs_resolution = throws[juggler][hand][index][slot]
                        .as_ref()
                        .and_then(|throw| throw.throw_mod.as_deref())
                        == Some("?");
                    if !needs_resolution {
                        continue;
                    }

                    let mut do_hold = true;
                    if index + 1 < indexes {
                        for slot2 in 0..max_occupancy {
                            let Some(next_throw) = &throws[juggler][hand][index + 1][slot2] else {
                                continue;
                            };
                            if next_throw.target_index == index as isize + 1 {
                                continue;
                            }
                            do_hold = false;
                            break;
                        }
                    }

                    if let Some(throw) = &mut throws[juggler][hand][index][slot] {
                        throw.throw_mod = Some(if do_hold { "H" } else { "T" }.to_string());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::parse_jml_animation;
    use crate::siteswap::parse_config;

    fn trimmed_pattern_body(xml: &str) -> &str {
        let start = xml.find("<setup").unwrap_or(0);
        let body = &xml[start..];
        let end = body.find("</pattern").unwrap_or(body.len());
        &body[..end]
    }

    #[test]
    fn builds_async_odd_period_switchdelay_matrix() {
        let spec = parse_config("pattern=3").unwrap();
        let matrix = MhnMatrix::from_siteswap(&spec).unwrap();
        assert_eq!(matrix.period, 2);
        assert!(matrix.odd_period_switchdelay);
        assert_eq!(matrix.number_of_paths, 3);
        assert_eq!(matrix.symmetries.len(), 2);
        assert_eq!(matrix.external_throws.len(), 3);

        let right = matrix.get(1, RIGHT_HAND, 0, 0).unwrap();
        assert_eq!(right.target_hand, LEFT_HAND);
        assert_eq!(right.target_index, 3);
        assert!(right.source.is_some());
        assert_eq!(right.path_num, 1);

        let left = matrix.get(1, LEFT_HAND, 1, 0).unwrap();
        assert_eq!(left.target_hand, RIGHT_HAND);
        assert_eq!(left.target_index, 4);
        assert!(left.source.is_some());
        assert_eq!(left.path_num, 2);
    }

    #[test]
    fn preserves_explicit_root_switchdelay_symmetry() {
        let spec = parse_config("pattern=(2,6x)(2x,6)*").unwrap();
        let matrix = MhnMatrix::from_siteswap(&spec).unwrap();

        assert!(!matrix.odd_period_switchdelay);
        assert_eq!(matrix.period, 8);
        let switchdelay = matrix
            .symmetries
            .iter()
            .find(|symmetry| symmetry.symmetry_type == MhnSymmetryType::SwitchDelay)
            .unwrap();
        assert_eq!(switchdelay.delay, 4);
        assert_eq!(switchdelay.jug_perm.as_deref(), Some("(1,1*)"));
    }

    #[test]
    fn leading_hand_spec_still_gets_automatic_switchdelay() {
        let spec = parse_config("pattern=R3").unwrap();
        let matrix = MhnMatrix::from_siteswap(&spec).unwrap();

        assert!(matrix.odd_period_switchdelay);
        assert_eq!(matrix.period, 2);
        assert!(
            matrix
                .symmetries
                .iter()
                .any(|symmetry| symmetry.symmetry_type == MhnSymmetryType::SwitchDelay)
        );
    }

    #[test]
    fn repeats_matrix_period_for_hands() {
        let spec = parse_config("pattern=4;hands=(30)(10).(20)(-20).(40)(-40).").unwrap();
        let matrix = MhnMatrix::from_siteswap(&spec).unwrap();
        assert_eq!(matrix.period, 6);
        assert_eq!(matrix.get(1, RIGHT_HAND, 0, 0).unwrap().hands_beat, 0);
        assert_eq!(matrix.get(1, LEFT_HAND, 1, 0).unwrap().hands_beat, 1);
        assert_eq!(matrix.get(1, RIGHT_HAND, 2, 0).unwrap().hands_beat, 2);
    }

    #[test]
    fn builds_sync_slots_by_hand() {
        let spec = parse_config("pattern=(4,4)").unwrap();
        let matrix = MhnMatrix::from_siteswap(&spec).unwrap();
        assert_eq!(matrix.period, 2);
        assert!(matrix.get(1, LEFT_HAND, 0, 0).is_some());
        assert!(matrix.get(1, RIGHT_HAND, 0, 0).is_some());
        assert!(matrix.get(1, LEFT_HAND, 1, 0).is_none());
    }

    #[test]
    fn resolves_question_modifiers() {
        let spec = parse_config("pattern=2").unwrap();
        let matrix = MhnMatrix::from_siteswap(&spec).unwrap();
        let throw = matrix.get(1, RIGHT_HAND, 0, 0).unwrap();
        assert_eq!(throw.throw_mod.as_deref(), Some("H"));
    }

    #[test]
    fn assigns_catch_and_throw_times() {
        let spec = parse_config("pattern=3;bps=3").unwrap();
        let mut matrix = MhnMatrix::from_siteswap(&spec).unwrap();
        matrix
            .find_catch_throw_times(&TimingConfig {
                bps: spec.bps,
                ..TimingConfig::default()
            })
            .unwrap();

        let throw = matrix.get(1, RIGHT_HAND, 0, 0).unwrap();
        assert_eq!(throw.throw_time, 0.0);
        assert!(throw.catch_time < throw.throw_time);
        assert!(throw.catch_time.is_finite());

        let left = matrix.get(1, LEFT_HAND, 1, 0).unwrap();
        assert!((left.throw_time - (1.0 / 3.0)).abs() < 1e-9);
        assert!(left.catch_time.is_finite());
    }

    #[test]
    fn generates_primary_jml_events_and_path_events() {
        let spec = parse_config("pattern=3;bps=3").unwrap();
        let mut matrix = MhnMatrix::from_siteswap(&spec).unwrap();
        let model = matrix.to_jml_pattern(&spec).unwrap();

        assert_eq!(model.number_of_paths, 3);
        assert_eq!(model.symmetries.len(), 2);
        assert!(model.events.iter().any(|event| {
            event
                .transitions
                .iter()
                .any(|transition| transition.transition_type == MhnJmlTransitionType::Throw)
        }));
        assert!(model.events.iter().any(|event| {
            event
                .transitions
                .iter()
                .any(|transition| transition.transition_type.is_catch())
        }));
        assert_eq!(model.path_events.len(), 3);
        assert!(model.path_events.iter().any(|path| !path.is_empty()));
        assert!(
            model
                .symmetries
                .iter()
                .all(|symmetry| symmetry.path_perm.size() == 3)
        );
    }

    #[test]
    fn generated_events_use_hands_coordinates() {
        let spec = parse_config("pattern=3;hands=(30)(10).(-30)(-10).").unwrap();
        let mut matrix = MhnMatrix::from_siteswap(&spec).unwrap();
        let model = matrix.to_jml_pattern(&spec).unwrap();

        assert!(model.events.iter().any(|event| event.x == 30.0));
        assert!(
            model
                .events
                .iter()
                .any(|event| event.x == 30.0 || event.x == -30.0)
        );
    }

    #[test]
    fn generated_model_contains_body_positions() {
        let spec = parse_config("pattern=3;body=(0,10,20,130)...").unwrap();
        let mut matrix = MhnMatrix::from_siteswap(&spec).unwrap();
        let model = matrix.to_jml_pattern(&spec).unwrap();

        assert!(
            model.positions.iter().any(|position| {
                position.x == 10.0 && position.y == 20.0 && position.z == 130.0
            })
        );
    }

    #[test]
    fn generated_model_contains_props_and_color_assignments() {
        let spec = parse_config("pattern=3;prop=square;propdiam=12;colors=mixed").unwrap();
        let mut matrix = MhnMatrix::from_siteswap(&spec).unwrap();
        let model = matrix.to_jml_pattern(&spec).unwrap();

        assert_eq!(model.props.len(), 3);
        assert_eq!(model.prop_assignment, vec![1, 2, 3]);
        assert_eq!(model.props[0].prop_type, "square");
        assert_eq!(
            model.props[0].modifier.as_deref(),
            Some("diam=12.0;color=red")
        );
    }

    #[test]
    fn generated_jml_model_validates_and_round_trips_to_xml() {
        let spec = parse_config("pattern=3;title=Cascade;colors=mixed").unwrap();
        let mut matrix = MhnMatrix::from_siteswap(&spec).unwrap();
        let model = matrix.to_jml_pattern(&spec).unwrap();

        model.assert_valid().unwrap();
        let xml = model.write_jml(true, true);
        let parsed = parse_jml_animation(&xml).unwrap();

        assert_eq!(parsed.title, "Cascade");
        assert_eq!(parsed.paths, 3);
        assert_eq!(parsed.jugglers, 1);
        assert!(
            parsed
                .events
                .iter()
                .any(|event| !event.transitions.is_empty())
        );
    }

    #[test]
    fn generated_jml_matches_original_mhn_pattern_fixture() {
        let spec = parse_config(
            "pattern=242334;bps=5;dwell=1;hands=(25,-15)(25,-15).(25)(0).(25,65)(25,65).(0)(15).(-25,65)(12.5,20).(15)(25).",
        )
        .unwrap();
        let mut matrix = MhnMatrix::from_siteswap(&spec).unwrap();
        let model = matrix.to_jml_pattern(&spec).unwrap();
        let expected = r#"<setup jugglers="1" paths="3" props="1,1,1"/>
<symmetry type="delay" pperm="(1,3,2)" delay="1.2"/>
<event x="25" y="0" z="-15" t="0" hand="1:right">
<holding path="1"/>
</event>
<event x="-25" y="0" z="0" t="0" hand="1:left">
<catch path="2"/>
</event>
<event x="25" y="0" z="-15" t="0.2" hand="1:right">
<holding path="1"/>
</event>
<event x="-25" y="0" z="0" t="0.2" hand="1:left">
<throw path="2" type="toss"/>
</event>
<event x="25" y="0" z="65" t="0.4" hand="1:right">
<holding path="1"/>
</event>
<event x="0" y="0" z="0" t="0.4" hand="1:left">
<catch path="3"/>
</event>
<event x="25" y="0" z="65" t="0.6" hand="1:right">
<holding path="1"/>
</event>
<event x="0" y="0" z="0" t="0.6" hand="1:left">
<throw path="3" type="toss"/>
</event>
<event x="-25" y="0" z="65" t="0.8" hand="1:right">
<throw path="1" type="toss"/>
</event>
<event x="-15" y="0" z="0" t="0.8" hand="1:left">
<catch path="2"/>
</event>
<event x="12.5" y="0" z="20" t="1" hand="1:right">
<catch path="3"/>
</event>
<event x="-15" y="0" z="0" t="1" hand="1:left">
<throw path="2" type="toss"/>
</event>
"#;

        assert_eq!(expected, trimmed_pattern_body(&model.write_jml(true, true)));
    }

    #[test]
    fn builds_two_juggler_passing_matrix() {
        let spec = parse_config("pattern=<2|0>;title=PUNCH").unwrap();
        let mut matrix = MhnMatrix::from_siteswap(&spec).unwrap();
        assert_eq!(matrix.number_of_jugglers, 2);
        assert_eq!(matrix.number_of_paths, 2);
        assert_eq!(matrix.period, 2);

        let juggler_one = matrix.get(1, RIGHT_HAND, 0, 0).unwrap();
        assert_eq!(juggler_one.target_juggler, 1);
        assert_eq!(juggler_one.target_hand, RIGHT_HAND);
        assert_eq!(juggler_one.throw_value(), 2);

        let juggler_two = matrix.get(2, RIGHT_HAND, 0, 0).unwrap();
        assert_eq!(juggler_two.throw_value(), 0);

        let model = matrix.to_jml_pattern(&spec).unwrap();
        assert_eq!(model.number_of_jugglers, 2);
        assert_eq!(model.number_of_paths, 2);
        model.assert_valid().unwrap();
    }

    #[test]
    fn builds_hss_converted_passing_matrix() {
        let spec = parse_config("pattern=3;hss=114;bps=3").unwrap();
        assert_eq!(spec.jugglers, 1);
        assert!(spec.dwell_array.is_some());

        let mut matrix = MhnMatrix::from_siteswap(&spec).unwrap();
        assert_eq!(matrix.number_of_jugglers, 1);
        let model = matrix.to_jml_pattern(&spec).unwrap();
        assert_eq!(model.number_of_jugglers, 1);
        assert!(!model.events.is_empty());
        assert!(model.events.iter().any(|event| {
            event
                .transitions
                .iter()
                .any(|transition| transition.transition_type == MhnJmlTransitionType::Throw)
        }));
    }
}

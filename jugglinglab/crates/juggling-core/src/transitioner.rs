use crate::generator::{GeneratedPattern, GenerationResult, GenerationStopReason, GeneratorLimits};
use crate::mhn_matrix::MhnMatrix;
use crate::mhn_throw::{MhnThrow, MhnThrowLink};
use crate::siteswap::{SiteswapSpec, parse_config};
use std::cmp::{max, min};
use std::sync::atomic::Ordering;
use std::time::Instant;

const MAX_TRANSITION_MEMORY_BYTES: u64 = 100 * 1024 * 1024;
const LOOP_COUNTER_MAX: usize = 20_000;
const MAX_THROW_VALUE: usize = 35;

type State = Vec<Vec<Vec<usize>>>;

pub fn transition_siteswaps(
    arguments: &str,
    limits: GeneratorLimits,
) -> Result<GenerationResult, String> {
    SiteswapTransitioner::new(arguments, limits)?.run()
}

#[derive(Clone, Debug)]
struct TransitionConfig {
    pattern_from: String,
    pattern_to: String,
    target_occupancy: usize,
    allow_simultaneous_catches: bool,
    allow_clusters: bool,
}

impl TransitionConfig {
    fn parse(arguments: &str) -> Result<Self, String> {
        let args = arguments.split_whitespace().collect::<Vec<_>>();
        if args.len() < 2 {
            return Err("Specify both a from pattern and a to pattern".to_string());
        }
        if args[0] == "-" {
            return Err("The from pattern is required".to_string());
        }
        if args[1] == "-" {
            return Err("The to pattern is required".to_string());
        }

        let mut config = Self {
            pattern_from: args[0].to_string(),
            pattern_to: args[1].to_string(),
            target_occupancy: 1,
            allow_simultaneous_catches: false,
            allow_clusters: true,
        };

        let mut index = 2;
        while index < args.len() {
            match args[index] {
                "-mf" => config.allow_simultaneous_catches = true,
                "-mc" => config.allow_clusters = false,
                // As in the original CLI, the caller decides whether to pass
                // bounded or unlimited execution limits.
                "-limits" => {}
                "-m" => {
                    let value = args
                        .get(index + 1)
                        .filter(|value| !value.starts_with('-'))
                        .ok_or_else(|| "A simultaneous throw count must follow -m".to_string())?;
                    config.target_occupancy = value.parse::<usize>().map_err(|_| {
                        "Simultaneous throws must be a positive integer".to_string()
                    })?;
                    if config.target_occupancy == 0 {
                        return Err("Simultaneous throws must be at least 1".to_string());
                    }
                    index += 1;
                }
                option => return Err(format!("Unrecognized transitioner option: {option}")),
            }
            index += 1;
        }

        Ok(config)
    }
}

#[derive(Debug)]
enum Halt {
    Stop(GenerationStopReason),
    Internal(String),
}

struct SiteswapTransitioner {
    config: TransitionConfig,
    siteswap_from: SiteswapSpec,
    siteswap_to: SiteswapSpec,
    matrix_from: MhnMatrix,
    matrix_to: MhnMatrix,
    jugglers: usize,
    indexes: usize,
    max_occupancy: usize,
    state_from: State,
    state_to: State,
    l_min: usize,
    l_max: usize,
    l_return: usize,

    state: Vec<State>,
    state_target: State,
    l_target: usize,
    throws: Vec<Vec<Vec<Vec<Option<MhnThrow>>>>>,
    throws_left: Vec<Vec<Vec<usize>>>,
    find_all: bool,
    output: Vec<Vec<String>>,
    should_print: Vec<bool>,
    async_hand_right: Vec<Vec<bool>>,
    previous_is_from: bool,
    target_max_filled_index: usize,

    limits: GeneratorLimits,
    patterns_found: usize,
    started_at: Instant,
    loop_counter: usize,
    prefix: String,
    suffix: String,
    patterns: Vec<GeneratedPattern>,
    captured_transition: Option<String>,
}

impl SiteswapTransitioner {
    fn new(arguments: &str, limits: GeneratorLimits) -> Result<Self, String> {
        let config = TransitionConfig::parse(arguments)?;

        let siteswap_from = parse_config(&config.pattern_from)
            .map_err(|error| format!("Error in from pattern: {error}"))?;
        let siteswap_to = parse_config(&config.pattern_to)
            .map_err(|error| format!("Error in to pattern: {error}"))?;
        if siteswap_from.balls != siteswap_to.balls {
            return Err(format!(
                "Patterns have different object counts: {} and {}",
                siteswap_from.balls, siteswap_to.balls
            ));
        }
        if siteswap_from.jugglers != siteswap_to.jugglers {
            return Err(format!(
                "Patterns have different juggler counts: {} and {}",
                siteswap_from.jugglers, siteswap_to.jugglers
            ));
        }

        let matrix_from = MhnMatrix::from_siteswap(&siteswap_from)
            .map_err(|error| format!("Error in from pattern: {error}"))?;
        let matrix_to = MhnMatrix::from_siteswap(&siteswap_to)
            .map_err(|error| format!("Error in to pattern: {error}"))?;
        let jugglers = matrix_from.number_of_jugglers;
        let indexes = max(matrix_from.indexes, matrix_to.indexes);
        let max_occupancy = max(
            config.target_occupancy,
            max(matrix_from.max_occupancy, matrix_to.max_occupancy),
        );
        let state_from = starting_state(&matrix_from, indexes);
        let state_to = starting_state(&matrix_to, indexes);
        let l_min = find_min_length(&state_from, &state_to, jugglers, indexes);
        let l_max = find_max_length(&state_from, jugglers, indexes);
        let l_return = find_min_length(&state_to, &state_from, jugglers, indexes);

        let mut transitioner = Self {
            config,
            siteswap_from,
            siteswap_to,
            matrix_from,
            matrix_to,
            jugglers,
            indexes,
            max_occupancy,
            state_from,
            state_to,
            l_min,
            l_max,
            l_return,
            state: Vec::new(),
            state_target: empty_state(jugglers, indexes),
            l_target: 0,
            throws: Vec::new(),
            throws_left: Vec::new(),
            find_all: false,
            output: Vec::new(),
            should_print: Vec::new(),
            async_hand_right: Vec::new(),
            previous_is_from: true,
            target_max_filled_index: 0,
            limits,
            patterns_found: 0,
            started_at: Instant::now(),
            loop_counter: 0,
            prefix: String::new(),
            suffix: String::new(),
            patterns: Vec::new(),
            captured_transition: None,
        };
        transitioner.allocate_workspace(max(l_max, l_return))?;
        Ok(transitioner)
    }

    fn run(mut self) -> Result<GenerationResult, String> {
        self.started_at = Instant::now();
        let result = self.run_inner();
        match result {
            Ok(()) => Ok(GenerationResult {
                patterns: self.patterns,
                stop_reason: None,
            }),
            Err(Halt::Stop(reason)) => Ok(GenerationResult {
                patterns: self.patterns,
                stop_reason: Some(reason),
            }),
            Err(Halt::Internal(error)) => Err(error),
        }
    }

    fn run_inner(&mut self) -> Result<(), Halt> {
        self.check_stopping()?;
        let return_transition = self.find_return_transition()?;
        self.prefix = format!("({}^2)", self.config.pattern_from);
        self.suffix = format!("({}^2){return_transition}", self.config.pattern_to);

        if self.l_min == 0 {
            let pattern = format!("{}{}", self.prefix, self.suffix);
            self.patterns.push(GeneratedPattern {
                display: pattern.clone(),
                notation: "siteswap".to_string(),
                config: pattern,
            });
            return Ok(());
        }

        self.previous_is_from = true;
        let mut count = 0usize;
        let mut length = self.l_min;
        while length <= self.l_max || count == 0 {
            count += self.find_trans(
                self.state_from.clone(),
                self.state_to.clone(),
                length,
                true,
                true,
            )?;
            length += 1;
        }
        Ok(())
    }

    fn find_return_transition(&mut self) -> Result<String, Halt> {
        if self.l_return == 0 {
            return Ok(String::new());
        }

        self.prefix.clear();
        self.suffix.clear();
        self.previous_is_from = false;
        loop {
            self.captured_transition = None;
            let count = self.find_trans(
                self.state_to.clone(),
                self.state_from.clone(),
                self.l_return,
                false,
                false,
            )?;
            match count {
                0 => self.l_return += 1,
                1 => break,
                _ => {
                    return Err(Halt::Internal(
                        "Return transition search produced more than one result".to_string(),
                    ));
                }
            }
        }
        Ok(strip_final_hands_modifier(
            self.captured_transition.take().unwrap_or_default(),
        ))
    }

    fn allocate_workspace(&mut self, size: usize) -> Result<(), String> {
        let state_values = (size as u64 + 1)
            .saturating_mul(self.jugglers as u64)
            .saturating_mul(2)
            .saturating_mul(self.indexes as u64);
        let target_values = (self.jugglers as u64)
            .saturating_mul(2)
            .saturating_mul(self.indexes as u64);
        let throw_slots = (self.jugglers as u64)
            .saturating_mul(2)
            .saturating_mul(size as u64)
            .saturating_mul(self.max_occupancy as u64);
        let total = state_values
            .saturating_mul(std::mem::size_of::<usize>() as u64)
            .saturating_add(target_values.saturating_mul(std::mem::size_of::<usize>() as u64))
            .saturating_add(
                throw_slots.saturating_mul(std::mem::size_of::<Option<MhnThrow>>() as u64),
            );
        if total > MAX_TRANSITION_MEMORY_BYTES {
            return Err(format!(
                "Transition search requires approximately {} MiB of memory",
                total / (1024 * 1024)
            ));
        }

        self.state = (0..=size)
            .map(|_| empty_state(self.jugglers, self.indexes))
            .collect();
        self.state_target = empty_state(self.jugglers, self.indexes);
        self.throws = vec![vec![vec![vec![None; self.max_occupancy]; size]; 2]; self.jugglers];
        self.throws_left = vec![vec![vec![0; 2]; self.jugglers]; size + 1];
        self.output = vec![vec![String::new(); size]; self.jugglers];
        self.should_print = vec![false; size + 1];
        self.async_hand_right = vec![vec![false; size + 1]; self.jugglers];
        Ok(())
    }

    fn ensure_workspace(&mut self, length: usize) -> Result<(), Halt> {
        if self.state.len() <= length {
            self.allocate_workspace(length).map_err(Halt::Internal)?;
        }
        Ok(())
    }

    fn check_stopping(&mut self) -> Result<(), Halt> {
        if self
            .limits
            .cancelled
            .as_ref()
            .is_some_and(|cancelled| cancelled.load(Ordering::Relaxed))
        {
            return Err(Halt::Stop(GenerationStopReason::Cancelled));
        }
        if let Some(limit) = self.limits.max_time {
            if self.started_at.elapsed() > limit {
                return Err(Halt::Stop(GenerationStopReason::TimeLimit(limit.as_secs())));
            }
        }
        Ok(())
    }

    fn find_trans(
        &mut self,
        from_state: State,
        to_state: State,
        length: usize,
        all: bool,
        emit_public: bool,
    ) -> Result<usize, Halt> {
        self.check_stopping()?;
        self.ensure_workspace(length)?;
        self.l_target = length;
        self.state[0] = from_state;
        self.state_target = to_state;
        self.target_max_filled_index = max_filled_index(&self.state_target, self.indexes);
        self.start_beat(0);
        self.find_all = all;
        self.recurse(0, 0, 0, emit_public)
    }

    fn recurse(
        &mut self,
        mut position: usize,
        mut juggler: usize,
        mut hand: usize,
        emit_public: bool,
    ) -> Result<usize, Halt> {
        self.loop_counter += 1;
        if self.loop_counter > LOOP_COUNTER_MAX {
            self.loop_counter = 0;
            self.check_stopping()?;
        }

        while self.throws_left[position][juggler][hand] == 0 {
            if hand == 1 {
                hand = 0;
                juggler += 1;
            } else {
                hand = 1;
            }

            if juggler == self.jugglers {
                position += 1;
                if position < self.l_target {
                    self.start_beat(position);
                    hand = 0;
                    juggler = 0;
                    continue;
                }

                if self.state[position] == self.state_target {
                    self.output_pattern(emit_public);
                    self.patterns_found += 1;
                    if self
                        .limits
                        .max_patterns
                        .is_some_and(|limit| self.patterns_found == limit)
                    {
                        return Err(Halt::Stop(GenerationStopReason::PatternLimit(
                            self.limits.max_patterns.unwrap_or_default(),
                        )));
                    }
                    return Ok(1);
                }
                return Ok(0);
            }
        }

        let target_index_min = position + 1;
        let target_index_max = min(
            position + min(self.indexes, MAX_THROW_VALUE),
            self.l_target + self.target_max_filled_index,
        );
        let threshold = min(max(self.l_target, target_index_min), target_index_max);
        let mut target_index = threshold;
        let mut count = 0usize;

        loop {
            for target_juggler in 0..self.jugglers {
                for target_hand in 0..2 {
                    let target_slot = self.state[position + 1][target_juggler][target_hand]
                        [target_index - position - 1];
                    let final_index = target_index as isize - self.l_target as isize;
                    if final_index >= 0 && final_index < self.indexes as isize {
                        if target_slot
                            >= self.state_target[target_juggler][target_hand][final_index as usize]
                        {
                            continue;
                        }
                    } else if target_slot >= self.config.target_occupancy {
                        continue;
                    }

                    let slot = self.throws[juggler][hand][position]
                        .iter()
                        .position(Option::is_none)
                        .unwrap_or(self.max_occupancy);
                    if slot == self.max_occupancy {
                        continue;
                    }
                    let candidate = MhnThrow::new(
                        juggler + 1,
                        hand,
                        position as isize,
                        slot,
                        target_juggler + 1,
                        target_hand,
                        target_index as isize,
                        target_slot as isize,
                        None,
                    );
                    if !self.is_throw_valid(position, &candidate) {
                        continue;
                    }

                    self.add_throw(position, candidate.clone());
                    count += self.recurse(position, juggler, hand, emit_public)?;
                    self.remove_throw(position, &candidate);
                    if !self.find_all && count > 0 {
                        return Ok(count);
                    }
                }
            }

            target_index += 1;
            if target_index > target_index_max {
                target_index = target_index_min;
            }
            if target_index == threshold {
                break;
            }
        }
        Ok(count)
    }

    fn is_throw_valid(&self, position: usize, throw: &MhnThrow) -> bool {
        let juggler = throw.juggler - 1;
        let hand = throw.hand;
        let index = throw.index as usize;
        let target_juggler = throw.target_juggler - 1;
        let target_hand = throw.target_hand;
        let target_index = throw.target_index as usize;

        if target_index - index > MAX_THROW_VALUE {
            return false;
        }

        let next_state = if position + 1 == self.l_target {
            &self.state_target
        } else {
            &self.state[position + 1]
        };
        if next_state[juggler][hand][0] > 0
            && (target_juggler != juggler || target_hand != hand || target_index != index + 1)
        {
            return false;
        }

        if position > 0
            && self.state[position - 1][juggler][hand][0] > 0
            && target_juggler == juggler
            && target_hand == hand
            && target_index == index + 1
        {
            return false;
        }

        for previous in self.throws[juggler][hand][position]
            .iter()
            .map_while(Option::as_ref)
        {
            if throw > previous {
                return false;
            }
            if self.max_occupancy > 1 && !self.config.allow_clusters && throw == previous {
                return false;
            }
        }

        if self.config.target_occupancy > 1
            && !self.config.allow_simultaneous_catches
            && self.throws[juggler][hand][position][0].is_some()
            && self.non_hold_catches_at(position, throw) > 1
        {
            return false;
        }

        let is_short_hold =
            target_index == index + 1 && target_juggler == juggler && target_hand == hand;
        if !is_short_hold && target_index >= position + 2 {
            let reserved =
                self.state[position + 1][target_juggler][target_hand][target_index - position - 2];
            let final_index = target_index as isize - self.l_target as isize;
            let mut maximum_slot = self.config.target_occupancy.saturating_sub(1);
            if final_index >= 0 && final_index < self.indexes as isize {
                maximum_slot = self.state_target[target_juggler][target_hand][final_index as usize]
                    .saturating_sub(1);
            }
            if throw.target_slot as usize > maximum_slot.saturating_sub(reserved) {
                return false;
            }
        }
        true
    }

    fn non_hold_catches_at(&self, position: usize, candidate: &MhnThrow) -> usize {
        let mut count = 0usize;
        for juggler in 0..self.jugglers {
            for hand in 0..2 {
                for index in 0..position {
                    for throw in self.throws[juggler][hand][index]
                        .iter()
                        .map_while(Option::as_ref)
                    {
                        if throw.target_juggler == candidate.juggler
                            && throw.target_hand == candidate.hand
                            && throw.target_index == position as isize
                            && !throw.is_hold()
                        {
                            count += 1;
                        }
                    }
                }
            }
        }

        let previous = if self.previous_is_from {
            &self.matrix_from
        } else {
            &self.matrix_to
        };
        for juggler in 0..self.jugglers {
            for hand in 0..2 {
                for index in 0..previous.period {
                    for slot in 0..previous.max_occupancy {
                        let Some(throw) = previous.throws[juggler][hand][index][slot].as_ref()
                        else {
                            break;
                        };
                        let overshoot = throw.target_index - (position + previous.period) as isize;
                        let correct_index =
                            overshoot >= 0 && overshoot % previous.period as isize == 0;
                        if correct_index
                            && throw.target_juggler == candidate.juggler
                            && throw.target_hand == candidate.hand
                            && !throw.is_hold()
                        {
                            count += 1;
                        }
                    }
                }
            }
        }
        count
    }

    fn add_throw(&mut self, position: usize, throw: MhnThrow) {
        let juggler = throw.juggler - 1;
        let hand = throw.hand;
        let target_juggler = throw.target_juggler - 1;
        let target_hand = throw.target_hand;
        let target_index = throw.target_index as usize;
        let slot = throw.slot;
        self.throws[juggler][hand][position][slot] = Some(throw);
        self.throws_left[position][juggler][hand] -= 1;

        for next_position in position + 1..=min(self.l_target, target_index) {
            self.state[next_position][target_juggler][target_hand][target_index - next_position] +=
                1;
        }
    }

    fn remove_throw(&mut self, position: usize, throw: &MhnThrow) {
        let juggler = throw.juggler - 1;
        let hand = throw.hand;
        let target_juggler = throw.target_juggler - 1;
        let target_hand = throw.target_hand;
        let target_index = throw.target_index as usize;
        self.throws[juggler][hand][position][throw.slot] = None;
        self.throws_left[position][juggler][hand] += 1;

        for next_position in position + 1..=min(self.l_target, target_index) {
            self.state[next_position][target_juggler][target_hand][target_index - next_position] -=
                1;
        }
    }

    fn output_pattern(&mut self, emit_public: bool) {
        for position in 0..self.l_target {
            self.output_beat(position);
        }
        let mut transition = String::new();
        if self.jugglers > 1 {
            transition.push('<');
        }
        for juggler in 0..self.jugglers {
            for position in 0..self.l_target {
                transition.push_str(&self.output[juggler][position]);
            }
            if transition.ends_with('/') {
                transition.pop();
            }
            if juggler + 1 < self.jugglers {
                transition.push('|');
            }
        }
        if self.jugglers > 1 {
            transition.push('>');
        }

        if (0..self.jugglers).any(|juggler| !self.async_hand_right[juggler][self.l_target]) {
            if self.jugglers > 1 {
                transition.push('<');
            }
            for juggler in 0..self.jugglers {
                transition.push('R');
                if juggler + 1 < self.jugglers {
                    transition.push('|');
                }
            }
            if self.jugglers > 1 {
                transition.push('>');
            }
        }

        if emit_public {
            let pattern = format!("{}{}{}", self.prefix, transition, self.suffix);
            self.patterns.push(GeneratedPattern {
                display: pattern.clone(),
                notation: "siteswap".to_string(),
                config: pattern,
            });
        } else {
            self.captured_transition = Some(transition);
        }
    }

    fn output_beat(&mut self, position: usize) {
        if !self.should_print[position] {
            self.should_print[position + 1] = true;
            for juggler in 0..self.jugglers {
                self.async_hand_right[juggler][position + 1] =
                    !self.async_hand_right[juggler][position];
                self.output[juggler][position].clear();
            }
            return;
        }

        let have_sync_throw = (0..self.jugglers).any(|juggler| {
            self.throws[juggler][0][position][0].is_some()
                && self.throws[juggler][1][position][0].is_some()
        });
        let have_throw_next_beat = (0..self.jugglers).any(|juggler| {
            self.state[position + 1][juggler][0][0] > 0
                || self.state[position + 1][juggler][1][0] > 0
        });
        let print_double_beat = have_sync_throw && !have_throw_next_beat;
        self.should_print[position + 1] = !print_double_beat;

        for juggler in 0..self.jugglers {
            let mut text = String::new();
            let mut next_right = !self.async_hand_right[juggler][position];
            let right = self.throws[juggler][0][position][0].is_some();
            let left = self.throws[juggler][1][position][0].is_some();
            let hands_throwing = usize::from(right) + usize::from(left);
            let previous_has_hands = if self.previous_is_from {
                has_hands_specifier(&self.siteswap_from)
            } else {
                has_hands_specifier(&self.siteswap_to)
            };

            match hands_throwing {
                0 => {
                    if position == 0 && previous_has_hands {
                        text.push('R');
                    }
                    text.push('0');
                    if print_double_beat {
                        text.push('0');
                    }
                }
                1 => {
                    let needs_slash = if right {
                        if !self.async_hand_right[juggler][position] {
                            text.push('R');
                            next_right = false;
                        } else if position == 0 && previous_has_hands {
                            text.push('R');
                        }
                        self.output_multi_throw(position, juggler, 0, &mut text)
                    } else {
                        if self.async_hand_right[juggler][position] {
                            text.push('L');
                            next_right = true;
                        } else if position == 0 && previous_has_hands {
                            text.push('R');
                        }
                        self.output_multi_throw(position, juggler, 1, &mut text)
                    };
                    if needs_slash {
                        text.push('/');
                    }
                    if print_double_beat {
                        text.push('0');
                    }
                }
                _ => {
                    if position == 0 && previous_has_hands {
                        text.push('R');
                    }
                    text.push('(');
                    self.output_multi_throw(position, juggler, 1, &mut text);
                    text.push(',');
                    self.output_multi_throw(position, juggler, 0, &mut text);
                    text.push(')');
                    if !print_double_beat || position + 1 == self.l_target {
                        text.push('!');
                    }
                }
            }
            self.async_hand_right[juggler][position + 1] = next_right;
            self.output[juggler][position] = text;
        }
    }

    fn output_multi_throw(
        &self,
        position: usize,
        juggler: usize,
        hand: usize,
        output: &mut String,
    ) -> bool {
        let throws = self.throws[juggler][hand][position]
            .iter()
            .map_while(Option::as_ref)
            .collect::<Vec<_>>();
        if throws.is_empty() {
            return false;
        }
        if throws.len() > 1 {
            output.push('[');
        }

        let mut needs_slash = false;
        for (slot, throw) in throws.iter().enumerate() {
            let beats = throw.throw_value() as usize;
            let crossed = (throw.hand == throw.target_hand) ^ (beats % 2 == 0);
            let pass = throw.target_juggler != throw.juggler;
            output.push(digit_for_value(beats));
            if crossed {
                output.push('x');
            }
            if pass {
                output.push('p');
                if self.jugglers > 2 {
                    output.push_str(&throw.target_juggler.to_string());
                }
                if slot + 1 < throws.len() {
                    output.push('/');
                }
                needs_slash = true;
            } else {
                needs_slash = false;
            }
        }
        if throws.len() > 1 {
            output.push(']');
            needs_slash = false;
        }
        needs_slash
    }

    fn start_beat(&mut self, position: usize) {
        if position == 0 {
            self.should_print.fill(false);
            self.should_print[0] = true;
            for hands in &mut self.async_hand_right {
                hands.fill(false);
                hands[0] = true;
            }
        }
        for juggler in 0..self.jugglers {
            for hand in 0..2 {
                let current = self.state[position][juggler][hand].clone();
                self.state[position + 1][juggler][hand].copy_from_slice(&current);
                self.state[position + 1][juggler][hand].rotate_left(1);
                self.state[position + 1][juggler][hand][self.indexes - 1] = 0;
                self.throws_left[position][juggler][hand] = self.state[position][juggler][hand][0];
            }
        }
    }
}

fn empty_state(jugglers: usize, indexes: usize) -> State {
    vec![vec![vec![0; indexes]; 2]; jugglers]
}

fn starting_state(matrix: &MhnMatrix, beats: usize) -> State {
    let mut result = empty_state(matrix.number_of_jugglers, beats);
    for index in matrix.period..matrix.indexes {
        for juggler in 0..matrix.number_of_jugglers {
            for hand in 0..2 {
                for slot in 0..matrix.max_occupancy {
                    let Some(throw) = matrix.throws[juggler][hand][index][slot].as_ref() else {
                        continue;
                    };
                    let source_index = match throw.source {
                        Some(MhnThrowLink::Matrix(source)) => source.index,
                        Some(MhnThrowLink::External(source)) => matrix
                            .external_throws
                            .get(source)
                            .map_or(matrix.period as isize, |throw| throw.index),
                        None => matrix.period as isize,
                    };
                    if source_index < matrix.period as isize && index - matrix.period < beats {
                        result[juggler][hand][index - matrix.period] += 1;
                    }
                }
            }
        }
    }
    result
}

fn find_min_length(from: &State, to: &State, jugglers: usize, indexes: usize) -> usize {
    let mut length = 0usize;
    loop {
        let mut done = true;
        for juggler in 0..jugglers {
            for hand in 0..2 {
                for index in 0..indexes.saturating_sub(length) {
                    if from[juggler][hand][index + length] > to[juggler][hand][index] {
                        done = false;
                    }
                }
            }
        }
        if done {
            return length;
        }
        length += 1;
    }
}

fn find_max_length(from: &State, jugglers: usize, indexes: usize) -> usize {
    let mut length = 0usize;
    for index in 0..indexes {
        for juggler in 0..jugglers {
            for hand in 0..2 {
                if from[juggler][hand][index] > 0 {
                    length = index + 1;
                }
            }
        }
    }
    length
}

fn max_filled_index(state: &State, indexes: usize) -> usize {
    for index in (0..indexes).rev() {
        if state
            .iter()
            .any(|juggler| juggler.iter().any(|hand| hand[index] > 0))
        {
            return index;
        }
    }
    0
}

fn has_hands_specifier(spec: &SiteswapSpec) -> bool {
    spec.beats
        .iter()
        .flat_map(|beat| &beat.throws)
        .any(|throw| throw.hand_fixed)
}

fn digit_for_value(value: usize) -> char {
    char::from_digit(value.min(35) as u32, 36).unwrap_or('?')
}

fn strip_final_hands_modifier(mut transition: String) -> String {
    transition = transition.replace('\n', "");
    if transition.ends_with('R') {
        transition.pop();
    }
    if transition.ends_with('>') {
        if let Some(start) = transition.rfind('<') {
            let modifier = &transition[start + 1..transition.len() - 1];
            if !modifier.is_empty() && modifier.split('|').all(|part| part == "R") {
                transition.truncate(start);
            }
        }
    }
    transition
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, atomic::AtomicBool};

    #[test]
    fn equal_patterns_need_no_transition() {
        let result = transition_siteswaps("3 3", GeneratorLimits::unlimited()).unwrap();
        assert_eq!(result.patterns.len(), 1);
        assert_eq!(result.patterns[0].config, "(3^2)(3^2)");
    }

    #[test]
    fn transition_patterns_round_trip_through_siteswap_parser() {
        let transitioner = SiteswapTransitioner::new("3 51", GeneratorLimits::unlimited()).unwrap();
        assert!(transitioner.l_min > 0);
        let result = transitioner.run().unwrap();
        assert_eq!(
            result
                .patterns
                .iter()
                .map(|pattern| pattern.config.as_str())
                .collect::<Vec<_>>(),
            vec![
                "(3^2)34(51^2)23",
                "(3^2)52(51^2)23",
                "(3^2)4x5x1xR(51^2)23",
                "(3^2)4x1x(4,1x)!R(51^2)23",
                "(3^2)6x3x1xR(51^2)23",
                "(3^2)6x1x(2,1x)!R(51^2)23",
            ]
        );
        for pattern in result.patterns {
            let parsed = parse_config(&pattern.config).unwrap();
            assert_eq!(parsed.balls, 3);
            assert_eq!(parsed.jugglers, 1);
        }
    }

    #[test]
    fn rejects_patterns_with_different_object_counts() {
        let error = transition_siteswaps("3 4", GeneratorLimits::unlimited()).unwrap_err();
        assert!(error.contains("different object counts"));
    }

    #[test]
    fn parses_original_multiplex_options() {
        let config = TransitionConfig::parse("[33] 42 -m 2 -mf -mc").unwrap();
        assert_eq!(config.target_occupancy, 2);
        assert!(config.allow_simultaneous_catches);
        assert!(!config.allow_clusters);
    }

    #[test]
    fn supports_sync_passing_and_multiplex_patterns() {
        for arguments in ["(4,4) (4x,4x)", "<3p|3p> <3p|3p>", "[33] [33] -m 2"] {
            let result = transition_siteswaps(arguments, GeneratorLimits::unlimited())
                .unwrap_or_else(|error| panic!("{arguments}: {error}"));
            assert!(!result.patterns.is_empty(), "{arguments}");
            for pattern in result.patterns {
                parse_config(&pattern.config).unwrap_or_else(|error| {
                    panic!("invalid generated pattern for {arguments}: {error}")
                });
            }
        }
    }

    #[test]
    fn observes_cancellation_before_search_completes() {
        let cancelled = Arc::new(AtomicBool::new(true));
        let result = transition_siteswaps(
            "3 441",
            GeneratorLimits {
                max_patterns: None,
                max_time: None,
                cancelled: Some(cancelled),
            },
        )
        .unwrap();
        assert_eq!(result.stop_reason, Some(GenerationStopReason::Cancelled));
    }

    #[test]
    fn strips_single_and_passing_hands_resets() {
        assert_eq!(strip_final_hands_modifier("12R".to_string()), "12");
        assert_eq!(
            strip_final_hands_modifier("<3|3><R|R>".to_string()),
            "<3|3>"
        );
    }
}

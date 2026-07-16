use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

const ASYNC: usize = 0;
const SYNC: usize = 1;
const MAX_GENERATOR_MEMORY_BYTES: u64 = 100 * 1024 * 1024;
const LOOPS_PER_CHECK: usize = 100;

const MP_EMPTY: i32 = 0;
const MP_THROW: i32 = 1;
const MP_LOWER_BOUND: i32 = 2;
const MP_TYPE: usize = 0;
const MP_FROM: usize = 1;
const MP_VALUE: usize = 2;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedPattern {
    pub display: String,
    pub notation: String,
    pub config: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GenerationStopReason {
    PatternLimit(usize),
    TimeLimit(u64),
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerationResult {
    pub patterns: Vec<GeneratedPattern>,
    pub stop_reason: Option<GenerationStopReason>,
}

#[derive(Clone, Debug)]
pub struct GeneratorLimits {
    pub max_patterns: Option<usize>,
    pub max_time: Option<Duration>,
    pub cancelled: Option<Arc<AtomicBool>>,
}

impl GeneratorLimits {
    pub fn application_defaults() -> Self {
        Self {
            max_patterns: Some(1_000),
            max_time: Some(Duration::from_secs(15)),
            cancelled: None,
        }
    }

    pub fn unlimited() -> Self {
        Self {
            max_patterns: None,
            max_time: None,
            cancelled: None,
        }
    }
}

impl Default for GeneratorLimits {
    fn default() -> Self {
        Self::application_defaults()
    }
}

pub fn generate_siteswaps(
    arguments: &str,
    limits: GeneratorLimits,
) -> Result<GenerationResult, String> {
    SiteswapGenerator::new(arguments, limits)?.run()
}

#[derive(Clone, Debug)]
struct SiteswapGeneratorConfig {
    n: usize,
    jugglers: usize,
    ht: usize,
    l_min: usize,
    l_max: usize,
    exclude: Vec<Regex>,
    include: Vec<Regex>,
    numflag: usize,
    groundflag: usize,
    rotflag: usize,
    fullflag: usize,
    mpflag: usize,
    multiplex: usize,
    delaytime: usize,
    hands: usize,
    max_occupancy: usize,
    leader_person: usize,
    rhythm_repunit: Vec<Vec<usize>>,
    rhythm_period: usize,
    holdthrow: Vec<usize>,
    person_number: Vec<usize>,
    ground_state: Vec<Vec<usize>>,
    ground_state_length: usize,
    mp_clustered_flag: bool,
    lame_flag: bool,
    sequence_flag: bool,
    connected_patterns_flag: bool,
    symmetric_patterns_flag: bool,
    juggler_permutations_flag: bool,
    mode: usize,
    slot_size: usize,
}

impl SiteswapGeneratorConfig {
    fn parse(arguments: &str) -> Result<Self, String> {
        let args = arguments.split_whitespace().collect::<Vec<_>>();
        if args.len() < 3 {
            return Err("Must specify number of balls, max throw, and period".to_string());
        }

        let mut config = Self {
            n: 0,
            jugglers: 1,
            ht: 0,
            l_min: 0,
            l_max: 0,
            exclude: Vec::new(),
            include: Vec::new(),
            numflag: 0,
            groundflag: 0,
            rotflag: 0,
            fullflag: 1,
            mpflag: 1,
            multiplex: 1,
            delaytime: 0,
            hands: 0,
            max_occupancy: 0,
            leader_person: 1,
            rhythm_repunit: Vec::new(),
            rhythm_period: 0,
            holdthrow: Vec::new(),
            person_number: Vec::new(),
            ground_state: Vec::new(),
            ground_state_length: 0,
            mp_clustered_flag: true,
            lame_flag: false,
            sequence_flag: true,
            connected_patterns_flag: false,
            symmetric_patterns_flag: false,
            juggler_permutations_flag: false,
            mode: ASYNC,
            slot_size: 0,
        };

        let true_multiplex = config.parse_flags(&args)?;
        config.configure_mode();
        config.parse_input_config(&args)?;
        config.find_ground();
        config.configure_multiplexing(true_multiplex)?;
        Ok(config)
    }

    fn parse_flags(&mut self, args: &[&str]) -> Result<bool, String> {
        let mut true_multiplex = false;
        let mut i = 3;
        while i < args.len() {
            match args[i] {
                "-n" => self.numflag = 1,
                "-no" => self.numflag = 2,
                "-g" => self.groundflag = 1,
                "-ng" => self.groundflag = 2,
                "-f" => self.fullflag = 0,
                "-prime" => self.fullflag = 2,
                "-rot" => self.rotflag = 1,
                "-jp" => self.juggler_permutations_flag = true,
                "-lame" => self.lame_flag = true,
                "-se" => self.sequence_flag = false,
                "-s" => self.mode = SYNC,
                "-cp" => self.connected_patterns_flag = true,
                "-sym" => self.symmetric_patterns_flag = true,
                "-mf" => self.mpflag = 0,
                "-mc" => self.mp_clustered_flag = false,
                "-mt" => true_multiplex = true,
                "-m" => {
                    if let Some(value) = next_flag_value(args, i) {
                        self.multiplex = parse_usize(value, "simultaneous throws")?;
                        i += 1;
                    }
                }
                "-j" => {
                    if let Some(value) = next_flag_value(args, i) {
                        self.jugglers = parse_usize(value, "jugglers")?;
                        i += 1;
                    }
                }
                "-d" => {
                    if let Some(value) = next_flag_value(args, i) {
                        self.delaytime = parse_usize(value, "passing communication delay")?;
                        self.groundflag = 1;
                        i += 1;
                    }
                }
                "-l" => {
                    if let Some(value) = next_flag_value(args, i) {
                        self.leader_person = parse_usize(value, "passing leader number")?;
                        i += 1;
                    }
                }
                "-x" | "-i" => {
                    let excluded = args[i] == "-x";
                    i += 1;
                    while i < args.len() && !args[i].starts_with('-') {
                        let mut expression = make_standard_regex(args[i]);
                        if excluded {
                            if !expression.contains('^') {
                                expression = format!(".*{expression}.*");
                            }
                        } else {
                            if !expression.contains('^') {
                                expression = format!(".*{expression}");
                            }
                            if !expression.contains('$') {
                                expression.push_str(".*");
                            }
                        }
                        let regex = full_regex(&expression).map_err(|_| {
                            if excluded {
                                "Format error in excluded throws".to_string()
                            } else {
                                "Format error in included throws".to_string()
                            }
                        })?;
                        if excluded {
                            self.exclude.push(regex);
                        } else {
                            self.include.push(regex);
                        }
                        i += 1;
                    }
                    i = i.saturating_sub(1);
                }
                option => return Err(format!("Unrecognized option \"{option}\"")),
            }
            i += 1;
        }
        Ok(true_multiplex)
    }

    fn configure_mode(&mut self) {
        match self.mode {
            ASYNC => {
                self.rhythm_repunit = vec![vec![1]; self.jugglers];
                self.holdthrow = vec![2; self.jugglers];
                self.person_number = (1..=self.jugglers).collect();
                self.hands = self.jugglers;
                self.rhythm_period = 1;
            }
            SYNC => {
                self.hands = 2 * self.jugglers;
                self.rhythm_period = 2;
                self.rhythm_repunit = (0..self.hands).map(|_| vec![1, 0]).collect::<Vec<_>>();
                self.holdthrow = vec![2; self.hands];
                self.person_number = (0..self.hands).map(|hand| hand / 2 + 1).collect();
            }
            _ => unreachable!(),
        }
    }

    fn parse_input_config(&mut self, args: &[&str]) -> Result<(), String> {
        self.n = parse_usize(args[0], "balls")?;
        self.ht = if args[1] == "-" {
            usize::MAX
        } else if args[1].chars().all(|ch| ch.is_ascii_digit()) {
            parse_usize(args[1], "max throw")?
        } else if args[1].len() == 1
            && args[1]
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_lowercase())
        {
            usize::from_str_radix(args[1], 36).map_err(|_| number_format_error("max throw"))?
        } else {
            return Err(number_format_error("max throw"));
        };

        if args[2] == "-" {
            self.l_min = self.rhythm_period;
            self.l_max = usize::MAX;
        } else if let Some(divider) = args[2].find('-') {
            if divider == 0 {
                self.l_min = self.rhythm_period;
                self.l_max = parse_usize(&args[2][1..], "period")?;
            } else if divider == args[2].len() - 1 {
                self.l_min = parse_usize(&args[2][..divider], "period")?;
                self.l_max = usize::MAX;
            } else {
                self.l_min = parse_usize(&args[2][..divider], "period")?;
                self.l_max = parse_usize(&args[2][divider + 1..], "period")?;
            }
        } else {
            self.l_max = parse_usize(args[2], "period")?;
            self.l_min = self.l_max;
        }

        if self.n < 1 {
            return Err("Must have at least 1 object".to_string());
        }
        if self.l_max == usize::MAX {
            if self.fullflag != 2 {
                return Err("Must specify max period if not in prime mode".to_string());
            }
            if self.ht == usize::MAX {
                return Err("Either max throw or period must be specified".to_string());
            }
            self.l_max = binomial_saturating(self.ht.saturating_mul(self.hands), self.n)
                .min(i32::MAX as usize);
            self.l_max -= self.l_max % self.rhythm_period;
        }
        if self.ht == usize::MAX {
            self.ht = self.n.saturating_mul(self.l_max).min(i32::MAX as usize);
        }
        self.ht = self.ht.min(self.n.saturating_mul(self.l_max));
        if self.ht < 1 {
            return Err("Maximum throw must be at least 1".to_string());
        }
        if self.l_min < 1 || self.l_max < 1 || self.l_min > self.l_max {
            return Err("Syntax error in period".to_string());
        }
        if self.jugglers > 1 && !self.juggler_permutations_flag && self.groundflag != 0 {
            return Err("Must include juggler permutations when generating only ground or excited state patterns".to_string());
        }
        if self.l_min % self.rhythm_period != 0 || self.l_max % self.rhythm_period != 0 {
            return Err(format!(
                "Pattern period must be a multiple of {}",
                self.rhythm_period
            ));
        }
        Ok(())
    }

    fn find_ground(&mut self) {
        let mut balls_left = self.n;
        let mut index = 0;
        'length: loop {
            for hand in 0..self.hands {
                if self.rhythm_repunit[hand][index % self.rhythm_period] != 0 {
                    balls_left -= 1;
                    if balls_left == 0 {
                        self.ground_state_length = (index + 1).max(self.ht);
                        break 'length;
                    }
                }
            }
            index += 1;
        }

        self.ground_state = vec![vec![0; self.ground_state_length]; self.hands];
        balls_left = self.n;
        index = 0;
        'state: loop {
            for hand in 0..self.hands {
                if self.rhythm_repunit[hand][index % self.rhythm_period] != 0 {
                    self.ground_state[hand][index] = 1;
                    balls_left -= 1;
                    if balls_left == 0 {
                        break 'state;
                    }
                }
            }
            index += 1;
        }
    }

    fn configure_multiplexing(&mut self, true_multiplex: bool) -> Result<(), String> {
        self.slot_size = self.ht.max(self.l_max);
        self.slot_size += self.rhythm_period - self.slot_size % self.rhythm_period;
        for hand in 0..self.hands {
            for beat in 0..self.rhythm_period {
                self.max_occupancy = self.max_occupancy.max(self.rhythm_repunit[hand][beat]);
            }
        }
        self.max_occupancy *= self.multiplex;
        if self.max_occupancy == 1 {
            self.mpflag = 0;
        }

        if true_multiplex {
            let expression = if self.jugglers == 1 && self.mode == ASYNC {
                Some(r".*\[[^2]*\].*")
            } else if self.jugglers == 1 && self.mode == SYNC {
                Some(r".*\[([^2\]]*2x)*[^2\]]*\].*")
            } else if self.mode == ASYNC {
                Some(r".*\[([^2\]]*(2p|.p2|2p.))*[^2\]]*\].*")
            } else {
                Some(r".*\[([^2\]]*(2p|.p2|2p.|2x|2xp|.xp2|2xp.))*[^2\]]*\].*")
            };
            if let Some(expression) = expression {
                self.include.push(
                    full_regex(expression)
                        .map_err(|_| "Format error in included throws".to_string())?,
                );
            }
        }
        Ok(())
    }
}

fn next_flag_value<'a>(args: &'a [&str], index: usize) -> Option<&'a str> {
    args.get(index + 1)
        .copied()
        .filter(|value| !value.starts_with('-'))
}

fn number_format_error(name: &str) -> String {
    format!("Number format error in \"{name}\" value")
}

fn parse_usize(value: &str, name: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .map_err(|_| number_format_error(name))
}

fn binomial_saturating(a: usize, b: usize) -> usize {
    let mut result = 1_u128;
    for index in 0..b {
        result = result.saturating_mul(a.saturating_sub(index) as u128);
        result /= (index + 1) as u128;
    }
    result.min(usize::MAX as u128) as usize
}

fn make_standard_regex(term: &str) -> String {
    let mut result = term.replace(r"\[", "@");
    result = result.replace('[', r"\[");
    result = result.replace('@', "[");
    result = result.replace(r"\]", "@");
    result = result.replace(']', r"\]");
    result = result.replace('@', "]");
    result = result.replace(r"\(", "@");
    result = result.replace('(', r"\(");
    result = result.replace('@', "(");
    result = result.replace(r"\)", "@");
    result = result.replace(')', r"\)");
    result = result.replace('@', ")");
    result = result.replace(r"\|", "@");
    result = result.replace('|', r"\|");
    result.replace('@', "|")
}

fn full_regex(expression: &str) -> Result<Regex, regex::Error> {
    Regex::new(&format!(r"\A(?:{expression})\z"))
}

#[derive(Clone, Debug, Default)]
struct SearchFrame {
    beat: usize,
    hand: Option<usize>,
    slot: Option<usize>,
    start_buffer_length: usize,
    min_throw: usize,
    min_hand: usize,
    throw_value: usize,
    target_hand: usize,
    num: usize,
    status: usize,
}

#[derive(Debug)]
enum Halt {
    Stop(GenerationStopReason),
    Internal(String),
}

struct SiteswapGenerator {
    config: SiteswapGeneratorConfig,
    limits: GeneratorLimits,
    search_frames: Vec<SearchFrame>,
    state: Vec<Vec<Vec<usize>>>,
    l_target: usize,
    throws_left: Vec<Vec<usize>>,
    holes: Vec<Vec<usize>>,
    throw_to: Vec<Vec<Vec<usize>>>,
    throw_value: Vec<Vec<Vec<usize>>>,
    mp_filter: Vec<Vec<Vec<[i32; 3]>>>,
    connections: Vec<bool>,
    starting_sequence: String,
    ending_sequence: String,
    patterns_found: usize,
    started_at: Instant,
    patterns: Vec<GeneratedPattern>,
}

impl SiteswapGenerator {
    fn new(arguments: &str, limits: GeneratorLimits) -> Result<Self, String> {
        let config = SiteswapGeneratorConfig::parse(arguments)?;
        let max_depth = config
            .l_max
            .checked_mul(config.hands.saturating_mul(config.max_occupancy) + 1)
            .ok_or_else(|| "Memory needed is too large".to_string())?;
        let state_size = (config.l_max as u64 + 1)
            .saturating_mul(config.hands as u64)
            .saturating_mul(config.ground_state_length as u64)
            .saturating_mul(usize::BITS as u64 / 8);
        let holes_size = (config.hands as u64)
            .saturating_mul((config.l_max + config.ht) as u64)
            .saturating_mul(usize::BITS as u64 / 8);
        let throws_size = 2_u64
            .saturating_mul(config.slot_size as u64)
            .saturating_mul(config.hands as u64)
            .saturating_mul(config.max_occupancy as u64)
            .saturating_mul(usize::BITS as u64 / 8);
        let filter_size = if config.mpflag != 0 {
            (config.l_max as u64 + 1)
                .saturating_mul(config.hands as u64)
                .saturating_mul(config.slot_size as u64)
                .saturating_mul(3)
                .saturating_mul(i32::BITS as u64 / 8)
        } else {
            0
        };
        if state_size
            .saturating_add(holes_size)
            .saturating_add(throws_size)
            .saturating_add(filter_size)
            > MAX_GENERATOR_MEMORY_BYTES
        {
            return Err("Memory needed is too large".to_string());
        }

        let state = vec![vec![vec![0; config.ground_state_length]; config.hands]; config.l_max + 1];
        let holes = vec![vec![0; config.l_max + config.ht]; config.hands];
        let throw_to = vec![vec![vec![0; config.max_occupancy]; config.hands]; config.slot_size];
        let throw_value = vec![vec![vec![0; config.max_occupancy]; config.hands]; config.slot_size];
        let mp_filter = if config.mpflag != 0 {
            vec![vec![vec![[0; 3]; config.slot_size]; config.hands]; config.l_max + 1]
        } else {
            Vec::new()
        };
        let throws_left = vec![vec![0; config.hands]; config.l_max];
        let connections = if config.connected_patterns_flag {
            vec![false; config.jugglers]
        } else {
            Vec::new()
        };

        Ok(Self {
            config,
            limits,
            search_frames: vec![SearchFrame::default(); max_depth.max(1)],
            state,
            l_target: 0,
            throws_left,
            holes,
            throw_to,
            throw_value,
            mp_filter,
            connections,
            starting_sequence: String::new(),
            ending_sequence: String::new(),
            patterns_found: 0,
            started_at: Instant::now(),
            patterns: Vec::new(),
        })
    }

    fn run(mut self) -> Result<GenerationResult, String> {
        if self.config.groundflag == 1 && self.config.ground_state_length > self.config.ht {
            return Ok(GenerationResult {
                patterns: Vec::new(),
                stop_reason: None,
            });
        }
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
        self.l_target = self.config.l_min;
        while self.l_target <= self.config.l_max {
            self.find_patterns()?;
            self.l_target += self.config.rhythm_period;
        }
        Ok(())
    }

    fn rhythm(&self, beat: usize, hand: usize, index: usize) -> usize {
        self.config.multiplex
            * self.config.rhythm_repunit[hand][(beat + index) % self.config.rhythm_period]
    }

    fn check_stopping(&self) -> Result<(), Halt> {
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

    fn find_patterns(&mut self) -> Result<usize, Halt> {
        self.check_stopping()?;
        if self.config.groundflag == 1 {
            for hand in 0..self.config.hands {
                for index in 0..self.config.ht {
                    self.state[0][hand][index] = self.config.ground_state[hand][index];
                }
            }
            return self.process_completed_state();
        }

        for hand in 0..self.config.hands {
            self.state[0][hand][..self.config.ht].fill(0);
        }
        if self.config.n == 0 {
            return self.process_completed_state();
        }

        let max_position = self.config.ht * self.config.hands;
        let mut positions = vec![0; self.config.n];
        let mut ball = 0;
        let mut total_patterns = 0;
        let mut check_counter = 0;
        loop {
            check_counter += 1;
            if check_counter > LOOPS_PER_CHECK {
                check_counter = 0;
                self.check_stopping()?;
            }

            if positions[ball] < max_position {
                let position = positions[ball];
                let index = position / self.config.hands;
                let hand = position % self.config.hands;
                if self.state[0][hand][index] < self.rhythm(0, hand, index) {
                    self.state[0][hand][index] += 1;
                    let valid = index < self.l_target
                        || self.state[0][hand][index] <= self.state[0][hand][index - self.l_target];
                    if valid {
                        if ball == self.config.n - 1 {
                            total_patterns += self.process_completed_state()?;
                            self.state[0][hand][index] -= 1;
                            positions[ball] += 1;
                        } else {
                            ball += 1;
                            positions[ball] = positions[ball - 1];
                        }
                    } else {
                        self.state[0][hand][index] -= 1;
                        positions[ball] += 1;
                    }
                } else {
                    positions[ball] += 1;
                }
            } else {
                if ball == 0 {
                    break;
                }
                ball -= 1;
                let previous = positions[ball];
                let index = previous / self.config.hands;
                let hand = previous % self.config.hands;
                self.state[0][hand][index] -= 1;
                positions[ball] += 1;
            }
        }
        Ok(total_patterns)
    }

    fn process_completed_state(&mut self) -> Result<usize, Halt> {
        if self.config.groundflag == 2 && states_equal(&self.state[0], &self.config.ground_state) {
            return Ok(0);
        }

        for hand in 0..self.config.hands {
            let mut index = 0;
            while index < self.config.ht {
                let occupancy = self.state[0][hand][index];
                if self.config.mpflag != 0 && occupancy == 0 {
                    self.mp_filter[0][hand][index][MP_TYPE] = MP_EMPTY;
                } else {
                    if self.config.mpflag != 0 {
                        self.mp_filter[0][hand][index][MP_VALUE] = (index + 1) as i32;
                        self.mp_filter[0][hand][index][MP_FROM] = hand as i32;
                        self.mp_filter[0][hand][index][MP_TYPE] = MP_LOWER_BOUND;
                    }
                    let mut scan = index;
                    loop {
                        scan += self.l_target;
                        if scan >= self.config.ht {
                            break;
                        }
                        let later_occupancy = self.state[0][hand][scan];
                        if later_occupancy > occupancy {
                            return Ok(0);
                        }
                        if self.config.mpflag != 0 && later_occupancy != 0 {
                            if later_occupancy < occupancy && index > self.config.holdthrow[hand] {
                                return Ok(0);
                            }
                            self.mp_filter[0][hand][index][MP_VALUE] = (scan + 1) as i32;
                        }
                    }
                }
                index += 1;
            }
            if self.config.mpflag != 0 {
                while index < self.config.slot_size {
                    self.mp_filter[0][hand][index][MP_TYPE] = MP_EMPTY;
                    index += 1;
                }
            }
        }

        if self.config.numflag != 2 && self.config.sequence_flag {
            self.find_start_end();
        }

        for hand in 0..self.config.hands {
            for target_index in 0..self.l_target + self.config.ht {
                let mut holes = if target_index < self.l_target {
                    self.config.multiplex
                        * self.config.rhythm_repunit[hand][target_index % self.config.rhythm_period]
                } else {
                    self.state[0][hand][target_index - self.l_target]
                };
                if target_index < self.config.ht {
                    holes = holes.saturating_sub(self.state[0][hand][target_index]);
                }
                self.holes[hand][target_index] = holes;
            }
        }
        self.start_beat(0);
        self.find_cycles()
    }

    fn find_cycles(&mut self) -> Result<usize, Halt> {
        let mut buffer = String::new();
        let mut stack_pointer = 0_isize;
        self.search_frames[0] = SearchFrame {
            beat: 0,
            hand: None,
            slot: None,
            start_buffer_length: 0,
            min_throw: 1,
            min_hand: 0,
            throw_value: 1,
            target_hand: 0,
            num: 0,
            status: 0,
        };
        let mut latest_return_value = 0;
        let mut child_returned = false;
        let mut check_counter = 0;

        while stack_pointer >= 0 {
            check_counter += 1;
            if check_counter > LOOPS_PER_CHECK {
                check_counter = 0;
                self.check_stopping()?;
            }
            let frame_index = stack_pointer as usize;
            let mut frame = self.search_frames[frame_index].clone();

            if child_returned {
                child_returned = false;
                if frame.status == 1 {
                    let target_index = frame.beat + frame.throw_value;
                    self.holes[frame.target_hand][target_index] += 1;
                    frame.num += latest_return_value;
                    frame.target_hand += 1;
                    self.search_frames[frame_index] = frame.clone();
                } else if frame.status == 2 {
                    buffer.truncate(frame.start_buffer_length);
                    stack_pointer -= 1;
                    child_returned = true;
                    continue;
                }
            }

            if frame.status == 0 {
                let mut hand = 0;
                while hand < self.config.hands && self.throws_left[frame.beat][hand] == 0 {
                    hand += 1;
                }
                frame.hand = (hand < self.config.hands).then_some(hand);

                if hand == self.config.hands {
                    self.output_beat(frame.beat, &mut buffer);
                    if !self.are_throws_valid(frame.beat, &buffer)
                        || (self.config.mpflag != 0 && !self.is_multiplexing_valid(frame.beat))
                    {
                        buffer.truncate(frame.start_buffer_length);
                        latest_return_value = 0;
                        stack_pointer -= 1;
                        child_returned = true;
                        continue;
                    }
                    self.calculate_state(frame.beat + 1);
                    if !self.is_state_valid(frame.beat + 1) {
                        buffer.truncate(frame.start_buffer_length);
                        latest_return_value = 0;
                        stack_pointer -= 1;
                        child_returned = true;
                        continue;
                    }

                    if frame.beat + 1 < self.l_target {
                        self.start_beat(frame.beat + 1);
                        frame.status = 2;
                        self.search_frames[frame_index] = frame.clone();
                        stack_pointer += 1;
                        let child_index = stack_pointer as usize;
                        self.search_frames[child_index] = SearchFrame {
                            beat: frame.beat + 1,
                            start_buffer_length: buffer.len(),
                            min_throw: 1,
                            throw_value: 1,
                            ..SearchFrame::default()
                        };
                        continue;
                    }

                    let valid = states_equal(&self.state[0], &self.state[self.l_target])
                        && self.is_pattern_valid(&buffer)?;
                    let result = usize::from(valid);
                    if valid {
                        if self.config.numflag != 2 {
                            self.output_pattern(&buffer);
                        }
                        self.patterns_found += 1;
                        if self
                            .limits
                            .max_patterns
                            .is_some_and(|limit| limit == self.patterns_found)
                        {
                            return Err(Halt::Stop(GenerationStopReason::PatternLimit(
                                self.patterns_found,
                            )));
                        }
                    }
                    buffer.truncate(frame.start_buffer_length);
                    latest_return_value = result;
                    stack_pointer -= 1;
                    child_returned = true;
                    continue;
                }

                self.throws_left[frame.beat][hand] -= 1;
                frame.slot = Some(self.throws_left[frame.beat][hand]);
                frame.throw_value = frame.min_throw;
                frame.target_hand = frame.min_hand;
                frame.status = 1;
                self.search_frames[frame_index] = frame.clone();
            }

            if frame.status == 1 {
                let beat = frame.beat;
                let hand = frame
                    .hand
                    .ok_or_else(|| Halt::Internal("Generator frame has no hand".to_string()))?;
                let slot = frame
                    .slot
                    .ok_or_else(|| Halt::Internal("Generator frame has no slot".to_string()))?;
                let mut found_choice = false;

                while frame.throw_value <= self.config.ht {
                    let target_index = beat + frame.throw_value;
                    while frame.target_hand < self.config.hands {
                        let target_hand = frame.target_hand;
                        if self.holes[target_hand][target_index] == 0 {
                            frame.target_hand += 1;
                            continue;
                        }
                        self.holes[target_hand][target_index] -= 1;
                        self.throw_to[beat][hand][slot] = target_hand;
                        self.throw_value[beat][hand][slot] = frame.throw_value;
                        let next_min_throw = if slot != 0 { frame.throw_value } else { 1 };
                        let next_min_hand = if slot != 0 { target_hand } else { 0 };
                        found_choice = true;
                        self.search_frames[frame_index] = frame.clone();
                        stack_pointer += 1;
                        let child_index = stack_pointer as usize;
                        self.search_frames[child_index] = SearchFrame {
                            beat,
                            start_buffer_length: buffer.len(),
                            min_throw: next_min_throw,
                            min_hand: next_min_hand,
                            throw_value: next_min_throw,
                            target_hand: next_min_hand,
                            ..SearchFrame::default()
                        };
                        break;
                    }
                    if found_choice {
                        break;
                    }
                    frame.throw_value += 1;
                    frame.target_hand = 0;
                }

                if !found_choice {
                    self.throws_left[beat][hand] += 1;
                    buffer.truncate(frame.start_buffer_length);
                    latest_return_value = frame.num;
                    stack_pointer -= 1;
                    child_returned = true;
                }
            }
        }
        Ok(latest_return_value)
    }

    fn calculate_state(&mut self, beat: usize) {
        if beat == 0 {
            return;
        }
        for hand in 0..self.config.hands {
            for index in 0..self.config.ht - 1 {
                self.state[beat][hand][index] = self.state[beat - 1][hand][index + 1];
            }
            self.state[beat][hand][self.config.ht - 1] = 0;
        }
        for hand in 0..self.config.hands {
            for slot in 0..self.config.max_occupancy {
                let value = self.throw_value[beat - 1][hand][slot];
                if value == 0 {
                    break;
                }
                self.state[beat][self.throw_to[beat - 1][hand][slot]][value - 1] += 1;
            }
        }
    }

    fn is_state_valid(&self, beat: usize) -> bool {
        if self.config.ht > self.l_target {
            for hand in 0..self.config.hands {
                for index in 0..self.l_target {
                    let mut scan = index;
                    while scan < self.config.ht - self.l_target {
                        if self.state[beat][hand][scan + self.l_target]
                            > self.state[beat][hand][scan]
                        {
                            return false;
                        }
                        scan += self.l_target;
                    }
                }
            }
        }
        if beat % self.config.rhythm_period == 0 {
            if states_equal(&self.state[0], &self.state[beat]) {
                if self.config.fullflag != 0 && beat != self.l_target {
                    return false;
                }
            } else if self.config.rotflag == 0
                && compare_states(&self.state[0], &self.state[beat]) == 1
            {
                return false;
            }
        }
        if self.config.fullflag == 2 {
            for prior in 1..beat {
                if (beat - prior) % self.config.rhythm_period == 0
                    && states_equal(&self.state[prior], &self.state[beat])
                {
                    return false;
                }
            }
        }
        true
    }

    fn start_beat(&mut self, beat: usize) {
        for hand in 0..self.config.hands {
            self.throws_left[beat][hand] = self.state[beat][hand][0];
            for slot in 0..self.config.max_occupancy {
                self.throw_to[beat][hand][slot] = hand;
                self.throw_value[beat][hand][slot] = 0;
            }
        }
    }

    fn is_multiplexing_valid(&mut self, beat: usize) -> bool {
        for hand in 0..self.config.hands {
            for index in 0..self.config.slot_size - 1 {
                self.mp_filter[beat + 1][hand][index] = self.mp_filter[beat][hand][index + 1];
            }
            self.mp_filter[beat + 1][hand][self.config.slot_size - 1][MP_TYPE] = MP_EMPTY;
            let shifted = self.mp_filter[beat][hand][0];
            let hold = self.config.holdthrow[hand];
            if add_throw_mp_filter(
                &mut self.mp_filter[beat + 1][hand][self.l_target - 1],
                hand,
                shifted[MP_TYPE],
                shifted[MP_VALUE],
                shifted[MP_FROM],
                hold,
            ) {
                return false;
            }
        }

        for hand in 0..self.config.hands {
            for slot in 0..self.config.max_occupancy {
                let value = self.throw_value[beat][hand][slot];
                if value == 0 {
                    break;
                }
                let target = self.throw_to[beat][hand][slot];
                let hold = self.config.holdthrow[target];
                if add_throw_mp_filter(
                    &mut self.mp_filter[beat + 1][target][value - 1],
                    target,
                    MP_THROW,
                    value as i32,
                    hand as i32,
                    hold,
                ) {
                    return false;
                }
            }
        }
        true
    }

    fn are_throws_valid(&self, beat: usize, pattern: &str) -> bool {
        if self
            .config
            .exclude
            .iter()
            .any(|expression| expression.is_match(pattern))
        {
            return false;
        }

        if !self.config.mp_clustered_flag {
            for hand in 0..self.config.hands {
                if self.rhythm(beat, hand, 0) == 0 {
                    continue;
                }
                let mut slot = 0;
                while slot < self.config.max_occupancy && self.throw_value[beat][hand][slot] != 0 {
                    for previous in 0..slot {
                        if self.throw_value[beat][hand][slot]
                            == self.throw_value[beat][hand][previous]
                            && self.throw_to[beat][hand][slot]
                                == self.throw_to[beat][hand][previous]
                        {
                            return false;
                        }
                    }
                    slot += 1;
                }
            }
        }

        if self.config.jugglers > 1 && beat < self.config.delaytime {
            let mut balls_thrown = 0;
            for hand in 0..self.config.hands {
                if self.rhythm(beat, hand, 0) != 0 {
                    balls_thrown += 1;
                    if self.state[beat][hand][0] != 1
                        && self.config.person_number[hand] != self.config.leader_person
                    {
                        return false;
                    }
                }
            }
            let mut base_hands = vec![0; self.config.hands];
            let mut base_values = vec![0; self.config.hands];
            let mut balls_left = self.config.n;
            'placing: for index in 0..self.config.ht {
                for hand in 0..self.config.hands {
                    if self.rhythm(beat + 1, hand, index) == 0 {
                        continue;
                    }
                    balls_left -= 1;
                    if balls_left < balls_thrown {
                        base_hands[balls_left] = hand;
                        base_values[balls_left] = index + 1;
                    }
                    if balls_left == 0 {
                        break 'placing;
                    }
                }
            }
            if balls_left != 0 {
                return false;
            }
            for hand in 0..self.config.hands {
                if self.state[beat][hand][0] == 0
                    || self.config.person_number[hand] == self.config.leader_person
                {
                    continue;
                }
                let mut found = false;
                for ball in 0..balls_thrown {
                    if base_hands[ball] == self.throw_to[beat][hand][0]
                        && base_values[ball] == self.throw_value[beat][hand][0]
                    {
                        base_values[ball] = 0;
                        found = true;
                        break;
                    }
                }
                if !found {
                    return false;
                }
            }
        }
        true
    }

    fn is_pattern_valid(&mut self, pattern: &str) -> Result<bool, Halt> {
        if self
            .config
            .include
            .iter()
            .any(|expression| !expression.is_match(pattern))
        {
            return Ok(false);
        }

        if self.config.mode == ASYNC && self.config.lame_flag && self.config.max_occupancy == 1 {
            for beat in 0..self.l_target - 1 {
                for hand in 0..self.config.hands {
                    if self.throw_value[beat][hand][0] == 1
                        && self.config.person_number[self.throw_to[beat][hand][0]]
                            == self.config.person_number[hand]
                        && self.throw_value[beat + 1][hand][0] == 1
                        && self.config.person_number[self.throw_to[beat + 1][hand][0]]
                            == self.config.person_number[hand]
                    {
                        return Ok(false);
                    }
                }
            }
        }

        if self.config.fullflag == 0 && self.config.rotflag == 0 {
            for beat in 1..self.l_target {
                if beat % self.config.rhythm_period == 0
                    && states_equal(&self.state[0], &self.state[beat])
                    && self.compare_rotations(0, beat)? < 0
                {
                    return Ok(false);
                }
            }
        }

        if self.config.jugglers > 1 && self.config.connected_patterns_flag {
            self.connections.fill(false);
            self.connections[0] = true;
            let mut changed = true;
            while changed {
                changed = false;
                for beat in 0..self.l_target {
                    for hand in 0..self.config.hands {
                        let person = self.config.person_number[hand] - 1;
                        if self.connections[person] {
                            continue;
                        }
                        let mut slot = 0;
                        while slot < self.config.max_occupancy
                            && self.throw_value[beat][hand][slot] > 0
                        {
                            let target_person =
                                self.config.person_number[self.throw_to[beat][hand][slot]] - 1;
                            if self.connections[target_person] {
                                self.connections[person] = true;
                                changed = true;
                            }
                            slot += 1;
                        }
                    }
                }
            }
            if self.connections.iter().any(|connected| !connected) {
                return Ok(false);
            }
        }

        if self.config.jugglers > 1 && !self.config.juggler_permutations_flag {
            let mut done_first = vec![false; self.config.l_max];
            let mut done_second = vec![false; self.config.l_max];
            'pairs: for person in 1..self.config.jugglers {
                done_first[..self.l_target].fill(false);
                done_second[..self.l_target].fill(false);
                for _ in 0..self.l_target {
                    let mut score_first = -1_i64;
                    let mut score_second = -1_i64;
                    let mut max_first = 0;
                    let mut max_second = 0;
                    for beat in 0..self.l_target {
                        if !done_first[beat] {
                            let score = self.juggler_score(beat, person);
                            if score > score_first {
                                score_first = score;
                                max_first = beat;
                            }
                        }
                        if !done_second[beat] {
                            let score = self.juggler_score(beat, person + 1);
                            if score > score_second {
                                score_second = score;
                                max_second = beat;
                            }
                        }
                    }
                    if score_second > score_first {
                        return Ok(false);
                    }
                    if score_second < score_first {
                        continue 'pairs;
                    }
                    done_first[max_first] = true;
                    done_second[max_second] = true;
                }
            }
        }

        if self.config.jugglers > 1 && self.config.symmetric_patterns_flag {
            'person: for person in 2..=self.config.jugglers {
                'offset: for offset in 0..self.l_target {
                    for beat in 0..self.l_target {
                        let mut first_hand = 0;
                        let shifted = (beat + offset) % self.l_target;
                        for hand in 1..self.config.hands {
                            if self.config.person_number[hand] != person {
                                continue;
                            }
                            for slot in 0..self.config.max_occupancy {
                                let first_value = self.throw_value[beat][first_hand][slot];
                                let first_self = self.config.person_number
                                    [self.throw_to[beat][first_hand][slot]]
                                    == 1;
                                let first_same =
                                    self.throw_to[beat][first_hand][slot] == first_hand;
                                let value = self.throw_value[shifted][hand][slot];
                                let self_throw = self.config.person_number
                                    [self.throw_to[shifted][hand][slot]]
                                    == person;
                                let same = self.throw_to[shifted][hand][slot] == hand;
                                if first_value == 0 && value == 0 {
                                    break;
                                }
                                if first_value != value
                                    || first_self != self_throw
                                    || first_same != same
                                {
                                    continue 'offset;
                                }
                            }
                            first_hand += 1;
                        }
                    }
                    continue 'person;
                }
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn juggler_score(&self, beat: usize, person: usize) -> i64 {
        let occupancy_factor = 2 * self.config.max_occupancy;
        let mut score = 0_i64;
        for hand in 0..self.config.hands {
            if self.config.person_number[hand] != person {
                continue;
            }
            let mut slot = 0;
            while slot < self.config.max_occupancy && self.throw_value[beat][hand][slot] > 0 {
                score +=
                    (4 * self.throw_value[beat][hand][slot] * occupancy_factor * occupancy_factor)
                        as i64;
                if self.throw_to[beat][hand][slot] != hand {
                    score += (2 * occupancy_factor) as i64;
                    if self.config.person_number[self.throw_to[beat][hand][slot]] != person {
                        score += 1;
                    }
                }
                slot += 1;
            }
        }
        score
    }

    fn compare_rotations(&self, first: usize, second: usize) -> Result<i32, Halt> {
        let mut offset = 0;
        while offset < self.l_target {
            self.check_stopping()?;
            let result = self.compare_loops(
                (first + offset) % self.l_target,
                (second + offset) % self.l_target,
            );
            if result != 0 {
                return Ok(result);
            }
            offset += 1;
            while offset < self.l_target
                && !states_equal(
                    &self.state[first],
                    &self.state[(first + offset) % self.l_target],
                )
            {
                offset += 1;
            }
        }
        Ok(0)
    }

    fn compare_loops(&self, first: usize, second: usize) -> i32 {
        let mut current_first = first;
        let mut current_second = second;
        let state_start = &self.state[first];
        let mut result = 0;
        let mut length = 0;
        loop {
            length += 1;
            if current_first + 1 >= self.state.len() || current_second + 1 >= self.state.len() {
                return result;
            }
            if result == 0 {
                result = self.compare_throws(current_first, current_second);
            }
            if length % self.config.rhythm_period == 0 {
                let first_equal = states_equal(&self.state[current_first + 1], state_start);
                let second_equal = states_equal(&self.state[current_second + 1], state_start);
                if first_equal {
                    return if second_equal { result } else { -1 };
                }
                if second_equal {
                    return 1;
                }
            }
            current_first += 1;
            current_second += 1;
        }
    }

    fn compare_throws(&self, first: usize, second: usize) -> i32 {
        for hand in 0..self.config.hands {
            for slot in 0..self.rhythm(first, hand, 0) {
                match self.throw_value[first][hand][slot].cmp(&self.throw_value[second][hand][slot])
                {
                    std::cmp::Ordering::Greater => return 1,
                    std::cmp::Ordering::Less => return -1,
                    std::cmp::Ordering::Equal => {}
                }
                match self.throw_to[first][hand][slot].cmp(&self.throw_to[second][hand][slot]) {
                    std::cmp::Ordering::Greater => return 1,
                    std::cmp::Ordering::Less => return -1,
                    std::cmp::Ordering::Equal => {}
                }
            }
        }
        0
    }

    fn output_throw_value(value: usize, buffer: &mut String) {
        if value > 35 {
            buffer.push('{');
            buffer.push_str(&value.to_string());
            buffer.push('}');
        } else if value < 10 {
            buffer.push((b'0' + value as u8) as char);
        } else {
            buffer.push((b'a' + (value - 10) as u8) as char);
        }
    }

    fn output_beat(&self, beat: usize, buffer: &mut String) {
        if !(0..self.config.hands).any(|hand| self.rhythm(beat, hand, 0) != 0) {
            return;
        }
        let mut x_space = !buffer.is_empty();
        if self.config.jugglers > 1 {
            buffer.push('<');
            x_space = false;
        }

        for person in 1..=self.config.jugglers {
            let low_hand = self
                .config
                .person_number
                .iter()
                .position(|candidate| *candidate == person)
                .unwrap_or(0);
            let mut high_hand = low_hand;
            while high_hand < self.config.hands && self.config.person_number[high_hand] == person {
                high_hand += 1;
            }
            let throwing_hands = (low_hand..high_hand)
                .filter(|hand| self.rhythm(beat, *hand, 0) != 0)
                .count();
            if throwing_hands > 0 {
                let parenthesized = throwing_hands > 1;
                if parenthesized {
                    buffer.push('(');
                    x_space = false;
                }
                for hand in low_hand..high_hand {
                    if self.rhythm(beat, hand, 0) == 0 {
                        continue;
                    }
                    let multiplex =
                        self.config.max_occupancy > 1 && self.throw_value[beat][hand][1] > 0;
                    if multiplex {
                        buffer.push('[');
                        x_space = false;
                    }
                    let mut got_throw = false;
                    let mut slot = 0;
                    while slot < self.config.max_occupancy && self.throw_value[beat][hand][slot] > 0
                    {
                        got_throw = true;
                        let value = self.throw_value[beat][hand][slot];
                        if value == 33 && x_space {
                            buffer.push(' ');
                        }
                        Self::output_throw_value(value, buffer);
                        x_space = true;

                        if self.config.hands > 1 {
                            let target = self.throw_to[beat][hand][slot];
                            let target_person = self.config.person_number[target];
                            if self.config.mode == SYNC {
                                let mut scan = target as isize - 1;
                                let mut destination_hand = 0;
                                while scan >= 0
                                    && self.config.person_number[scan as usize] == target_person
                                {
                                    scan -= 1;
                                    destination_hand += 1;
                                }
                                if destination_hand != hand - low_hand {
                                    buffer.push('x');
                                }
                            }
                            if target_person != person {
                                buffer.push('p');
                                if self.config.jugglers > 2 {
                                    buffer.push_str(&target_person.to_string());
                                }
                            }
                        }
                        if multiplex
                            && self.config.jugglers > 1
                            && slot != self.config.max_occupancy - 1
                            && self.throw_value[beat][hand][slot + 1] > 0
                        {
                            buffer.push('/');
                            x_space = false;
                        }
                        slot += 1;
                    }
                    if !got_throw {
                        buffer.push('0');
                        x_space = true;
                    }
                    if multiplex {
                        buffer.push(']');
                        x_space = false;
                    }
                    if hand < high_hand - 1 && parenthesized {
                        buffer.push(',');
                        x_space = false;
                    }
                }
                if parenthesized {
                    buffer.push(')');
                }
            }
            if person < self.config.jugglers {
                buffer.push('|');
            }
        }
        if self.config.jugglers > 1 {
            buffer.push('>');
        }
    }

    fn output_pattern(&mut self, pattern: &str) {
        let mut display = String::new();
        let mut excited = false;
        if self.config.groundflag != 1 {
            if self.config.sequence_flag {
                if self.config.mode == ASYNC {
                    display.push_str(
                        &" ".repeat(self.config.n.saturating_sub(self.starting_sequence.len())),
                    );
                }
                display.push_str(&self.starting_sequence);
                display.push_str("  ");
            } else {
                excited = !states_equal(&self.config.ground_state, &self.state[0]);
                display.push_str(if excited { "* " } else { "  " });
            }
        }
        display.push_str(pattern);
        if self.config.groundflag != 1 {
            if self.config.sequence_flag {
                display.push_str("  ");
                display.push_str(&self.ending_sequence);
                if self.config.mode == ASYNC {
                    display.push_str(
                        &" ".repeat(self.config.n.saturating_sub(self.ending_sequence.len())),
                    );
                }
            } else {
                display.push_str(if excited { " *" } else { "  " });
            }
        }
        self.patterns.push(GeneratedPattern {
            display,
            notation: "siteswap".to_string(),
            config: pattern
                .trim_matches(|character: char| character <= ' ')
                .to_string(),
        });
    }

    fn find_start_end(&mut self) {
        let mut start_beats = 0;
        'find_start: loop {
            for hand in 0..self.config.hands {
                for index in 0..self.config.ht {
                    self.state[1][hand][index] =
                        if index + start_beats < self.config.ground_state_length {
                            self.config.ground_state[hand][index + start_beats]
                        } else {
                            0
                        };
                    if self.state[1][hand][index] > self.state[0][hand][index] {
                        start_beats += self.config.rhythm_period;
                        continue 'find_start;
                    }
                    self.state[1][hand][index] =
                        self.state[0][hand][index] - self.state[1][hand][index];
                }
            }
            break;
        }

        for beat in 0..start_beats {
            for hand in 0..self.config.hands {
                for slot in 0..self.config.max_occupancy {
                    self.throw_value[beat][hand][slot] = 0;
                    self.throw_to[beat][hand][slot] = hand;
                }
                if beat >= self.config.ground_state_length
                    || self.config.ground_state[hand][beat] == 0
                {
                    continue;
                }
                'destination: for index in 0..self.config.ht {
                    for target in 0..self.config.hands {
                        if self.state[1][target][index] > 0 {
                            self.state[1][target][index] -= 1;
                            self.throw_value[beat][hand][0] = index + start_beats - beat;
                            self.throw_to[beat][hand][0] = target;
                            break 'destination;
                        }
                    }
                }
            }
        }
        let mut starting = String::new();
        for beat in 0..start_beats {
            self.output_beat(beat, &mut starting);
        }
        self.starting_sequence = starting;

        let mut end_beats = 0;
        'find_end: loop {
            for hand in 0..self.config.hands {
                for index in 0..self.config.ground_state_length {
                    self.state[1][hand][index] = if index + end_beats < self.config.ht {
                        self.state[0][hand][index + end_beats]
                    } else {
                        0
                    };
                    if self.state[1][hand][index] > self.config.ground_state[hand][index] {
                        end_beats += self.config.rhythm_period;
                        continue 'find_end;
                    }
                    self.state[1][hand][index] =
                        self.config.ground_state[hand][index] - self.state[1][hand][index];
                }
            }
            break;
        }

        for beat in 0..end_beats {
            for hand in 0..self.config.hands {
                for slot in 0..self.config.max_occupancy {
                    self.throw_value[beat][hand][slot] = 0;
                    self.throw_to[beat][hand][slot] = hand;
                }
                if beat >= self.config.ht {
                    continue;
                }
                for slot in 0..self.state[0][hand][beat] {
                    'destination: for index in 0..self.config.ground_state_length {
                        for target in 0..self.config.hands {
                            if self.state[1][target][index] > 0 {
                                self.state[1][target][index] -= 1;
                                self.throw_value[beat][hand][slot] = index + end_beats - beat;
                                self.throw_to[beat][hand][slot] = target;
                                break 'destination;
                            }
                        }
                    }
                }
            }
        }
        let mut ending = String::new();
        for beat in 0..end_beats {
            self.output_beat(beat, &mut ending);
        }
        self.ending_sequence = ending;
    }
}

fn add_throw_mp_filter(
    destination: &mut [i32; 3],
    slot_hand: usize,
    throw_type: i32,
    value: i32,
    from: i32,
    holdthrow: usize,
) -> bool {
    match throw_type {
        MP_EMPTY => false,
        MP_LOWER_BOUND => {
            if destination[MP_TYPE] == MP_EMPTY {
                destination[MP_TYPE] = MP_LOWER_BOUND;
                destination[MP_VALUE] = value;
                destination[MP_FROM] = from;
            }
            false
        }
        MP_THROW => {
            if from == slot_hand as i32 && value == holdthrow as i32 {
                return false;
            }
            match destination[MP_TYPE] {
                MP_EMPTY => {
                    destination[MP_TYPE] = MP_THROW;
                    destination[MP_VALUE] = value;
                    destination[MP_FROM] = from;
                    false
                }
                MP_LOWER_BOUND
                    if destination[MP_VALUE] <= value
                        || destination[MP_VALUE] <= holdthrow as i32 =>
                {
                    destination[MP_TYPE] = MP_THROW;
                    destination[MP_VALUE] = value;
                    destination[MP_FROM] = from;
                    false
                }
                MP_THROW if destination[MP_FROM] == from && destination[MP_VALUE] == value => false,
                _ => true,
            }
        }
        _ => true,
    }
}

fn states_equal(left: &[Vec<usize>], right: &[Vec<usize>]) -> bool {
    left == right
}

fn compare_states(left: &[Vec<usize>], right: &[Vec<usize>]) -> i32 {
    let max_left = left.iter().flatten().copied().max().unwrap_or(0);
    let max_right = right.iter().flatten().copied().max().unwrap_or(0);
    match max_left.cmp(&max_right) {
        std::cmp::Ordering::Greater => return 1,
        std::cmp::Ordering::Less => return -1,
        std::cmp::Ordering::Equal => {}
    }
    let height = left.first().map(Vec::len).unwrap_or(0);
    for index in (0..height).rev() {
        for hand in (0..left.len()).rev() {
            match left[hand][index].cmp(&right[hand][index]) {
                std::cmp::Ordering::Greater => return 1,
                std::cmp::Ordering::Less => return -1,
                std::cmp::Ordering::Equal => {}
            }
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patterns(arguments: &str) -> Vec<String> {
        generate_siteswaps(arguments, GeneratorLimits::unlimited())
            .unwrap()
            .patterns
            .into_iter()
            .map(|pattern| pattern.config)
            .collect()
    }

    #[test]
    fn generates_original_three_ball_async_case() {
        let generated = patterns("3 5 6 -se -f");
        assert_eq!(generated.len(), 55);
        assert_eq!(generated[10], "444042");
    }

    #[test]
    fn matches_siteswap_counting_rule() {
        let generated = patterns("4 12 3 -f -rot -se");
        assert_eq!(generated.len(), 5_usize.pow(3) - 4_usize.pow(3));

        let generated = patterns("5 25 5 -f -rot -se");
        assert_eq!(generated.len(), 6_usize.pow(5) - 5_usize.pow(5));
    }

    #[test]
    fn generates_prime_height_limited_patterns() {
        let generated = patterns("5 7 - -prime -se");
        assert_eq!(generated.len(), 337);
        assert_eq!(generated.last().unwrap(), "777717077717707740");
    }

    #[test]
    fn generates_prime_period_limited_patterns() {
        assert_eq!(patterns("4 24 6 -prime -se").len(), 1663);
    }

    #[test]
    fn applies_regex_and_passing_filters() {
        assert_eq!(patterns("5 3 4 -j 2 -f -cp -x <3p|.*>").len(), 7);
        assert_eq!(patterns("5 7 4 -f").len(), 17);
        assert_eq!(patterns(r"5 7 4 -m 2 -f -x [").len(), 17);
    }

    #[test]
    fn applies_original_multiplex_filters() {
        assert_eq!(patterns("5 5 3 -m 2 -f").len(), 23);
        assert_eq!(patterns("5 5 3 -m 2 -f -mt").len(), 16);
        assert_eq!(patterns("5 5 3 -m 2 -f -mt -mc").len(), 5);
    }

    #[test]
    fn stops_at_pattern_limit_without_discarding_results() {
        let result = generate_siteswaps(
            "3 5 6 -se -f",
            GeneratorLimits {
                max_patterns: Some(10),
                max_time: None,
                cancelled: None,
            },
        )
        .unwrap();
        assert_eq!(result.patterns.len(), 10);
        assert_eq!(
            result.stop_reason,
            Some(GenerationStopReason::PatternLimit(10))
        );
    }
}

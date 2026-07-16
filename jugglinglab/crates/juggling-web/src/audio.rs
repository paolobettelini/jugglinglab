use std::cell::RefCell;
use web_sys::HtmlAudioElement;

const CLIP_POOL_SIZE: usize = 16;
const CATCH_SOUND_SOURCE: &str = "/assets/sounds/catch.wav";
const BOUNCE_SOUND_SOURCE: &str = "/assets/sounds/bounce.wav";

struct AudioPool {
    clips: Vec<HtmlAudioElement>,
    next: usize,
}

impl AudioPool {
    fn new(source: &str) -> Option<Self> {
        let clips = (0..CLIP_POOL_SIZE)
            .map(|_| {
                let clip = HtmlAudioElement::new_with_src(source).ok()?;
                clip.set_preload("auto");
                clip.load();
                Some(clip)
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self { clips, next: 0 })
    }

    fn play(&mut self, volume: f64) {
        let Some(clip) = self.clips.get(self.next) else {
            return;
        };
        self.next = (self.next + 1) % self.clips.len();
        clip.pause().ok();
        clip.set_current_time(0.0);
        clip.set_volume(volume.clamp(0.0, 1.0));
        let _ = clip.play();
    }
}

thread_local! {
    static CATCH_POOL: RefCell<Option<AudioPool>> = const { RefCell::new(None) };
    static BOUNCE_POOL: RefCell<Option<AudioPool>> = const { RefCell::new(None) };
}

pub fn prepare_catch() {
    prepare(&CATCH_POOL, CATCH_SOUND_SOURCE);
}

pub fn prepare_bounce() {
    prepare(&BOUNCE_POOL, BOUNCE_SOUND_SOURCE);
}

pub fn play_catch(volume: f64) {
    play(&CATCH_POOL, CATCH_SOUND_SOURCE, volume);
}

pub fn play_bounce(volume: f64) {
    play(&BOUNCE_POOL, BOUNCE_SOUND_SOURCE, volume);
}

fn prepare(pool: &'static std::thread::LocalKey<RefCell<Option<AudioPool>>>, source: &str) {
    pool.with(|pool| {
        let mut pool = pool.borrow_mut();
        if pool.is_none() {
            *pool = AudioPool::new(source);
        }
    });
}

fn play(
    pool: &'static std::thread::LocalKey<RefCell<Option<AudioPool>>>,
    source: &str,
    volume: f64,
) {
    prepare(pool, source);
    pool.with(|pool| {
        if let Some(pool) = pool.borrow_mut().as_mut() {
            pool.play(volume);
        }
    });
}

use serde_json::{Value, json};

pub const TAG_INPUT: i64 = 1;
pub const TAG_BUTTON: i64 = 4;

const MAX_SAMPLES: usize = 60;
const SAMPLE_INTERVAL_MS: f64 = 50.0;

#[derive(Debug, Clone)]
pub struct Options {
    pub width: f64,
    pub height: f64,
    pub target_x: f64,
    pub target_y: f64,
    pub duration_ms: f64,
    pub start_ms: f64,
    pub touch: bool,
    pub scroll: bool,
    pub now_ms: u64,
}

pub struct Random(u64);

impl Random {
    pub fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn next(&mut self) -> u64 {
        let mut state = self.0;
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        self.0 = state;
        state
    }

    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, low: f64, high: f64) -> f64 {
        low + self.unit() * (high - low)
    }
}

pub fn synthesize(options: &Options, random: &mut Random) -> Value {
    let steps = ((options.duration_ms / SAMPLE_INTERVAL_MS).floor() as usize).clamp(6, MAX_SAMPLES);

    let start = (
        random.range(options.width * 0.05, options.width * 0.95),
        random.range(options.height * 0.55, options.height * 0.95),
    );
    let control = (
        random.range(start.0.min(options.target_x), start.0.max(options.target_x)),
        (start.1 + options.target_y) / 2.0 - random.range(20.0, 140.0),
    );

    let mut pointer = Vec::with_capacity(steps);
    let mut touch = Vec::with_capacity(steps);
    let mut time = options.start_ms;

    for step in 0..steps {
        let progress = ease((step as f64 + 1.0) / steps as f64);
        let inverse = 1.0 - progress;

        let x = inverse * inverse * start.0
            + 2.0 * inverse * progress * control.0
            + progress * progress * options.target_x
            + random.range(-1.5, 1.5);
        let y = inverse * inverse * start.1
            + 2.0 * inverse * progress * control.1
            + progress * progress * options.target_y
            + random.range(-1.5, 1.5);

        time += SAMPLE_INTERVAL_MS + random.range(0.0, 34.0);

        if options.touch {
            touch.push(json!([
                x.round(),
                y.round(),
                time.round(),
                (random.range(0.15, 0.6) * 1000.0).round() / 1000.0,
                random.range(8.0, 24.0).round(),
                random.range(8.0, 24.0).round(),
            ]));
        } else {
            pointer.push(json!([x.round(), y.round(), time.round()]));
        }
    }

    let mut scroll = Vec::new();
    if options.scroll {
        let mut offset = 0.0;
        let mut scroll_time = options.start_ms + random.range(0.0, 400.0);
        for _ in 0..random.range(2.0, 6.0).round() as usize {
            offset += random.range(40.0, 220.0);
            scroll_time += SAMPLE_INTERVAL_MS + random.range(0.0, 90.0);
            scroll.push(json!([offset.round(), scroll_time.round()]));
        }
    }

    let focus_elapsed = random.range(0.0, 60.0);
    let focus = vec![
        json!([0, 0, if options.touch { TAG_BUTTON } else { TAG_INPUT }, 1]),
        json!([focus_elapsed.round(), 0, TAG_BUTTON, 1]),
    ];

    json!({
        "focus": focus,
        "maxTouchPoints": if options.touch { 5 } else { 0 },
        "pointer": pointer,
        "scroll": scroll,
        "time": options.now_ms,
        "touch": touch,
    })
}

fn ease(progress: f64) -> f64 {
    if progress < 0.5 {
        2.0 * progress * progress
    } else {
        1.0 - (-2.0 * progress + 2.0).powi(2) / 2.0
    }
}

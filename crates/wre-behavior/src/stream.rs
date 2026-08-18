use serde::{Deserialize, Serialize};

use wre_core::error::{Error, Result};
use wre_crypto::prng::{Rng, SplitMix64};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn distance_to(self, other: Point) -> f64 {
        ((other.x - self.x).powi(2) + (other.y - self.y).powi(2)).sqrt()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Event {
    PointerMove { at: f64, x: f64, y: f64 },
    PointerDown { at: f64, x: f64, y: f64, button: u8 },
    PointerUp { at: f64, x: f64, y: f64, button: u8 },
    TouchStart { at: f64, x: f64, y: f64 },
    TouchMove { at: f64, x: f64, y: f64 },
    TouchEnd { at: f64, x: f64, y: f64 },
    KeyDown { at: f64, key: String },
    KeyUp { at: f64, key: String },
    Scroll { at: f64, x: f64, y: f64 },
}

impl Event {
    pub fn at(&self) -> f64 {
        match self {
            Event::PointerMove { at, .. }
            | Event::PointerDown { at, .. }
            | Event::PointerUp { at, .. }
            | Event::TouchStart { at, .. }
            | Event::TouchMove { at, .. }
            | Event::TouchEnd { at, .. }
            | Event::KeyDown { at, .. }
            | Event::KeyUp { at, .. }
            | Event::Scroll { at, .. } => *at,
        }
    }

    pub fn position(&self) -> Option<Point> {
        match self {
            Event::PointerMove { x, y, .. }
            | Event::PointerDown { x, y, .. }
            | Event::PointerUp { x, y, .. }
            | Event::TouchStart { x, y, .. }
            | Event::TouchMove { x, y, .. }
            | Event::TouchEnd { x, y, .. }
            | Event::Scroll { x, y, .. } => Some(Point::new(*x, *y)),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Event::PointerMove { .. } => "pointermove",
            Event::PointerDown { .. } => "pointerdown",
            Event::PointerUp { .. } => "pointerup",
            Event::TouchStart { .. } => "touchstart",
            Event::TouchMove { .. } => "touchmove",
            Event::TouchEnd { .. } => "touchend",
            Event::KeyDown { .. } => "keydown",
            Event::KeyUp { .. } => "keyup",
            Event::Scroll { .. } => "scroll",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    pub step_px: f64,
    pub sample_ms: f64,
    pub jitter_px: f64,
    pub overshoot: f64,
    pub dwell_ms: f64,
    pub flight_ms: f64,
    pub pause_ms: f64,
}

impl Default for Shape {
    fn default() -> Self {
        Self {
            step_px: 6.0,
            sample_ms: 8.0,
            jitter_px: 1.2,
            overshoot: 0.06,
            dwell_ms: 85.0,
            flight_ms: 120.0,
            pause_ms: 220.0,
        }
    }
}

pub struct Stream {
    rng: SplitMix64,
    shape: Shape,
    clock: f64,
    at: Point,
    events: Vec<Event>,
}

impl Stream {
    pub fn new(seed: u64, start: Point, shape: Shape) -> Self {
        Self {
            rng: SplitMix64::new(seed.wrapping_add(0x5DEE_CE66)),
            shape,
            clock: 0.0,
            at: start,
            events: Vec::new(),
        }
    }

    pub fn seeded(seed: u64) -> Self {
        Self::new(seed, Point::new(0.0, 0.0), Shape::default())
    }

    pub fn now(&self) -> f64 {
        self.clock
    }

    pub fn at(&self) -> Point {
        self.at
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub fn into_events(self) -> Vec<Event> {
        self.events
    }

    fn unit(&mut self) -> f64 {
        (self.rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn spread(&mut self, scale: f64) -> f64 {
        (self.unit() - 0.5) * 2.0 * scale
    }

    fn advance(&mut self, base: f64) -> f64 {
        let wobble = 1.0 + self.spread(0.35);
        let step = (base * wobble).max(1.0);
        self.clock = tidy(self.clock + step);
        self.clock
    }

    pub fn wait(&mut self, ms: f64) -> &mut Self {
        self.clock = tidy(self.clock + ms.max(0.0));
        self
    }

    pub fn pause(&mut self) -> &mut Self {
        let base = self.shape.pause_ms;
        self.advance(base);
        self
    }

    pub fn move_to(&mut self, target: Point) -> Result<&mut Self> {
        if !target.x.is_finite() || !target.y.is_finite() {
            return Err(Error::msg("a pointer target must be a finite point"));
        }

        let from = self.at;
        let distance = from.distance_to(target);

        if distance < 0.5 {
            self.at = target;
            return Ok(self);
        }

        let step = if self.shape.step_px > 0.5 { self.shape.step_px } else { 6.0 };
        let steps = ((distance / step).ceil() as usize).clamp(4, 220);
        let overshoot = self.shape.overshoot;

        let lift = Point::new(
            target.x + (target.x - from.x) * overshoot,
            target.y + (target.y - from.y) * overshoot,
        );

        for step in 1..=steps {
            let progress = step as f64 / steps as f64;
            let eased = minimum_jerk(progress);

            let aim = if progress > 0.82 { target } else { lift };
            let jitter = self.shape.jitter_px * (1.0 - progress);

            let x = tidy(from.x + (aim.x - from.x) * eased + self.spread(jitter));
            let y = tidy(from.y + (aim.y - from.y) * eased + self.spread(jitter));

            let at = self.advance(self.shape.sample_ms);
            self.events.push(Event::PointerMove { at, x, y });
            self.at = Point::new(x, y);
        }

        let at = self.advance(self.shape.sample_ms);
        self.events.push(Event::PointerMove { at, x: target.x, y: target.y });
        self.at = target;

        Ok(self)
    }

    pub fn click(&mut self) -> &mut Self {
        let position = self.at;
        let down = self.advance(self.shape.flight_ms);
        self.events.push(Event::PointerDown {
            at: down,
            x: position.x,
            y: position.y,
            button: 0,
        });

        let up = self.advance(self.shape.dwell_ms);
        self.events.push(Event::PointerUp {
            at: up,
            x: position.x,
            y: position.y,
            button: 0,
        });

        self
    }

    pub fn click_at(&mut self, target: Point) -> Result<&mut Self> {
        self.move_to(target)?;
        Ok(self.click())
    }

    pub fn type_text(&mut self, text: &str) -> &mut Self {
        for character in text.chars() {
            let down = self.advance(self.shape.flight_ms);
            self.events.push(Event::KeyDown { at: down, key: character.to_string() });

            let up = self.advance(self.shape.dwell_ms);
            self.events.push(Event::KeyUp { at: up, key: character.to_string() });
        }

        self
    }

    pub fn scroll_by(&mut self, dx: f64, dy: f64, steps: usize) -> &mut Self {
        let steps = steps.max(1);
        let mut moved = Point::new(0.0, 0.0);

        for step in 1..=steps {
            let progress = minimum_jerk(step as f64 / steps as f64);
            let next = Point::new(tidy(dx * progress), tidy(dy * progress));

            let at = self.advance(self.shape.sample_ms * 2.0);
            self.events.push(Event::Scroll { at, x: next.x, y: next.y });
            moved = next;
        }

        let _ = moved;
        self
    }

    pub fn tap(&mut self, target: Point) -> &mut Self {
        let start = self.advance(self.shape.flight_ms);
        self.events.push(Event::TouchStart { at: start, x: target.x, y: target.y });

        let drift = self.shape.jitter_px;
        let x = tidy(target.x + self.spread(drift));
        let y = tidy(target.y + self.spread(drift));

        let middle = self.advance(self.shape.sample_ms);
        self.events.push(Event::TouchMove { at: middle, x, y });

        let end = self.advance(self.shape.dwell_ms);
        self.events.push(Event::TouchEnd { at: end, x, y });

        self.at = target;
        self
    }
}

fn tidy(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn minimum_jerk(progress: f64) -> f64 {
    let clamped = progress.clamp(0.0, 1.0);
    10.0 * clamped.powi(3) - 15.0 * clamped.powi(4) + 6.0 * clamped.powi(5)
}

pub fn intervals(events: &[Event]) -> Vec<f64> {
    events
        .windows(2)
        .map(|pair| pair[1].at() - pair[0].at())
        .collect()
}

pub fn is_monotonic(events: &[Event]) -> bool {
    events.windows(2).all(|pair| pair[1].at() >= pair[0].at())
}

pub fn distinct_intervals(events: &[Event]) -> usize {
    let mut seen: Vec<u64> = intervals(events)
        .into_iter()
        .map(|gap| (gap * 1000.0).round() as u64)
        .collect();

    seen.sort_unstable();
    seen.dedup();
    seen.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stream_is_reproducible_from_its_seed() {
        let build = || {
            let mut stream = Stream::seeded(42);
            stream.click_at(Point::new(400.0, 300.0)).unwrap();
            stream.type_text("hello");
            stream.into_events()
        };

        assert_eq!(build(), build());
    }

    #[test]
    fn a_different_seed_gives_a_different_stream() {
        let build = |seed| {
            let mut stream = Stream::seeded(seed);
            stream.click_at(Point::new(400.0, 300.0)).unwrap();
            stream.into_events()
        };

        assert_ne!(build(1), build(2));
    }

    #[test]
    fn time_never_runs_backwards() {
        let mut stream = Stream::seeded(7);
        stream.click_at(Point::new(120.0, 80.0)).unwrap();
        stream.pause();
        stream.type_text("abc");
        stream.scroll_by(0.0, 400.0, 12);

        assert!(is_monotonic(stream.events()));
        assert!(stream.now() > 0.0);
    }

    #[test]
    fn a_pointer_path_ends_exactly_on_its_target() {
        let mut stream = Stream::seeded(3);
        let target = Point::new(640.0, 480.0);
        stream.move_to(target).unwrap();

        assert_eq!(stream.at(), target);
        assert_eq!(stream.events().last().unwrap().position(), Some(target));
    }

    #[test]
    fn a_pointer_path_is_sampled_rather_than_teleporting() {
        let mut stream = Stream::seeded(5);
        stream.move_to(Point::new(800.0, 600.0)).unwrap();

        let moves = stream
            .events()
            .iter()
            .filter(|event| matches!(event, Event::PointerMove { .. }))
            .count();

        assert!(moves > 20, "only {moves} samples for a long path");
    }

    #[test]
    fn the_path_does_not_run_in_a_straight_line() {
        let mut stream = Stream::seeded(11);
        let from = Point::new(0.0, 0.0);
        let to = Point::new(500.0, 500.0);
        stream.move_to(to).unwrap();

        let off_axis = stream
            .events()
            .iter()
            .filter_map(Event::position)
            .filter(|point| {
                let expected = from.y + (point.x - from.x) * (to.y - from.y) / (to.x - from.x);
                (point.y - expected).abs() > 0.5
            })
            .count();

        assert!(off_axis > 0, "every sample sat exactly on the straight line");
    }

    #[test]
    fn the_gaps_between_events_are_not_all_identical() {
        let mut stream = Stream::seeded(9);
        stream.move_to(Point::new(300.0, 200.0)).unwrap();

        assert!(
            distinct_intervals(stream.events()) > 10,
            "a constant sampling gap is the easiest tell there is"
        );
    }

    #[test]
    fn a_click_produces_a_down_then_an_up_with_dwell_between_them() {
        let mut stream = Stream::seeded(13);
        stream.click();

        let events = stream.events();
        assert!(matches!(events[0], Event::PointerDown { .. }));
        assert!(matches!(events[1], Event::PointerUp { .. }));
        assert!(events[1].at() > events[0].at());
    }

    #[test]
    fn typing_produces_a_key_pair_per_character() {
        let mut stream = Stream::seeded(17);
        stream.type_text("abc");

        assert_eq!(stream.events().len(), 6);
        assert_eq!(stream.events()[0].name(), "keydown");
        assert_eq!(stream.events()[1].name(), "keyup");

        let keys: Vec<String> = stream
            .events()
            .iter()
            .filter_map(|event| match event {
                Event::KeyDown { key, .. } => Some(key.clone()),
                _ => None,
            })
            .collect();

        assert_eq!(keys, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    #[test]
    fn a_tap_is_a_start_a_drift_and_an_end() {
        let mut stream = Stream::seeded(19);
        stream.tap(Point::new(200.0, 400.0));

        let names: Vec<&str> = stream.events().iter().map(Event::name).collect();
        assert_eq!(names, vec!["touchstart", "touchmove", "touchend"]);
    }

    #[test]
    fn a_move_to_where_we_already_are_emits_nothing() {
        let mut stream = Stream::new(23, Point::new(50.0, 50.0), Shape::default());
        stream.move_to(Point::new(50.0, 50.0)).unwrap();

        assert!(stream.events().is_empty());
    }

    #[test]
    fn a_target_that_is_not_a_number_is_rejected() {
        let mut stream = Stream::seeded(29);
        assert!(stream.move_to(Point::new(f64::NAN, 0.0)).is_err());
        assert!(stream.move_to(Point::new(0.0, f64::INFINITY)).is_err());
    }

    #[test]
    fn coordinates_and_timestamps_are_reported_at_a_sane_precision() {
        let mut stream = Stream::seeded(37);
        stream.click_at(Point::new(321.0, 654.0)).unwrap();

        for event in stream.events() {
            assert_eq!(event.at(), tidy(event.at()));
            if let Some(point) = event.position() {
                assert_eq!(point.x, tidy(point.x));
                assert_eq!(point.y, tidy(point.y));
            }
        }
    }

    #[test]
    fn events_round_trip_through_json() {
        let mut stream = Stream::seeded(31);
        stream.click_at(Point::new(10.0, 20.0)).unwrap();

        let events = stream.into_events();
        let text = serde_json::to_string(&events).unwrap();

        assert_eq!(serde_json::from_str::<Vec<Event>>(&text).unwrap(), events);
    }
}

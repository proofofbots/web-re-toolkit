use serde_json::{Value, json};

use crate::profile::Profile;

pub struct Machine {
    pub gpu: &'static str,
    pub cores: u64,
    pub memory: &'static [u64],
}

pub const MACS: &[Machine] = &[
    Machine { gpu: "Apple M1", cores: 8, memory: &[8, 16] },
    Machine { gpu: "Apple M1 Pro", cores: 8, memory: &[16, 32] },
    Machine { gpu: "Apple M1 Pro", cores: 10, memory: &[16, 32] },
    Machine { gpu: "Apple M1 Max", cores: 10, memory: &[32, 64] },
    Machine { gpu: "Apple M2", cores: 8, memory: &[8, 16, 24] },
    Machine { gpu: "Apple M2 Pro", cores: 10, memory: &[16, 32] },
    Machine { gpu: "Apple M2 Pro", cores: 12, memory: &[16, 32] },
    Machine { gpu: "Apple M2 Max", cores: 12, memory: &[32, 64, 96] },
    Machine { gpu: "Apple M3", cores: 8, memory: &[8, 16, 24] },
    Machine { gpu: "Apple M3 Pro", cores: 11, memory: &[18, 36] },
    Machine { gpu: "Apple M3 Pro", cores: 12, memory: &[18, 36] },
    Machine { gpu: "Apple M3 Max", cores: 14, memory: &[36, 96] },
    Machine { gpu: "Apple M3 Max", cores: 16, memory: &[48, 64, 128] },
    Machine { gpu: "Apple M4", cores: 10, memory: &[16, 24, 32] },
    Machine { gpu: "Apple M4 Pro", cores: 12, memory: &[24, 48] },
    Machine { gpu: "Apple M4 Pro", cores: 14, memory: &[24, 48, 64] },
];

const RENDERER: &str = "37446";

pub fn vary(profile: &mut Profile, seed: u64) -> String {
    if !describes_apple_silicon(profile) {
        return describe(profile);
    }

    let mut rng = Rng::new(seed);
    let machine = &MACS[rng.below(MACS.len())];
    let memory = machine.memory[rng.below(machine.memory.len())];

    let renderer = json!(format!(
        "ANGLE (Apple, ANGLE Metal Renderer: {}, Unspecified Version)",
        machine.gpu
    ));

    profile.webgl_parameters.insert(RENDERER.to_string(), renderer.clone());

    if !profile.webgl2_parameters.is_empty() {
        profile.webgl2_parameters.insert(RENDERER.to_string(), renderer);
    }

    for brand in ["Navigator", "WorkerNavigator"] {
        set(profile, brand, "hardwareConcurrency", json!(machine.cores));
        set(profile, brand, "deviceMemory", json!(memory));
    }

    describe(profile)
}

pub fn describe(profile: &Profile) -> String {
    let gpu = profile
        .webgl_parameters
        .get(RENDERER)
        .and_then(Value::as_str)
        .and_then(|text| text.rsplit_once(": ").map(|(_, rest)| rest))
        .and_then(|text| text.split_once(',').map(|(name, _)| name))
        .unwrap_or("unknown")
        .to_string();

    let read = |name: &str| {
        profile
            .property("Navigator", name)
            .and_then(Value::as_u64)
            .unwrap_or_default()
    };

    format!("{gpu}, {} cores, {} GB", read("hardwareConcurrency"), read("deviceMemory"))
}

fn describes_apple_silicon(profile: &Profile) -> bool {
    profile
        .webgl_parameters
        .get(RENDERER)
        .and_then(Value::as_str)
        .map(|text| text.contains("Apple M"))
        .unwrap_or(false)
}

fn set(profile: &mut Profile, brand: &str, name: &str, value: Value) {
    let Some(interface) = profile.interfaces.iter_mut().find(|entry| entry.brand == brand) else {
        return;
    };

    if !interface.properties.contains_key(name) {
        return;
    }

    interface.properties.insert(name.to_string(), value);
}

struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self { state: seed ^ 0x9e3779b97f4a7c15 }
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e3779b97f4a7c15);
        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
        value ^ (value >> 31)
    }

    fn below(&mut self, ceiling: usize) -> usize {
        if ceiling == 0 {
            return 0;
        }

        (self.next() % ceiling as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_lands_on_a_mac_that_exists() {
        for seed in 0..64u64 {
            let mut profile = Profile::desktop_chrome();
            let named = vary(&mut profile, seed);

            let (gpu, rest) = named.split_once(", ").expect("a label reads as gpu, cores, memory");
            let machine = MACS
                .iter()
                .find(|entry| entry.gpu == gpu && rest.starts_with(&format!("{} cores", entry.cores)))
                .unwrap_or_else(|| panic!("{named} is not a machine on the list"));

            let memory = profile
                .property("Navigator", "deviceMemory")
                .and_then(Value::as_u64)
                .unwrap();

            assert!(machine.memory.contains(&memory), "{named} has {memory} GB");
        }
    }

    #[test]
    fn the_canvas_and_the_metrics_are_left_alone() {
        let base = Profile::desktop_chrome();
        let mut moved = Profile::desktop_chrome();

        vary(&mut moved, 11);

        assert_eq!(base.canvas, moved.canvas);
        assert_eq!(base.font_widths, moved.font_widths);
        assert_eq!(base.audio, moved.audio);
    }

    #[test]
    fn two_seeds_do_not_agree() {
        let mut first = Profile::desktop_chrome();
        let mut second = Profile::desktop_chrome();

        let mut seen = std::collections::BTreeSet::new();
        for seed in 0..24u64 {
            seen.insert(vary(&mut first, seed));
        }

        assert!(seen.len() > 8, "only {} machines in 24 seeds", seen.len());
        assert_ne!(vary(&mut first, 1), vary(&mut second, 2));
    }

    #[test]
    fn a_profile_that_is_not_a_mac_is_left_where_it_is() {
        let mut profile = Profile::desktop_chrome();
        profile
            .webgl_parameters
            .insert(RENDERER.to_string(), json!("ANGLE (NVIDIA, NVIDIA GeForce RTX 4070)"));

        let before = profile.clone();
        vary(&mut profile, 5);

        assert_eq!(before, profile);
    }
}

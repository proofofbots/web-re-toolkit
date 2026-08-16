use std::fmt::Write as _;
use std::path::PathBuf;

fn main() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/graph/profiles");
    println!("cargo:rerun-if-changed={}", dir.display());

    let mut found: Vec<(String, PathBuf)> = Vec::new();

    if let Ok(listing) = std::fs::read_dir(&dir) {
        for entry in listing.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();

            let Some(id) = name.strip_suffix(".json.gz") else {
                continue;
            };

            println!("cargo:rerun-if-changed={}", path.display());
            found.push((id.to_string(), path));
        }
    }

    found.sort_by(|left, right| left.0.cmp(&right.0));

    let mut table = String::from("pub static BUNDLED_GRAPHS: &[(&str, &[u8])] = &[\n");
    for (id, path) in &found {
        let _ = writeln!(table, "    ({id:?}, include_bytes!({:?})),", path.display().to_string());
    }
    table.push_str("];\n");

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("bundled_graphs.rs");
    std::fs::write(&out, table).expect("write bundled_graphs.rs");
}

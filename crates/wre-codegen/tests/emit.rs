use std::path::PathBuf;

use serde_json::json;

use wre_client::shape::{Shape, field};
use wre_client::spec::{BundleDescriptor, ClientDescriptor, EventSpec, OpSpec, PROTOCOL_VERSION};
use wre_codegen::binaries::Binaries;
use wre_codegen::{Language, PackageConfig, Plan, emit_all};

fn bundle() -> BundleDescriptor {
    let client = ClientDescriptor::new("demo", "0.2.0")
        .summary("A worked target")
        .config(Shape::object(
            "DemoConfig",
            [
                field("key", Shape::Str).with_default(json!("k")),
                field("proxy", Shape::optional(Shape::Str)),
            ],
        ))
        .op(
            OpSpec::new(
                "solve",
                Shape::object(
                    "Facts",
                    [
                        field("url", Shape::Str),
                        field("mode", Shape::enumeration("Mode", &["fast", "slow"]))
                            .with_default(json!("fast")),
                        field("headers", Shape::optional(Shape::map(Shape::Str))),
                    ],
                ),
                Shape::object(
                    "Solved",
                    [field("body", Shape::Str), field("took_ms", Shape::Int)],
                ),
            )
            .summary("Solve one challenge")
            .streams(&["progress"]),
        )
        .op(OpSpec::new("roles", Shape::object("RolesInput", []), Shape::list(Shape::Str)))
        .event(EventSpec::new(
            "progress",
            Shape::object("Progress", [field("done", Shape::Int)]),
        ))
        .seal()
        .expect("descriptor seals");

    BundleDescriptor {
        protocol: PROTOCOL_VERSION,
        bundle: "test".to_string(),
        toolkit_version: "0.1.0".to_string(),
        binary_version: "0.1.0".to_string(),
        clients: vec![client],
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wre-codegen-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn read(path: &PathBuf) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[test]
fn every_language_gets_a_package_with_typed_ops() {
    let descriptor = bundle();
    let client = &descriptor.clients[0];
    let config = PackageConfig::default();
    let binaries = Binaries::default();
    let out = scratch("all");

    let plan = Plan {
        bundle: &descriptor,
        client,
        config: &config,
        binaries: &binaries,
        out: out.clone(),
    };

    let emitted = emit_all(&Language::ALL, &plan).expect("packages are written");
    assert_eq!(emitted.len(), 4);

    let node = read(&out.join("node/demo/index.d.ts"));
    assert!(node.contains("export interface Facts {"));
    assert!(node.contains("url: string;"));
    assert!(node.contains("mode?: Mode;"));
    assert!(node.contains("export type Mode = \"fast\" | \"slow\";"));
    assert!(node.contains("solve(params: Facts, options?: CallOptions): Promise<Solved>;"));
    assert!(node.contains("roles(options?: CallOptions): Promise<Array<string>>;"));

    let index = read(&out.join("node/demo/index.js"));
    assert!(index.contains("export const SCHEMA_HASH ="));
    assert!(index.contains("async solve(params, options = {})"));
    assert!(index.contains("async diagnose(write = true)"));

    let types = read(&out.join("python/demo/wre_client_demo/types.py"));
    assert!(types.contains("class FactsRequired(TypedDict):"));
    assert!(types.contains("class Facts(FactsRequired, total=False):"));
    assert!(types.contains("Mode = Literal[\"fast\", \"slow\"]"));

    let init = read(&out.join("python/demo/wre_client_demo/__init__.py"));
    assert!(init.contains("def solve(self, params: \"Facts\""));
    assert!(init.contains("class DemoClient:"));

    let go_types = read(&out.join("go/demo/types.go"));
    assert!(go_types.contains("type Facts struct {"));
    assert!(go_types.contains("URL string `json:\"url\"`"));
    assert!(go_types.contains("Mode *Mode `json:\"mode,omitempty\"`"));
    assert!(go_types.contains("ModeFast Mode = \"fast\""));

    let go_client = read(&out.join("go/demo/client.go"));
    assert!(go_client.contains("func (c *Client) Solve(ctx context.Context, params Facts) (Solved, error)"));
    assert!(go_client.contains("func (c *Client) Roles(ctx context.Context) ([]string, error)"));

    let rust = read(&out.join("rust/demo/src/lib.rs"));
    assert!(rust.contains("pub struct Facts {"));
    assert!(rust.contains("pub url: String,"));
    assert!(rust.contains("pub mode: Option<Mode>,"));
    assert!(rust.contains("pub fn solve(&self, params: &Facts) -> ClientResult<Solved>"));
    assert!(rust.contains("pub fn roles(&self) -> ClientResult<Vec<String>>"));

    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn a_bundle_without_binaries_still_generates_packages_that_need_wre_binary() {
    let descriptor = bundle();
    let client = &descriptor.clients[0];
    let config = PackageConfig::default();
    let binaries = Binaries::default();
    let out = scratch("nobins");

    let plan = Plan {
        bundle: &descriptor,
        client,
        config: &config,
        binaries: &binaries,
        out: out.clone(),
    };

    emit_all(&[Language::Node], &plan).expect("node package is written");

    let package = read(&out.join("node/demo/package.json"));
    assert!(!package.contains("optionalDependencies"));

    let index = read(&out.join("node/demo/index.js"));
    assert!(index.contains("const PLATFORMS = {\n};"));

    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn language_names_parse_from_a_comma_list() {
    let parsed = Language::parse_list(&["node,go".to_string()]).unwrap();
    assert_eq!(parsed, vec![Language::Node, Language::Go]);

    let all = Language::parse_list(&["all".to_string()]).unwrap();
    assert_eq!(all.len(), 4);

    let empty = Language::parse_list(&[]).unwrap();
    assert_eq!(empty.len(), 4);

    assert!(Language::parse_list(&["perl".to_string()]).is_err());
}

#[test]
fn a_bundle_with_two_targets_writes_one_package_each() {
    let mut descriptor = bundle();
    let second = ClientDescriptor::new("other", "0.1.0")
        .config(Shape::object("OtherConfig", []))
        .op(OpSpec::new("ping", Shape::object("PingInput", []), Shape::Str))
        .seal()
        .unwrap();
    descriptor.clients.push(second);

    let config = PackageConfig::default();
    let binaries = Binaries::default();
    let out = scratch("two");

    for client in &descriptor.clients {
        let plan = Plan {
            bundle: &descriptor,
            client,
            config: &config,
            binaries: &binaries,
            out: out.clone(),
        };
        emit_all(&[Language::Node, Language::Rust], &plan).expect("packages are written");
    }

    let demo = read(&out.join("node/demo/index.js"));
    let other = read(&out.join("node/other/index.js"));

    assert!(demo.contains("export const TARGET = \"demo\""));
    assert!(other.contains("export const TARGET = \"other\""));
    assert_eq!(descriptor.schema_hash().len(), 16);

    let hash = descriptor.schema_hash();
    assert!(demo.contains(&hash));
    assert!(other.contains(&hash));

    let _ = std::fs::remove_dir_all(&out);
}

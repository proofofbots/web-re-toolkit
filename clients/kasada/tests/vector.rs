use serde_json::{Value, json};

use wre_client::client::{Client, prepare, prepare_params};
use wre_client::context::{Call, Ctx, Services};
use wre_client::spec::ClientDescriptor;

const PAGE: &str = "https://www.realestate.com.au/buy";

struct Driver {
    client: Box<dyn Client>,
    descriptor: ClientDescriptor,
}

impl Driver {
    fn new(config: Value) -> Self {
        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .find(|dir| dir.join("wre.toml").is_file() || dir.join(".git").is_dir())
            .map(std::path::Path::to_path_buf);

        let services = Services::new(
            workspace,
            std::env::temp_dir().join("wre-kasada-vector"),
            std::sync::Arc::new(wre_client::context::Counters::default()),
        )
        .expect("services");

        let ctx = Ctx::new("kasada", "vector", services);
        let descriptor = wre_client_kasada::describe().seal().expect("descriptor");
        let config = prepare(&descriptor, &descriptor.config, config, "config").expect("config");
        let client = (wre_client_kasada::registration().build)(ctx, config).expect("client");

        Self { client, descriptor }
    }

    fn call(&mut self, op: &str, params: Value) -> Value {
        let params = prepare_params(&self.descriptor, op, params).expect("params");
        let call = Call::detached(op);

        self.client
            .call(op, params, &call)
            .unwrap_or_else(|error| panic!("{op} failed: {error}"))
    }
}

#[test]
#[ignore = "reaches www.realestate.com.au and writes the vector and its build to disk"]
fn the_vector_and_the_build_that_produced_it_are_written_out() {
    let mut driver = Driver::new(json!({ "page_url": PAGE, "capture_vector": true }));

    let _ = driver.call("solve", json!({}));
    let vector = driver.call("vector", json!({}));

    let out =
        std::env::var("WRE_VECTOR_OUT").unwrap_or_else(|_| "/tmp/kasada-vector.json".to_string());

    std::fs::write(
        &out,
        serde_json::to_string(&vector["vector"]).unwrap_or_default(),
    )
    .expect("write");

    let agent = format!("{out}.ips.js");
    std::fs::write(&agent, vector["agent"].as_str().unwrap_or_default()).expect("write");

    println!("slots {} out {out} agent {agent}", vector["slots"]);
}

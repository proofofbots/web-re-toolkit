use wre_client::client::Registry;

pub const BUNDLE: &str = match option_env!("WRE_BUNDLE") {
    Some(name) => name,
    None => "default",
};

pub fn build() -> Result<Registry, String> {
    let mut registry = Registry::new();

    #[cfg(feature = "target-example")]
    registry.register(wre_client_example::registration())?;

    Ok(registry)
}

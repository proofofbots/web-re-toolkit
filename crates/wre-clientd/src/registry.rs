use wre_client::client::Registry;

pub const BUNDLE: &str = match option_env!("WRE_BUNDLE") {
    Some(name) => name,
    None => "default",
};

pub fn build() -> Result<Registry, String> {
    let mut registry = Registry::new();

    #[cfg(feature = "target-example")]
    registry.register(wre_client_example::registration())?;

    #[cfg(feature = "target-altcha")]
    registry.register(wre_client_altcha::registration())?;

    #[cfg(feature = "target-akamai")]
    registry.register(wre_client_akamai::registration())?;

    #[cfg(feature = "target-kasada")]
    registry.register(wre_client_kasada::registration())?;

    Ok(registry)
}

#[cfg(feature = "ui-schema")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use lilyco_core::web_ui::UiContract;
    println!(
        "{}",
        serde_json::to_string_pretty(&schemars::schema_for!(UiContract))?
    );
    Ok(())
}

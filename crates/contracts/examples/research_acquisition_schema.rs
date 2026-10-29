//! Emit the current local research evidence schema; retained baselines are never rewritten.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    print!("{}", edgeagent_contracts::research_acquisition::schema()?);
    Ok(())
}

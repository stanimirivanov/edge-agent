//! Emit the immutable companion schema without changing retained Research v1 schemas.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    print!("{}", edgeagent_contracts::research_decision::schema()?);
    Ok(())
}

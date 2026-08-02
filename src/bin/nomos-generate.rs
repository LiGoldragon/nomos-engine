use std::env;
use std::fs;
use std::path::PathBuf;

use nomos_engine::batch::{
    BatchConfiguration, BatchOutcomeReporting, OfflineBatchConfiguration, OfflineBatchGeneration,
};

type AnyError = Box<dyn std::error::Error + Send + Sync>;

fn main() -> Result<(), AnyError> {
    let mut arguments = env::args_os().skip(1);
    let configuration_path = PathBuf::from(arguments.next().ok_or(
        "usage: nomos-generate <configuration.json> <source.ethos> <output.rs> <outcome.txt>",
    )?);
    let source_path = PathBuf::from(arguments.next().ok_or("missing Ethos source path")?);
    let output_path = PathBuf::from(arguments.next().ok_or("missing Rust output path")?);
    let outcome_path = PathBuf::from(arguments.next().ok_or("missing outcome report path")?);
    if arguments.next().is_some() {
        return Err("too many nomos-generate arguments".into());
    }

    let configuration = fs::read_to_string(configuration_path)?;
    let source = fs::read_to_string(source_path)?;
    let generator = BatchConfiguration::from_json(&configuration)?.prepare()?;
    let outcome = generator.generate(&source)?;
    fs::write(output_path, outcome.rust())?;
    fs::write(outcome_path, outcome.report())?;
    Ok(())
}

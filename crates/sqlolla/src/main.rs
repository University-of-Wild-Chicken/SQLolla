//! Offline newline-delimited JSON interface; no database execution or network service.
use sqlolla::pipeline::Pipeline;
use std::io::{self, BufRead, Write};

/// Process bounded input records and return one response per record.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut pipeline = Pipeline::new()?;
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    loop {
        let mut bytes = Vec::new();
        let size = std::io::Read::take(&mut input, 2_097_153).read_until(b'\n', &mut bytes)?;
        if size == 0 {
            break;
        }
        if size > 2_097_152 {
            return Err("request exceeds two MiB".into());
        }
        let response = pipeline.handle(&bytes);
        serde_json::to_writer(&mut output, &response)?;
        writeln!(output)?;
        output.flush()?;
    }
    Ok(())
}

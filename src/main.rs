use std::io;

use anyhow::{Result, bail};
use palc::Parser;

use forkseer::cli::Cli;
use forkseer::output;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let json = cli.json;

    let report = forkseer::run(cli).await?;

    let stdout = io::stdout();
    let stderr = io::stderr();
    output::render(json, &report, &mut stdout.lock(), &mut stderr.lock())?;

    if report.is_success() {
        Ok(())
    } else {
        bail!(
            "completed with {} repository scan failures",
            report.failures.len()
        )
    }
}

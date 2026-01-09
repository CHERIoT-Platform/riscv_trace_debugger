use anyhow::{anyhow, Context as _, Result};

pub fn init_logging() -> Result<()> {
    // Set up logging. RTD_LOG controls the log level. RTD_LOG_STYLE
    // controls colour output.
    let env = env_logger::Env::new()
        .filter("RTD_LOG")
        .write_style("RTD_LOG_STYLE");

    let mut builder = env_logger::Builder::from_env(env);

    // Optionally output to a file instead of stdout. Our process ID is apprended.
    if let Ok(log_file) = std::env::var("RTD_LOG_FILE") {
        let filename = format!("{log_file}.{}", std::process::id());
        let target = Box::new(
            std::fs::File::create(&filename)
                .with_context(|| anyhow!("opening output log file '{filename}'"))?,
        );
        builder.target(env_logger::Target::Pipe(target));
    }
    builder.filter_level(log::LevelFilter::Info);
    builder.init();
    Ok(())
}

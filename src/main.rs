use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    cloister::cli::run().await
}

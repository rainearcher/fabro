use async_trait::async_trait;

use super::{SpriteOutput, SpriteRunner};

/// Real implementation that invokes the `sprite` CLI binary.
#[derive(Default)]
pub struct CliSpriteRunner;

impl CliSpriteRunner {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SpriteRunner for CliSpriteRunner {
    async fn run(&self, args: &[&str]) -> Result<SpriteOutput, String> {
        let output = tokio::process::Command::new("sprite")
            .args(args)
            .output()
            .await
            .map_err(|e| format!("Failed to run sprite command: {e}"))?;

        Ok(SpriteOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
        })
    }

    async fn run_with_timeout(
        &self,
        args: &[&str],
        timeout: std::time::Duration,
    ) -> Result<SpriteOutput, String> {
        match tokio::time::timeout(timeout, self.run(args)).await {
            Ok(result) => result,
            Err(_) => Err("Command timed out".to_string()),
        }
    }
}

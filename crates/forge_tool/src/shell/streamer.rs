use std::io::{self, Write};

use tokio::io::AsyncReadExt;
use tokio::process::Child;

/// An output stream handler that captures and processes command output
pub struct OutputStream {
    buffer: Vec<u8>,
    writer: Box<dyn Write + Send>,
}

impl OutputStream {
    fn new(writer: Box<dyn Write + Send>) -> Self {
        Self { buffer: Vec::new(), writer }
    }

    /// Process stream data from a reader
    async fn process<T: tokio::io::AsyncRead + Unpin>(
        &mut self,
        mut reader: T,
    ) -> Result<(), String> {
        let mut buf = [0; 1024];

        while let Ok(n) = reader.read(&mut buf).await {
            if n == 0 {
                break;
            }
            self.writer
                .write_all(&buf[..n])
                .map_err(|e| format!("Failed to write to stream: {}", e))?;
            self.buffer.extend_from_slice(&buf[..n]);
        }
        Ok(())
    }

    /// Convert the captured output to a string, removing ANSI escape codes
    fn into_output(self) -> Result<String, String> {
        String::from_utf8(strip_ansi_escapes::strip(self.buffer))
            .map_err(|e| format!("Failed to convert output to string: {}", e))
    }
}

/// A stream processor that handles command output streams
pub struct CommandStreamer {
    child: Child,
}

impl CommandStreamer {
    pub fn new(child: Child) -> Self {
        Self { child }
    }

    /// Stream and process command output
    pub async fn stream(mut self) -> Result<(String, String, bool), String> {
        // Get stream handles
        let stdout = self
            .child
            .stdout
            .take()
            .ok_or_else(|| "Child process stdout not configured".to_string())?;
        let stderr = self
            .child
            .stderr
            .take()
            .ok_or_else(|| "Child process stderr not configured".to_string())?;

        // Initialize output streams without taking ownership yet
        let mut stdout_streamer = OutputStream::new(Box::new(io::stdout()));
        let mut stderr_streamer = OutputStream::new(Box::new(io::stderr()));

        // Process streams concurrently
        tokio::try_join!(
            stdout_streamer.process(stdout),
            stderr_streamer.process(stderr)
        )?;

        // Wait for command completion
        let status = self
            .child
            .wait()
            .await
            .map_err(|e| format!("Failed to wait for command: {}", e))?;

        Ok((
            stdout_streamer.into_output()?,
            stderr_streamer.into_output()?,
            status.success(),
        ))
    }
}

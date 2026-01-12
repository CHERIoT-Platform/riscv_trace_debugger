use std::io;

use gdbstub::conn::Connection;

/// This implements gdbstub's `Connection` trait which requires blocking writes.
/// It puts these writes immediately into a buffer and then later you can
/// actually send them with `flush()` which is async.
#[derive(Default)]
pub struct BufferedConnection {
    buffer: Vec<u8>,
    flush_pending: bool,
}

impl BufferedConnection {
    /// Actually send the buffered data, if any.
    pub async fn flush<W: tokio::io::AsyncWriteExt + Unpin>(
        &mut self,
        writer: &mut W,
    ) -> io::Result<()> {
        if self.flush_pending {
            writer.write_all(&self.buffer).await?;
            self.buffer.clear();
            self.flush_pending = false;
        }
        Ok(())
    }
}

impl Connection for BufferedConnection {
    type Error = std::convert::Infallible;

    fn write(&mut self, byte: u8) -> Result<(), Self::Error> {
        self.buffer.push(byte);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        // Just set a flag that a flush is pending.
        self.flush_pending = true;
        Ok(())
    }
}

//! This example demonstrates the capability of the crate to find which write incorrectly handled
//! splitting and caused a bug.

use std::io;

/// Hypothetical type that can be encoded into byte stream.
struct Value(u16, u16);

impl Value {
    /// Encodes the value to the writer
    fn to_writer<W: io::Write>(&self, mut writer: W) -> io::Result<()> {
        writer.write_all(&self.0.to_le_bytes())?;
        writer.write(&self.1.to_le_bytes())?; // this line is wrong and will get reported in the backtrace
        Ok(())
    }
}

/// Tests the implementation of `Value::to_writer`.
///
/// This is main for simplicity but in real life you'd use `#[test]`
fn main() {
    io_check::test_write(&[1, 0, 42, 0], |writer| {
        Value(1, 42).to_writer(writer).unwrap();
    });
}

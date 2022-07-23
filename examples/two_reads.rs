//! This example demonstrates the capability of the crate to find which read incorrectly handled
//! splitting and caused a bug.

use std::io;

/// Hypothetical type that can be decoded from byte stream.
#[derive(Debug, Eq, PartialEq)]
struct Value(u16, u16);

impl Value {
    /// Decodes the value from a reader
    fn from_reader<R: io::Read>(mut reader: R) -> io::Result<Self> {
        let mut buf = [0u8; 2];
        reader.read_exact(&mut buf)?;
        let num1 = u16::from_le_bytes(buf);
        reader.read(&mut buf)?; // this line is wrong and will get reported in the backtrace
        let num2 = u16::from_le_bytes(buf);
        Ok(Value(num1, num2))
    }
}

/// Tests the implementation of `Value::from_reader`.
///
/// This is main for simplicity but in real life you'd use `#[test]`
fn main() {
    io_check::test_read(&[1, 0, 42, 0], |reader| {
        let value = Value::from_reader(reader).unwrap();
        assert_eq!(value, Value(1, 42));
    });
}


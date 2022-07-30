//! Contains items related to testing of `Write` usage.

use std::io::{self, Write};
use std::panic::{UnwindSafe, RefUnwindSafe};

use crate::backtrace_impl::{Backtrace, BacktraceStorageMut, DisplayBacktrace};

/// Writer that splits writes the to test `Writer` consumers.
///
/// This writer is created by [`test_write`] function and provided to closure to test consumers of
/// the [`Write`] trait. While, as opposed to the read version, the writer can check writes
/// on-the-fly and panic directly, there is no guarantee that it will always perform checking this
/// way.
///
/// [`test_write`]: super::test_write
pub struct TestWriter<'a> {
    expected: &'a [u8],
    // must be mut ref so that `test_write` can check length
    stats: &'a mut WriteStats,
}

/// Write stats used to diagnose issues.
#[derive(Default)]
struct WriteStats {
    pos: usize,
    last_call: Option<Backtrace>,
    last_unwritten: usize,
}

impl WriteStats {
    fn emit_unhandled_partial_write(&self) -> ! {
        // If there was no previous write it couldn't be unhandled
        assert_ne!(self.pos, 0, "internal consistency check failed, this is a bug in the check_io library, not your code");
        let backtrace = DisplayBacktrace::write(&self.last_call);
        panic!("the write call at position {} didn't handle partial write\n{}", self.pos - 1, backtrace);
    }

    /// Must be called before a backtrace is displayed for the first time
    fn resolve_backtrace(&mut self) {
        crate::backtrace_impl::resolve(&mut self.last_call);
    }
}

impl<'a> TestWriter<'a> {
    fn new(expected: &'a [u8], stats: &'a mut WriteStats) -> Self {
        TestWriter {
            expected,
            stats,
        }
    }

    fn offset_data_matches(&self, data: &[u8]) -> bool {
        // shorten the code
        let last_unwritten = self.stats.last_unwritten;
        last_unwritten + data.len() <= self.expected.len() &&
            self.expected[last_unwritten..(last_unwritten + data.len())] == *data
    }

    /// Checks that data to be written is expected
    fn check_write(&mut self, data: &[u8]) {
        assert!(data.len() <= self.expected.len(), "attempt to write more data than expected");
        assert_ne!(data.len(), 0, "attempt to write 0 bytes to the writer; probably unrelated to splitting");
        let expected = &self.expected[..data.len()];
        if data != expected {
            self.stats.resolve_backtrace();
            if self.offset_data_matches(data) {
                self.stats.emit_unhandled_partial_write();
            } else {
                let backtrace = DisplayBacktrace::write(&self.stats.last_call);
                panic!("attempt to write unexpected data at pos {}, probably unrelated to partial writes\nexpected: {:?}\nreceived: {:?}\n{}", self.stats.pos, &self.expected[..data.len()], data, backtrace);
            }
        }
    }
}

impl Write for TestWriter<'_> {
    #[cfg_attr(feature = "rust_1_46", track_caller)]
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.check_write(data);
        if data.len() == 1 {
            // Erase backtrace since this is correct usage
            self.stats.last_call = None;
        } else {
            BacktraceStorageMut::from_mut(&mut self.stats.last_call).capture();
        }
        self.stats.last_unwritten = data.len() - 1;
        self.stats.pos += 1;
        self.expected = &self.expected[1..];
        Ok(1)
    }

    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        self.check_write(data);
        self.stats.last_unwritten = 0;
        // Erase backtrace since this is correct usage
        self.stats.last_call = None;
        self.stats.pos += data.len();
        self.expected = &self.expected[data.len()..];
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) mod hack {
    use super::TestWriter;
    use std::panic::{UnwindSafe, RefUnwindSafe};

    /// Tests whether the closure correctly handles partial writes.
    ///
    /// This is the entry point of this crate for write testing.
    /// You provide `expected` - bytes that should be produced by your closure, and a closure that
    /// accepts a writer and writes to it.
    ///
    /// For best results make sure no other inputs affect the test - the function should be pure.
    /// It is currently called once but it may be called multiple times in the future.
    ///
    /// The function may report the culprit incorrectly - in that case the problem is *before*
    /// reported function call. There's a plan to improve this in the future.
    pub fn test_write<F>(expected: &[u8], f: F) where F: Fn(TestWriter<'_>) + UnwindSafe + RefUnwindSafe {
        super::test_write(expected, f);
    }
}

fn test_write<F>(expected: &[u8], f: F) where F: Fn(TestWriter<'_>) + UnwindSafe + RefUnwindSafe {
    let mut stats = WriteStats::default();
    f(TestWriter::new(expected, &mut stats));
    if stats.pos < expected.len() {
        stats.resolve_backtrace();
        if stats.last_unwritten == expected.len() - stats.pos {
            stats.emit_unhandled_partial_write();
        } else {
            let backtrace = DisplayBacktrace::write(&stats.last_call);
            panic!("too few bytes were written to the writer but it seems unrelated to partial writes\n{}", backtrace);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::hack::test_write;
    use std::io::Write;

    #[test]
    fn empty() {
        test_write(&[], |_| ());
    }

    #[test]
    #[should_panic = "attempt to write more data than expected"]
    fn empty_write_past_end() {
        test_write(&[], |mut writer| { writer.write(&[42]).unwrap(); });
    }

    #[test]
    #[should_panic = "attempt to write more data than expected"]
    fn empty_write_all_past_end() {
        test_write(&[], |mut writer| writer.write_all(&[42]).unwrap());
    }

    #[test]
    #[should_panic = "too few bytes were written to the writer but it seems unrelated to partial writes"]
    fn one_byte_not_enoug() {
        test_write(&[42], |_| ());
    }

    #[test]
    fn one_byte_write() {
        test_write(&[42], |mut writer| { writer.write(&[42]).unwrap(); });
    }

    #[test]
    fn one_byte_write_all() {
        test_write(&[42], |mut writer| writer.write_all(&[42]).unwrap());
    }

    #[test]
    #[should_panic = "attempt to write more data than expected"]
    fn one_byte_write_past_end() {
        test_write(&[42], |mut writer| { writer.write(&[42, 47]).unwrap(); });
    }

    #[test]
    #[should_panic = "attempt to write more data than expected"]
    fn one_byte_write_all_past_end() {
        test_write(&[42], |mut writer| writer.write_all(&[42, 47]).unwrap());
    }

    #[test]
    #[should_panic = "attempt to write more data than expected"]
    fn one_byte_write_past_end_two_writes() {
        test_write(&[42], |mut writer| {
            writer.write(&[42]).unwrap();
            writer.write(&[47]).unwrap();
        });
    }

    #[test]
    #[should_panic = "attempt to write more data than expected"]
    fn one_byte_write_past_end_two_write_all() {
        test_write(&[42], |mut writer| {
            writer.write_all(&[42]).unwrap();
            writer.write_all(&[47]).unwrap();
        });
    }

    #[test]
    fn two_byte_write_all() {
        test_write(&[42, 47], |mut writer| writer.write_all(&[42, 47]).unwrap());
    }

    #[test]
    fn two_bytes_two_writes() {
        test_write(&[42, 47], |mut writer| {
            writer.write(&[42]).unwrap();
            writer.write(&[47]).unwrap();
        });
    }

    #[test]
    fn two_bytes_two_write_all() {
        test_write(&[42, 47], |mut writer| {
            writer.write_all(&[42]).unwrap();
            writer.write_all(&[47]).unwrap();
        });
    }

    #[test]
    #[should_panic = "the write call at position 0 didn't handle partial write"]
    fn two_bytes_unhandled_partial() {
        test_write(&[42, 47], |mut writer| {
            writer.write(&[42, 47]).unwrap();
        });
    }

    #[test]
    #[should_panic = "the write call at position 0 didn't handle partial write"]
    fn three_bytes_unhandled_partial() {
        test_write(&[42, 47, 1], |mut writer| {
            writer.write(&[42, 47]).unwrap();
            writer.write_all(&[1]).unwrap();
        });
    }
}

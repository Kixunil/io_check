//! Contains items related to testing of `Read` usage.

use std::io::{self, Read};
use std::fmt;
use std::panic::{catch_unwind, resume_unwind, UnwindSafe, RefUnwindSafe};
use either::Either;

use crate::backtrace_impl::{Backtrace, BacktraceStorageMut, DisplayBacktrace};

/// Reader that splits input the to test `Read` consumers.
///
/// This reader is created by [`test_read`] function and provided to closure to test consumers of
/// the [`Read`] trait. The reader returns same data that was provided in the parameter to
/// `test_read` but may split it into multiple chunks, as needed to achieve testing goals.
///
/// Currently the readers splits the input at each byte and, if the closure panics, it splits the
/// input in two to find the position where the problem occurs.
///
/// [`test_read`]: super::test_read
pub struct TestReader<'a>(Either<BreakingReader<'a>, SearchingReader<'a>>);

impl<'a> TestReader<'a> {
    fn breaking(input: &'a [u8]) -> Self {
        TestReader(Either::Left(BreakingReader(input)))
    }

    fn searching(input: &'a [u8], pos: usize, backtrace: BacktraceStorageMut<'a>) -> Self {
        TestReader(Either::Right(SearchingReader::new(input, pos, backtrace)))
    }
}

impl Read for TestReader<'_> {
    #[cfg_attr(feature = "rust_1_46", track_caller)]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // `read_exact` doesn't have `#[track_caller]`, so we have to bypass it
        // I don't care to find which version of `either` supports `for_both!` and supporting all
        // versions is nice.
        match &mut self.0 {
            Either::Left(reader) => reader.read(buf),
            Either::Right(reader) => reader.read(buf),
        }
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        // `read_exact` is not overridden in `Either`, so we have to do it ourselves
        // I don't care to find which version of `either` supports `for_both!` and supporting all
        // versions is nice.
        match &mut self.0 {
            Either::Left(reader) => reader.read_exact(buf),
            Either::Right(reader) => reader.read_exact(buf),
        }
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        self.0.read_to_end(buf)
    }
}

struct BreakingReader<'a>(&'a [u8]);

impl io::Read for BreakingReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.len() > 1 && self.0.len() > 1 {
            buf[1] = !self.0[1];
        }
        // intentional panic when buf.len() == 0: buggy use of the reader
        self.0.read(&mut buf[..1])
    }

    // read_exact is correct usage, so skip the BS
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        self.0.read_exact(buf)
    }

    // read_to_end is correct usage, so skip the BS
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        self.0.read_to_end(buf)
    }
}

struct SearchingReader<'a> {
    left: &'a [u8],
    right: &'a [u8],
    backtrace: BacktraceStorageMut<'a>,
}

impl<'a> SearchingReader<'a> {
    fn new(input: &'a [u8], pos: usize, backtrace: BacktraceStorageMut<'a>) -> Self {
        let (left, right) = input.split_at(pos);
        SearchingReader {
            left,
            right,
            backtrace,
        }
    }
}

impl io::Read for SearchingReader<'_> {
    #[cfg_attr(feature = "rust_1_46", track_caller)]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.left.is_empty() {
            self.right.read(buf)
        } else if self.left.len() < buf.len() {
            // if there is a problem it's caused by function that called `read` at the moment it
            // split - now. We don't know if there actually is a problem for this specific split,
            // so we collect backtrace and decide later whether to keep it.
            self.backtrace.capture();
            buf[self.left.len()] = !self.right[0];
            self.left.read(&mut buf[..self.left.len()])
        } else {
            self.left.read(buf)
        }
    }
}

// we want proper doc at top-level of the crate
pub(crate) mod hack {
    use super::*;

    /// Tests whether the closure correctly handles split reads.
    ///
    /// This is the entry point of this crate for read testing.
    /// You provide `input` that will be returned from reader and a closure that accepts the reader and
    /// uses it to read the input. The closure should check if the decoded values equal to the expected
    /// values and *panic* if not. (e.g. using `assert_eq!()`)
    ///
    /// For best results make sure no other inputs affect the test - the function should be pure.
    /// It will be called once and if it panics it'll be called again multiple times with
    /// differently-behaving readers.
    pub fn test_read<F>(input: &[u8], f: F) where F: Fn(TestReader<'_>) + UnwindSafe + RefUnwindSafe {
        test_read_no_panic(input, f).unwrap_or_else(|error| error.panic())
    }
}

fn test_read_no_panic<F>(input: &[u8], f: F) -> Result<(), Error> where F: Fn(TestReader<'_>) + UnwindSafe + RefUnwindSafe {
    if input.len() < 2 {
        panic!("Testing slices shorter than 2 bytes doesn't make sense");
    }
    catch_unwind(|| f(TestReader::breaking(input)))
        .map_err(|unwind| {
            // skip split at zero and end since those are non-sensical
            let failure_info = (1..input.len()).find_map(|pos| {
                let mut backtrace = None;
                let backtrace_mut = BacktraceStorageMut::from_mut(&mut backtrace);
                catch_unwind(|| f(TestReader::searching(input, pos, backtrace_mut)))
                    .err()
                    .map(|unwind| {
                        crate::backtrace_impl::resolve(&mut backtrace);
                        FailureInfo { unwind, pos, backtrace, }
                    })
            });
            Error {
                unwind,
                failure_info,
            }
        })
}

type Unwind = Box<dyn std::any::Any + Send + 'static>;

struct FailureInfo {
    unwind: Unwind,
    pos: usize,
    backtrace: Option<Backtrace>,
}

impl fmt::Debug for FailureInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("FailureInfo")
            .field("unwind", &format_args!("message: {:?}", get_panic_message(&self.unwind)))
            .field("pos", &self.pos)
            .finish()
    }
}


/// Test failure information
struct Error {
    unwind: Unwind,
    failure_info: Option<FailureInfo>,
}

impl Error {
    /// Resumes panic with relevant error information added if possible
    fn panic(self) -> ! {
        let first_panic_message = get_panic_message(&self.unwind);
        match self.failure_info {
            Some(FailureInfo { unwind, pos, backtrace }) => {
                let backtrace = DisplayBacktrace::read(&backtrace);
                let second_panic_message = get_panic_message(&unwind);
                match (first_panic_message, second_panic_message) {
                    (Some(msg1), Some(msg2)) if msg1 == msg2 => panic!("test failed at position {}: {}\n{}", pos, msg1, backtrace),
                    (Some(msg1), Some(msg2)) => panic!("test failed with message \"{}\" but a different message was encountered when breaking at position {}: {}\n{}", msg1, pos, msg2, backtrace),
                    (Some(msg), None) => panic!("test failed with message \"{}\" but a different panic with unknown message was encountered at position {}\n{}", msg, pos, backtrace),
                    (None, Some(msg)) => panic!("test failed with unknown message but a different panic was encountered at position {}: {}\n{}", pos, msg, backtrace),
                    (None, None) => panic!("test failed at position {} with unknown messages\n{}", pos, backtrace),
                }
            },
            None => {
                match first_panic_message {
                    Some(msg) => panic!("test failed at unknown position: {}", msg),
                    None => resume_unwind(self.unwind),
                }
            },
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Error")
            .field("unwind", &format_args!("message: {:?}", get_panic_message(&self.unwind)))
            .field("failure_info", &self.failure_info)
            .finish()
    }
}


fn get_panic_message(unwind: &Unwind) -> Option<&str> {
    match unwind.as_ref().downcast_ref::<&'static str>() {
        Some(msg) => Some(*msg),
        None => match unwind.as_ref().downcast_ref::<String>() {
            Some(msg) => Some(msg.as_str()),
            // Copy what rustc does in the default panic handler
            None => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use super::test_read_no_panic;

    impl super::Error {
        fn panic_message1(&self) -> Option<&str> {
            super::get_panic_message(&self.unwind)
        }

        fn panic_message2(&self) -> Option<&str> {
            self.failure_info.as_ref().and_then(|info| super::get_panic_message(&info.unwind))
        }

        fn pos(&self) -> Option<usize> {
            self.failure_info.as_ref().map(|info| info.pos)
        }
    }

    #[test]
    fn basic() {
        let err = test_read_no_panic(&[1, 0], |mut reader| {
            let mut buf = [0u8; 2];
            reader.read(&mut buf).unwrap();
            let num = u16::from_le_bytes(buf);
            assert_eq!(num, 1);
        }).unwrap_err();

        assert_eq!(err.panic_message1(), Some("assertion failed: `(left == right)`\n  left: `65281`,\n right: `1`"));
        assert_eq!(err.panic_message2(), err.panic_message1());
        assert_eq!(err.pos().unwrap(), 1);
    }

    #[test]
    fn read_exact_followed_by_read() {
        let err = test_read_no_panic(&[1, 0, 1, 0], |mut reader| {
            let mut buf = [0u8; 2];
            reader.read_exact(&mut buf).unwrap();
            let num = u16::from_le_bytes(buf);
            assert_eq!(num, 1);

            let mut buf = [0u8; 2];
            reader.read(&mut buf).unwrap();
            let num = u16::from_le_bytes(buf);
            assert_eq!(num, 1);
        }).unwrap_err();

        assert_eq!(err.panic_message1(), Some("assertion failed: `(left == right)`\n  left: `65281`,\n right: `1`"));
        assert_eq!(err.panic_message2(), err.panic_message1());
        assert_eq!(err.pos().unwrap(), 3);
    }

    #[test]
    fn no_error() {
        test_read_no_panic(&[1, 0], |mut reader| {
            let mut buf = [0u8; 2];
            reader.read_exact(&mut buf).unwrap();
            let num = u16::from_le_bytes(buf);
            assert_eq!(num, 1);
        }).unwrap();

    }
}

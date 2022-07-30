enum Operation {
    Read,
    Write,
}

pub struct DisplayBacktrace<'a> {
    #[cfg_attr(not(feature = "backtrace"), allow(unused))]
    backtrace: &'a Option<Backtrace>,
    #[cfg_attr(not(feature = "backtrace"), allow(unused))]
    operation: Operation
}

impl<'a> DisplayBacktrace<'a> {
    pub fn read(backtrace: &'a Option<Backtrace>) -> Self {
        DisplayBacktrace {
            backtrace,
            operation: Operation::Read,
        }
    }

    pub fn write(backtrace: &'a Option<Backtrace>) -> Self {
        DisplayBacktrace {
            backtrace,
            operation: Operation::Write,
        }
    }
}

#[cfg(feature = "backtrace")]
mod imp {
    use std::fmt;
    use std::panic::AssertUnwindSafe;
    use super::Operation;

    pub use backtrace::Backtrace;

    // safe because the only modification we do is assigning which can not panic
    // we also don't read from it
    pub struct BacktraceStorageMut<'a>(AssertUnwindSafe<&'a mut Option<Backtrace>>);

    impl<'a> BacktraceStorageMut<'a> {
        pub fn from_mut(storage: &'a mut Option<Backtrace>) -> Self {
            BacktraceStorageMut(AssertUnwindSafe(storage))
        }

        #[inline(always)] // remove this useless frame from backtrace
        pub fn capture(&mut self) {
            *(self.0).0 = Some(Backtrace::new_unresolved());
        }
    }

    pub fn resolve(storage: &mut Option<Backtrace>) {
        storage.as_mut().map(|storage| storage.resolve());
    }

    impl<'a> fmt::Display for super::DisplayBacktrace<'a> {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {

            match &self.backtrace {
                Some(backtrace) => {
                    let mut culprit = None;
                    let mut symbols = backtrace.frames().iter().flat_map(|frame| frame.symbols());
                    let op_fn_name = match self.operation {
                        Operation::Read => "<io_check::read::TestReader as std::io::Read>::read::",
                        Operation::Write => "<io_check::write::TestWriter as std::io::Write>::write::",
                    };
                    while let Some(symbol) = symbols.next() {
                        let is_test_reader_read = symbol.name().map(|name| name.to_string().starts_with(op_fn_name));
                        if is_test_reader_read == Some(true) {
                            culprit = symbols.next();
                            break;
                        }
                    }

                    if let Some(culprit) = culprit {
                        if let Some(name) = culprit.name() {
                            writeln!(f, "*******\nMost likely culprit in {}", name)?;
                            if let Some(file) = culprit.filename() {
                                write!(f, "    at {}", file.display())?;
                                if let Some(line) = culprit.lineno() {
                                    write!(f, ":{}", line)?;
                                    if let Some(column) = culprit.colno() {
                                        write!(f, ":{}", column)?;
                                    }
                                }
                                writeln!(f)?;
                            }
                            writeln!(f, "*******")?;
                        }

                        if std::env::var("RUST_BACKTRACE").unwrap_or(String::new()) == "1" {
                            write!(f, "backtrace:\n\n{:?}", backtrace)
                        } else {
                            write!(f, "Set RUST_BACKTRACE=1 environment variable to see the full backtrace")
                        }
                    } else {
                        write!(f, "backtrace:\n\n{:?}", backtrace)
                    }
                },
                None => write!(f, "no backtrace found - the problem is most likely unrelated to flaky IO"),
            }
        }
    }

}

#[cfg(all(not(feature = "backtrace"), not(feature = "rust_1_46")))]
mod imp {
    use std::panic::AssertUnwindSafe;

    use std::fmt;

    pub enum Backtrace {}

    // Avoids changing variance based on features.
    // It's UnwindSafe because no operation leaves content in inconsistent state
    pub struct BacktraceStorageMut<'a>(AssertUnwindSafe<std::marker::PhantomData<&'a mut ()>>);

    impl<'a> BacktraceStorageMut<'a> {
        pub fn from_mut(_storage: &'a mut Option<Backtrace>) -> Self {
            BacktraceStorageMut(AssertUnwindSafe(Default::default()))
        }

        pub fn capture(&mut self) {
        }
    }

    pub fn resolve(_storage: &mut Option<Backtrace>) {
    }

    impl<'a> fmt::Display for super::DisplayBacktrace<'a> {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "backtrace unavailable - compile with `backtrace` or `rust_1_46` feature to get the location of incorrect IO handling")
        }
    }
}

#[cfg(all(not(feature = "backtrace"), feature = "rust_1_46"))]
mod imp {
    use std::panic::{AssertUnwindSafe, Location};

    use std::fmt;

    pub type Backtrace = &'static std::panic::Location<'static>;

    // Avoids changing variance based on features.
    // It's UnwindSafe because no operation leaves content in inconsistent state
    pub struct BacktraceStorageMut<'a>(AssertUnwindSafe<&'a mut Option<Backtrace>>);

    impl<'a> BacktraceStorageMut<'a> {
        pub fn from_mut(storage: &'a mut Option<Backtrace>) -> Self {
            BacktraceStorageMut(AssertUnwindSafe(storage))
        }

        #[track_caller]
        pub fn capture(&mut self) {
            *(self.0).0 = Some(Location::caller());
        }
    }

    pub fn resolve(_storage: &mut Option<Backtrace>) {
    }

    impl<'a> fmt::Display for super::DisplayBacktrace<'a> {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            match self.backtrace {
                Some(location) => {
                    writeln!(f, "*******\nMost likely culprit in {}:{}:{}\n*******", location.file(), location.line(), location.column())?;
                    write!(f, "compile with `backtrace` feature to get a full backtrace")
                },
                None => write!(f, "no error location found - the problem is most likely unrelated to flaky IO"),
            }
        }
    }
}

pub use imp::*;

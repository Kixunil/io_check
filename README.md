# IO check

A Rust crate for thorough checking of `std::io::Read` and (in the future) `std::io::Write` uses.

## About

The `read` and `write` methods of their respective traits are not guaranteed to use the whole provided buffer.
A correct application must handle these cases.
This is usually achieved using `read_exact` or `write_all` but it is possible that people forget about those
or have special reasons to avoid them but fail to write the code correctly.

This crate provides a tool for testing such implementations automatically and even finding the exact location of the call that wasn't properly handled!

## Usage

Currently this is provided for `Read` only but the plan is to extend it to `Write`.
The interface is very simple: there's just one function that accepts the bytes that should be returned from `Read` and a closure implementing the test.
The closure acepts a reader as an argument and should call your decoding function.
You should then compare the decoded value to the expected value and *panic* if they are not equal - just as in tests.

If your code has a bug caused by improper handling of splits this crate will find it and even find the exact incorrectly-handled call.
Finding the culprit requires `backtrace` feature which is on by default.

Keep in mind that if there are multiple such bugs the crate only finds one at a time - the one corresponding to the leftmost part of the input.
Once you fix it you can re-run the test and it will report the next bug.
Repeat this until you fix all of them.

Note that this crate should be normally used as a dev-dependency only.

## How it works

The closure is called first with a reader that returns data byte-by-byte.
If there is a bug that causes reliance on `read` reading whole buffer this will trigger it unless the code is very exotic.
However, people usually fill the buffer with zeros first and if the input also contains zeos the bug would not trigger.
To avoid this the unused part of the buffer is scrambled such that the data is guaranteed to be invalid.

If the bug triggers the panic is caught using `catch_unwind` and search is run to find the exact place where it occurs.
The closure gets called multiple times with another reader that splits the input in two.
The sizes of the two parts change by one on each call.
Once a call panics we learn the position in the input where the problem is.

To find the actual function call the reader captures a backtrace when the `read` call provides a buffer that overlaps the split position.
There is only a single `read` call that can trigger this per iteration.
If the closure panics the captured backtrace is used in error reporting.

## MSRV

1.41.1 without `backtrace` feature, 1.48 with `backtrace` feature.

## License

MITNFA

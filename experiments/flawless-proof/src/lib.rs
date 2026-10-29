//! Isolated Flawless M08 experiment; no production dependency on the engine.

#[cfg(all(test, all(target_os = "linux", target_arch = "x86_64")))]
mod tests;

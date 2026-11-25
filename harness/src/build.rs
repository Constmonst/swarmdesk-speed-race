//! Compiles one submitted source file to WebAssembly. The submission is a
//! plain module; this file owns the crate root around it, so a submission
//! needs no attributes and no string literals.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The pinned compiler. `rust-toolchain.toml` must name the same version.
pub const TOOLCHAIN: &str = "1.100.0";
/// Maximum compiled WebAssembly size before reading into checker memory (4 MiB).
pub const MAX_WASM_BYTES: usize = 4 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(180);

/// Why compilation failed, distinguishing timeout from compile/size failures and host faults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    Timeout,
    DoesNotCompile,
    WasmTooLarge,
    Infrastructure(String),
}

impl CompileError {
    pub fn reason_code(&self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::DoesNotCompile | Self::WasmTooLarge => "compile_error",
            Self::Infrastructure(_) => "fault",
        }
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout => f.write_str("the compiler ran out of time"),
            Self::DoesNotCompile => f.write_str("does not compile"),
            Self::WasmTooLarge => f.write_str("compiled wasm exceeds 4 MiB limit"),
            Self::Infrastructure(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for CompileError {}

const ROOT: &str = r#"#![no_std]
#[path = "submission.rs"]
mod submission;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

#[no_mangle]
pub unsafe extern "C" fn run(inp: *const u8, len: usize, out: *mut u8, cap: usize) -> usize {
    submission::solve(
        core::slice::from_raw_parts(inp, len),
        core::slice::from_raw_parts_mut(out, cap),
    )
}
"#;

/// Builds `source` inside `work/label` and returns the module's bytes.
pub fn compile(source: &Path, work: &Path, label: &str) -> Result<Vec<u8>, String> {
    compile_checked(source, work, label).map_err(|e| e.to_string())
}

/// Builds `source` inside `work/label`, returning a structured `CompileError` on failure.
pub fn compile_checked(source: &Path, work: &Path, label: &str) -> Result<Vec<u8>, CompileError> {
    let dir = work.join(label);
    std::fs::create_dir_all(&dir).map_err(|e| CompileError::Infrastructure(e.to_string()))?;
    std::fs::copy(source, dir.join("submission.rs"))
        .map_err(|e| CompileError::Infrastructure(e.to_string()))?;
    std::fs::write(dir.join("root.rs"), ROOT)
        .map_err(|e| CompileError::Infrastructure(e.to_string()))?;
    let log = std::fs::File::create(dir.join("rustc.log"))
        .map_err(|e| CompileError::Infrastructure(e.to_string()))?;
    let rustc = rustc_path().map_err(CompileError::Infrastructure)?;
    let mut cmd = rustc_command(&rustc);
    let mut child = cmd
        .current_dir(&dir)
        .env_clear()
        .envs(passed_env())
        .env("RUSTUP_TOOLCHAIN", TOOLCHAIN)
        .args(["--edition", "2021", "--target", "wasm32-unknown-unknown"])
        .args(["--crate-type", "cdylib", "--crate-name", "submission"])
        .args([
            "-C",
            "opt-level=3",
            "-C",
            "panic=abort",
            "-C",
            "codegen-units=1",
        ])
        .args([
            "-C",
            "strip=symbols",
            "-C",
            "target-feature=+simd128",
            "-A",
            "warnings",
        ])
        .args(["-o", "out.wasm", "root.rs"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log)
        .spawn()
        .map_err(|e| CompileError::Infrastructure(format!("the compiler did not start: {e}")))?;
    let started = Instant::now();
    loop {
        match child
            .try_wait()
            .map_err(|e| CompileError::Infrastructure(e.to_string()))?
        {
            Some(status) if status.success() => break,
            Some(_) => return Err(CompileError::DoesNotCompile),
            None if started.elapsed() > TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(CompileError::Timeout);
            }
            None => std::thread::sleep(Duration::from_millis(40)),
        }
    }
    read_wasm(&dir.join("out.wasm"))
}

/// On Linux, cap the compiler's virtual address space to 2 GiB (`ulimit -v 2097152`)
/// without external crates; non-Linux platforms invoke `rustc` directly.
fn rustc_command(rustc: &Path) -> Command {
    #[cfg(target_os = "linux")]
    {
        let mut cmd = Command::new("/bin/sh");
        cmd.args([
            "-c",
            "ulimit -v 2097152 2>/dev/null || true; exec \"$@\"",
            "--",
        ])
        .arg(rustc);
        cmd
    }
    #[cfg(not(target_os = "linux"))]
    {
        Command::new(rustc)
    }
}

fn read_wasm(path: &Path) -> Result<Vec<u8>, CompileError> {
    let meta = std::fs::metadata(path).map_err(|e| CompileError::Infrastructure(e.to_string()))?;
    if meta.len() > MAX_WASM_BYTES as u64 {
        return Err(CompileError::WasmTooLarge);
    }
    let bytes = std::fs::read(path).map_err(|e| CompileError::Infrastructure(e.to_string()))?;
    if bytes.len() > MAX_WASM_BYTES {
        return Err(CompileError::WasmTooLarge);
    }
    Ok(bytes)
}

/// The compiler sees only these variables, so no credential of the caller reaches it.
fn passed_env() -> Vec<(String, String)> {
    ["PATH", "HOME", "RUSTUP_HOME", "CARGO_HOME", "TMPDIR"]
        .iter()
        .filter_map(|name| {
            std::env::var(name)
                .ok()
                .map(|value| (name.to_string(), value))
        })
        .collect()
}

/// Where the compiler's messages for `label` were written.
pub fn log_path(work: &Path, label: &str) -> PathBuf {
    work.join(label).join("rustc.log")
}

/// `SD_RUSTC` wins; otherwise ask rustup for the pinned compiler; otherwise the `rustc` on
/// the path, but only when it is the pinned version: another version counts other fuel.
fn rustc_path() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("SD_RUSTC") {
        return Ok(PathBuf::from(path));
    }
    let asked = Command::new("rustup")
        .args(["which", "rustc", "--toolchain", TOOLCHAIN])
        .output();
    if let Ok(out) = asked {
        if out.status.success() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return Ok(PathBuf::from(path));
        }
    }
    let version = Command::new("rustc")
        .env("RUSTUP_TOOLCHAIN", TOOLCHAIN)
        .arg("--version")
        .output();
    match version {
        Ok(out) if is_pinned(&String::from_utf8_lossy(&out.stdout)) => Ok(PathBuf::from("rustc")),
        _ => Err(format!("the compiler rustc {TOOLCHAIN} was not found")),
    }
}

/// True for the first line `rustc --version` prints for the pinned compiler.
fn is_pinned(version: &str) -> bool {
    version.starts_with(&format!("rustc {TOOLCHAIN} "))
}

#[cfg(test)]
mod tests {
    #[test]
    fn toolchain_file_names_the_same_version() {
        let file = include_str!("../../rust-toolchain.toml");
        assert!(file.contains(&format!("channel = \"{}\"", super::TOOLCHAIN)));
    }

    #[test]
    fn only_the_pinned_version_is_taken_from_the_path() {
        let pinned = format!("rustc {} (abc 2026-01-01)\n", super::TOOLCHAIN);
        assert!(super::is_pinned(&pinned));
        assert!(!super::is_pinned("rustc 1.99.0 (abc 2026-01-01)\n"));
        assert!(!super::is_pinned(&format!(
            "rustc {}1 (abc)\n",
            super::TOOLCHAIN
        )));
    }

    #[test]
    fn compile_error_distinguishes_timeout_from_failure() {
        use super::CompileError;
        assert_eq!(CompileError::Timeout.reason_code(), "timeout");
        assert_eq!(
            CompileError::Timeout.to_string(),
            "the compiler ran out of time"
        );
        assert_eq!(CompileError::DoesNotCompile.reason_code(), "compile_error");
        assert_eq!(CompileError::DoesNotCompile.to_string(), "does not compile");
        assert_eq!(CompileError::WasmTooLarge.reason_code(), "compile_error");
        assert_eq!(
            CompileError::WasmTooLarge.to_string(),
            "compiled wasm exceeds 4 MiB limit"
        );
        assert_eq!(
            CompileError::Infrastructure("boom".into()).reason_code(),
            "fault"
        );
    }

    #[test]
    fn read_wasm_rejects_modules_exceeding_max_wasm_bytes() {
        let dir = crate::work_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let big = dir.join("big.wasm");
        let small = dir.join("small.wasm");
        std::fs::write(&small, [0u8; 16]).unwrap();
        assert_eq!(super::read_wasm(&small).unwrap().len(), 16);

        let f = std::fs::File::create(&big).unwrap();
        f.set_len((super::MAX_WASM_BYTES as u64) + 1).unwrap();
        assert_eq!(super::read_wasm(&big), Err(super::CompileError::WasmTooLarge));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

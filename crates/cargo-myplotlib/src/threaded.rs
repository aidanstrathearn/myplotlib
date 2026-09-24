use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::Path;
use std::process::Command;

use wasmparser::{MemoryType, Parser, Payload, TypeRef};

use crate::Error;

pub(crate) const TOOLCHAIN: &str = "nightly-2025-11-15";
pub(crate) const DEFAULT_MAX_MEMORY: u64 = 1_073_741_824;
const WASM_PAGE_SIZE: u64 = 65_536;
const MAX_WASM32_MEMORY: u64 = WASM_PAGE_SIZE * 65_536;
const BUILD_STD: &str = "build-std=panic_abort,std";
const REQUIRED_COMPONENTS: [&str; 2] = ["rust-src", "rust-std-wasm32-unknown-unknown"];
const REQUIRED_EXPORTS: [&str; 2] = ["initThreadPool", "__tls_base"];

pub(crate) fn validate_max_memory(bytes: u64) -> Result<(), Error> {
    if bytes == 0 || bytes > MAX_WASM32_MEMORY || !bytes.is_multiple_of(WASM_PAGE_SIZE) {
        return Err(Error::new(format!(
            "--wasm-max-memory must be a non-zero multiple of {WASM_PAGE_SIZE} bytes no greater than {MAX_WASM32_MEMORY}"
        )));
    }
    Ok(())
}

pub(crate) fn cargo_arguments(max_memory: u64) -> Vec<OsString> {
    vec![
        OsString::from("--config"),
        OsString::from(rustflags_config(max_memory)),
        OsString::from("-Z"),
        OsString::from(BUILD_STD),
    ]
}

fn rustflags_config(max_memory: u64) -> String {
    format!(
        "target.wasm32-unknown-unknown.rustflags=[\
         \"-C\",\"target-feature=+atomics,+bulk-memory\",\
         \"-C\",\"link-arg=--shared-memory\",\
         \"-C\",\"link-arg=--max-memory={max_memory}\",\
         \"-C\",\"link-arg=--import-memory\",\
         \"-C\",\"link-arg=--export=__wasm_init_tls\",\
         \"-C\",\"link-arg=--export=__tls_size\",\
         \"-C\",\"link-arg=--export=__tls_align\",\
         \"-C\",\"link-arg=--export=__tls_base\"]"
    )
}

pub(crate) fn ensure_prerequisites() -> Result<(), Error> {
    let rustup = env::var_os("MYPLOTLIB_RUSTUP").unwrap_or_else(|| "rustup".into());
    ensure_prerequisites_with(&rustup)
}

fn ensure_prerequisites_with(rustup: &OsStr) -> Result<(), Error> {
    let toolchain = Command::new(rustup)
        .args(["run", TOOLCHAIN, "rustc", "--version"])
        .output()
        .map_err(|error| {
            Error::new(format!(
                "failed to run {}: {error}; install rustup, then run `{}`",
                Path::new(rustup).display(),
                install_command()
            ))
        })?;
    if !toolchain.status.success() {
        return Err(missing_prerequisite_error(format!(
            "the {TOOLCHAIN} toolchain is not available"
        )));
    }

    let components = Command::new(rustup)
        .args(["component", "list", "--installed", "--toolchain", TOOLCHAIN])
        .output()
        .map_err(|error| {
            Error::new(format!(
                "failed to inspect components for {TOOLCHAIN}: {error}"
            ))
        })?;
    if !components.status.success() {
        return Err(missing_prerequisite_error(format!(
            "failed to inspect components for {TOOLCHAIN}"
        )));
    }

    let installed = String::from_utf8_lossy(&components.stdout);
    let missing = missing_components(&installed);
    if !missing.is_empty() {
        return Err(missing_prerequisite_error(format!(
            "{TOOLCHAIN} is missing {}",
            missing.join(" and ")
        )));
    }

    Ok(())
}

fn missing_components(installed: &str) -> Vec<&'static str> {
    REQUIRED_COMPONENTS
        .iter()
        .copied()
        .filter(|required| !installed.lines().any(|line| line.trim() == *required))
        .collect()
}

fn missing_prerequisite_error(reason: String) -> Error {
    Error::new(format!(
        "threaded WebAssembly prerequisite failed: {reason}; run `{}`",
        install_command()
    ))
}

fn install_command() -> String {
    format!(
        "rustup toolchain install {TOOLCHAIN} --component rust-src --target wasm32-unknown-unknown"
    )
}

pub(crate) fn run_wasm_pack(program: &OsStr, arguments: &[OsString]) -> Result<(), Error> {
    let rustup = env::var_os("MYPLOTLIB_RUSTUP").unwrap_or_else(|| "rustup".into());
    let command_arguments = wasm_pack_command_arguments(program, arguments);
    let status = Command::new(&rustup)
        .args(&command_arguments)
        .status()
        .map_err(|error| {
            Error::new(format!(
                "failed to run {}: {error}",
                Path::new(&rustup).display()
            ))
        })?;

    if !status.success() {
        return Err(Error::new(format!(
            "threaded wasm-pack build failed with {status}"
        )));
    }
    Ok(())
}

fn wasm_pack_command_arguments(program: &OsStr, arguments: &[OsString]) -> Vec<OsString> {
    let mut command_arguments = vec![
        OsString::from("run"),
        OsString::from(TOOLCHAIN),
        program.to_owned(),
    ];
    command_arguments.extend_from_slice(arguments);
    command_arguments
}

pub(crate) fn validate_wasm(path: &Path, expected_max_memory: u64) -> Result<(), Error> {
    let bytes = fs::read(path)
        .map_err(|error| Error::new(format!("failed to read {}: {error}", path.display())))?;
    validate_wasm_bytes(path, &bytes, expected_max_memory)
}

fn validate_wasm_bytes(path: &Path, bytes: &[u8], expected_max_memory: u64) -> Result<(), Error> {
    let mut memories = Vec::new();
    let mut exports = Vec::new();

    for payload in Parser::new(0).parse_all(bytes) {
        match payload.map_err(|error| {
            Error::new(format!(
                "failed to parse threaded Wasm output {}: {error}",
                path.display()
            ))
        })? {
            Payload::ImportSection(section) => {
                for import in section.into_imports() {
                    let import = import.map_err(|error| {
                        Error::new(format!(
                            "failed to parse imports in {}: {error}",
                            path.display()
                        ))
                    })?;
                    if let TypeRef::Memory(memory) = import.ty {
                        memories.push(memory);
                    }
                }
            }
            Payload::MemorySection(section) => {
                for memory in section {
                    memories.push(memory.map_err(|error| {
                        Error::new(format!(
                            "failed to parse memory in {}: {error}",
                            path.display()
                        ))
                    })?);
                }
            }
            Payload::ExportSection(section) => {
                for export in section {
                    exports.push(
                        export
                            .map_err(|error| {
                                Error::new(format!(
                                    "failed to parse exports in {}: {error}",
                                    path.display()
                                ))
                            })?
                            .name
                            .to_owned(),
                    );
                }
            }
            _ => {}
        }
    }

    let memory = memories
        .iter()
        .find(|memory| memory.shared)
        .ok_or_else(|| {
            Error::new(format!(
                "threaded build did not produce shared memory in {}",
                path.display()
            ))
        })?;
    validate_memory(path, memory, expected_max_memory)?;

    for required in REQUIRED_EXPORTS {
        if !exports.iter().any(|export| export == required) {
            let guidance = if required == "initThreadPool" {
                "; add wasm-bindgen-rayon with its no-bundler feature and re-export init_thread_pool from the application crate"
            } else {
                ""
            };
            return Err(Error::new(format!(
                "threaded build did not export {required} from {}{guidance}",
                path.display()
            )));
        }
    }

    Ok(())
}

fn validate_memory(
    path: &Path,
    memory: &MemoryType,
    expected_max_memory: u64,
) -> Result<(), Error> {
    let page_size = 1_u64
        .checked_shl(memory.page_size_log2.unwrap_or(16))
        .ok_or_else(|| Error::new("threaded Wasm memory has an invalid page size"))?;
    let maximum_pages = memory.maximum.ok_or_else(|| {
        Error::new(format!(
            "threaded Wasm memory in {} has no maximum",
            path.display()
        ))
    })?;
    let maximum_bytes = maximum_pages.checked_mul(page_size).ok_or_else(|| {
        Error::new(format!(
            "threaded Wasm memory maximum in {} is too large",
            path.display()
        ))
    })?;
    if maximum_bytes != expected_max_memory {
        return Err(Error::new(format!(
            "threaded Wasm memory in {} has a maximum of {maximum_bytes} bytes; expected {expected_max_memory}",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_encoder::{ExportKind, ExportSection, MemorySection, Module};

    fn threaded_module(shared: bool, maximum_pages: Option<u64>, exports: &[&str]) -> Vec<u8> {
        let mut module = Module::new();
        let mut memories = MemorySection::new();
        memories.memory(wasm_encoder::MemoryType {
            minimum: 1,
            maximum: maximum_pages,
            memory64: false,
            shared,
            page_size_log2: None,
        });
        module.section(&memories);

        let mut export_section = ExportSection::new();
        for name in exports {
            export_section.export(name, ExportKind::Func, 0);
        }
        module.section(&export_section);
        module.finish()
    }

    #[test]
    fn validates_memory_limits() {
        assert!(validate_max_memory(DEFAULT_MAX_MEMORY).is_ok());
        assert!(validate_max_memory(0).is_err());
        assert!(validate_max_memory(WASM_PAGE_SIZE + 1).is_err());
        assert!(validate_max_memory(MAX_WASM32_MEMORY + WASM_PAGE_SIZE).is_err());
    }

    #[test]
    fn constructs_owned_cargo_configuration() {
        let arguments = cargo_arguments(DEFAULT_MAX_MEMORY);
        assert_eq!(arguments[0], "--config");
        let config = arguments[1].to_string_lossy();
        assert!(config.contains("target-feature=+atomics,+bulk-memory"));
        assert!(config.contains("link-arg=--shared-memory"));
        assert!(config.contains("link-arg=--max-memory=1073741824"));
        assert!(config.contains("link-arg=--export=__wasm_init_tls"));
        assert_eq!(arguments[2], "-Z");
        assert_eq!(arguments[3], BUILD_STD);
    }

    #[test]
    fn wraps_wasm_pack_in_the_pinned_toolchain() {
        let arguments = [OsString::from("build"), OsString::from("/work/app")];
        assert_eq!(
            wasm_pack_command_arguments(OsStr::new("wasm-pack"), &arguments),
            ["run", TOOLCHAIN, "wasm-pack", "build", "/work/app"]
                .map(OsString::from)
                .to_vec()
        );
    }

    #[test]
    fn reports_a_missing_rustup_executable() {
        let error =
            ensure_prerequisites_with(OsStr::new("/definitely/missing/cargo-myplotlib-rustup"))
                .unwrap_err();
        assert!(error.to_string().contains("install rustup"));
        assert!(error.to_string().contains(TOOLCHAIN));
    }

    #[test]
    fn identifies_missing_threaded_components() {
        assert_eq!(
            missing_components("rust-src\nrust-std-x86_64-unknown-linux-gnu\n"),
            ["rust-std-wasm32-unknown-unknown"]
        );
        assert!(missing_components("rust-src\nrust-std-wasm32-unknown-unknown\n").is_empty());
    }

    #[test]
    fn accepts_a_shared_module_with_the_expected_exports() {
        let bytes = threaded_module(
            true,
            Some(DEFAULT_MAX_MEMORY / WASM_PAGE_SIZE),
            &REQUIRED_EXPORTS,
        );
        validate_wasm_bytes(Path::new("test.wasm"), &bytes, DEFAULT_MAX_MEMORY).unwrap();
    }

    #[test]
    fn rejects_non_shared_and_misconfigured_modules() {
        let ordinary = threaded_module(
            false,
            Some(DEFAULT_MAX_MEMORY / WASM_PAGE_SIZE),
            &REQUIRED_EXPORTS,
        );
        let error =
            validate_wasm_bytes(Path::new("test.wasm"), &ordinary, DEFAULT_MAX_MEMORY).unwrap_err();
        assert!(error.to_string().contains("did not produce shared memory"));

        let wrong_maximum = threaded_module(true, Some(1), &REQUIRED_EXPORTS);
        let error = validate_wasm_bytes(Path::new("test.wasm"), &wrong_maximum, DEFAULT_MAX_MEMORY)
            .unwrap_err();
        assert!(error.to_string().contains("expected 1073741824"));

        let missing_export = threaded_module(
            true,
            Some(DEFAULT_MAX_MEMORY / WASM_PAGE_SIZE),
            &["__tls_base"],
        );
        let error =
            validate_wasm_bytes(Path::new("test.wasm"), &missing_export, DEFAULT_MAX_MEMORY)
                .unwrap_err();
        assert!(error.to_string().contains("initThreadPool"));
        assert!(error.to_string().contains("wasm-bindgen-rayon"));
    }
}

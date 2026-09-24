use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::process::Command;

mod server;
mod threaded;

const LOADER_JS: &str = include_str!("../../../web/loader.js");
const APP_JS: &str = include_str!("../../../web/app.js");

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const WEB_PACKAGE_VERSION: u32 = 1;

pub const HELP: &str = "\
Build embeddable Myplotlib web applications

Usage:
  cargo myplotlib build-web [OPTIONS]
  cargo myplotlib serve [OPTIONS]
  cargo myplotlib --version

Commands:
  build-web    Build a wasm-pack application and its Myplotlib entry module
  serve        Serve a web project with cross-origin isolation

Build options:
  --manifest-path <PATH>    Application Cargo.toml (default: Cargo.toml)
  --out-dir <PATH>          Output directory (default: dist)
  --release                 Build optimized Wasm
  --locked                  Require Cargo.lock to remain unchanged
  --wasm-threads            Build with shared-memory WebAssembly threads
  --wasm-max-memory <BYTES> Shared-memory maximum (default: 1073741824)
  -h, --help                Print help

Serve options:
  --dir <PATH>              Web root (default: current directory)
  --host <IP>               Bind address (default: 127.0.0.1)
  --port <PORT>             Port (default: 8080)
  -h, --help                Print help

General options:
  -V, --version             Print the cargo-myplotlib version
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildWeb {
    pub manifest_path: PathBuf,
    pub out_dir: PathBuf,
    pub release: bool,
    pub locked: bool,
    pub wasm_threads: bool,
    pub wasm_max_memory: u64,
}

impl Default for BuildWeb {
    fn default() -> Self {
        Self {
            manifest_path: PathBuf::from("Cargo.toml"),
            out_dir: PathBuf::from("dist"),
            release: false,
            locked: false,
            wasm_threads: false,
            wasm_max_memory: threaded::DEFAULT_MAX_MEMORY,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Serve {
    pub dir: PathBuf,
    pub host: IpAddr,
    pub port: u16,
}

impl Default for Serve {
    fn default() -> Self {
        Self {
            dir: PathBuf::from("."),
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8080,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Help,
    Version,
    BuildWeb(BuildWeb),
    Serve(Serve),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(String);

impl Error {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::error::Error for Error {}

pub fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Action, Error> {
    let mut args: Vec<OsString> = args.into_iter().collect();

    // Cargo may include the external subcommand name, depending on how the
    // binary is invoked. Supporting both also makes direct execution useful.
    if args.first().is_some_and(|argument| argument == "myplotlib") {
        args.remove(0);
    }

    let Some(command) = args.first().and_then(|argument| argument.to_str()) else {
        return Ok(Action::Help);
    };
    if matches!(command, "-h" | "--help") {
        return Ok(Action::Help);
    }
    if matches!(command, "-V" | "--version") {
        return Ok(Action::Version);
    }
    if command == "serve" {
        let mut config = Serve::default();
        let mut index = 1;
        while index < args.len() {
            let argument = args[index]
                .to_str()
                .ok_or_else(|| Error::new("options must be valid UTF-8"))?;
            match argument {
                "--dir" => {
                    index += 1;
                    config.dir = option_value(&args, index, "--dir")?;
                }
                "--host" => {
                    index += 1;
                    let value = text_option_value(&args, index, "--host")?;
                    config.host = value.parse().map_err(|_| {
                        Error::new(format!("--host requires an IP address, got: {value}"))
                    })?;
                }
                "--port" => {
                    index += 1;
                    let value = text_option_value(&args, index, "--port")?;
                    config.port = value.parse().map_err(|_| {
                        Error::new(format!(
                            "--port requires a number from 0 to 65535, got: {value}"
                        ))
                    })?;
                }
                "-h" | "--help" => return Ok(Action::Help),
                unknown => return Err(Error::new(format!("unknown serve option: {unknown}"))),
            }
            index += 1;
        }
        return Ok(Action::Serve(config));
    }
    if command != "build-web" {
        return Err(Error::new(format!("unknown command: {command}")));
    }

    let mut config = BuildWeb::default();
    let mut wasm_max_memory_set = false;
    let mut index = 1;
    while index < args.len() {
        let argument = args[index]
            .to_str()
            .ok_or_else(|| Error::new("options must be valid UTF-8"))?;
        match argument {
            "--manifest-path" => {
                index += 1;
                config.manifest_path = option_value(&args, index, "--manifest-path")?;
            }
            "--out-dir" => {
                index += 1;
                config.out_dir = option_value(&args, index, "--out-dir")?;
            }
            "--release" => config.release = true,
            "--locked" => config.locked = true,
            "--wasm-threads" => config.wasm_threads = true,
            "--wasm-max-memory" => {
                index += 1;
                let value = text_option_value(&args, index, "--wasm-max-memory")?;
                config.wasm_max_memory = value.parse().map_err(|_| {
                    Error::new(format!(
                        "--wasm-max-memory requires a number of bytes, got: {value}"
                    ))
                })?;
                wasm_max_memory_set = true;
            }
            "-h" | "--help" => return Ok(Action::Help),
            unknown => return Err(Error::new(format!("unknown build-web option: {unknown}"))),
        }
        index += 1;
    }

    if wasm_max_memory_set && !config.wasm_threads {
        return Err(Error::new("--wasm-max-memory requires --wasm-threads"));
    }
    if config.wasm_threads {
        threaded::validate_max_memory(config.wasm_max_memory)?;
    }

    Ok(Action::BuildWeb(config))
}

fn option_value(args: &[OsString], index: usize, option: &str) -> Result<PathBuf, Error> {
    args.get(index)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| Error::new(format!("{option} requires a path")))
}

fn text_option_value<'a>(
    args: &'a [OsString],
    index: usize,
    option: &str,
) -> Result<&'a str, Error> {
    args.get(index)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::new(format!("{option} requires a value")))?
        .to_str()
        .ok_or_else(|| Error::new(format!("{option} must be valid UTF-8")))
}

pub fn run(action: Action) -> Result<(), Error> {
    match action {
        Action::Help => {
            print!("{HELP}");
            Ok(())
        }
        Action::Version => {
            println!("{}", version_line());
            Ok(())
        }
        Action::BuildWeb(config) => build_web(config),
        Action::Serve(config) => server::serve(config),
    }
}

pub fn version_line() -> String {
    format!("cargo-myplotlib {VERSION}")
}

pub fn build_web(config: BuildWeb) -> Result<(), Error> {
    let working_directory = env::current_dir()
        .map_err(|error| Error::new(format!("failed to read the working directory: {error}")))?;
    let manifest_path = absolute_path(&working_directory, &config.manifest_path);
    let manifest_path = fs::canonicalize(&manifest_path).map_err(|error| {
        Error::new(format!(
            "failed to open manifest {}: {error}",
            manifest_path.display()
        ))
    })?;
    if !manifest_path.is_file() {
        return Err(Error::new(format!(
            "manifest path is not a file: {}",
            manifest_path.display()
        )));
    }
    let crate_directory = manifest_path
        .parent()
        .ok_or_else(|| Error::new("manifest path has no parent directory"))?;
    let out_dir = absolute_path(&working_directory, &config.out_dir);

    let arguments = wasm_pack_arguments(crate_directory, &out_dir, &config);
    let program = env::var_os("MYPLOTLIB_WASM_PACK").unwrap_or_else(|| "wasm-pack".into());
    if config.wasm_threads {
        threaded::ensure_prerequisites()?;
        threaded::run_wasm_pack(&program, &arguments)?;
    } else {
        run_wasm_pack(&program, &arguments)?;
    }
    validate_wasm_pack_output(&out_dir)?;
    if config.wasm_threads {
        threaded::validate_wasm(&out_dir.join("bindings_bg.wasm"), config.wasm_max_memory)?;
    }
    write_web_assets(&out_dir)?;

    println!("Myplotlib web package written to {}", out_dir.display());
    Ok(())
}

fn absolute_path(working_directory: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        working_directory.join(path)
    }
}

pub fn wasm_pack_arguments(
    crate_directory: &Path,
    out_dir: &Path,
    config: &BuildWeb,
) -> Vec<OsString> {
    let mut arguments = vec![
        OsString::from("build"),
        crate_directory.as_os_str().to_owned(),
        OsString::from("--target"),
        OsString::from("web"),
        OsString::from("--out-dir"),
        out_dir.as_os_str().to_owned(),
        OsString::from("--out-name"),
        OsString::from("bindings"),
        OsString::from("--no-typescript"),
        OsString::from("--no-pack"),
    ];
    if config.release {
        arguments.push(OsString::from("--release"));
    }
    let mut cargo_arguments = Vec::new();
    if config.wasm_threads {
        cargo_arguments.extend(threaded::cargo_arguments(config.wasm_max_memory));
    }
    if config.locked {
        cargo_arguments.push(OsString::from("--locked"));
    }
    if !cargo_arguments.is_empty() {
        arguments.push(OsString::from("--"));
        arguments.extend(cargo_arguments);
    }
    arguments
}

fn run_wasm_pack(program: &OsStr, arguments: &[OsString]) -> Result<(), Error> {
    let status = Command::new(program)
        .args(arguments)
        .status()
        .map_err(|error| {
            Error::new(format!(
                "failed to run {}: {error}; install wasm-pack or set MYPLOTLIB_WASM_PACK",
                Path::new(program).display()
            ))
        })?;

    if !status.success() {
        return Err(Error::new(format!("wasm-pack failed with {status}")));
    }
    Ok(())
}

fn validate_wasm_pack_output(out_dir: &Path) -> Result<(), Error> {
    for file in ["bindings.js", "bindings_bg.wasm"] {
        let path = out_dir.join(file);
        if !path.is_file() {
            return Err(Error::new(format!(
                "wasm-pack succeeded but did not create {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn write_web_assets(out_dir: &Path) -> Result<(), Error> {
    fs::create_dir_all(out_dir).map_err(|error| {
        Error::new(format!(
            "failed to create output directory {}: {error}",
            out_dir.display()
        ))
    })?;
    write_asset(
        &out_dir.join("myplotlib-loader.js"),
        &format!("{}{LOADER_JS}", generated_notice()),
    )?;
    write_asset(
        &out_dir.join("app.js"),
        &format!(
            "{}export const cargoMyplotlibVersion = {VERSION:?};\n\
             export const myplotlibWebPackageVersion = {WEB_PACKAGE_VERSION};\n\n\
             {APP_JS}",
            generated_notice(),
        ),
    )?;
    Ok(())
}

fn generated_notice() -> String {
    format!(
        "// Generated by cargo-myplotlib {VERSION}; web package format {WEB_PACKAGE_VERSION}; do not edit.\n\n"
    )
}

fn write_asset(path: &Path, contents: &str) -> Result<(), Error> {
    fs::write(path, contents)
        .map_err(|error| Error::new(format!("failed to write {}: {error}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn arguments(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    fn temporary_directory() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = env::temp_dir().join(format!(
            "cargo-myplotlib-test-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn parses_build_options_with_or_without_cargo_prefix() {
        let expected = Action::BuildWeb(BuildWeb {
            manifest_path: PathBuf::from("example/Cargo.toml"),
            out_dir: PathBuf::from("public/app"),
            release: true,
            locked: true,
            wasm_threads: false,
            wasm_max_memory: threaded::DEFAULT_MAX_MEMORY,
        });
        let options = [
            "build-web",
            "--manifest-path",
            "example/Cargo.toml",
            "--out-dir",
            "public/app",
            "--release",
            "--locked",
        ];

        assert_eq!(parse_args(arguments(&options)).unwrap(), expected);

        let mut prefixed = vec!["myplotlib"];
        prefixed.extend(options);
        assert_eq!(parse_args(arguments(&prefixed)).unwrap(), expected);
    }

    #[test]
    fn reports_the_release_version_with_or_without_cargo_prefix() {
        assert_eq!(
            parse_args(arguments(&["--version"])).unwrap(),
            Action::Version
        );
        assert_eq!(
            parse_args(arguments(&["myplotlib", "--version"])).unwrap(),
            Action::Version
        );
        assert_eq!(version_line(), format!("cargo-myplotlib {VERSION}"));
    }

    #[test]
    fn parses_serve_options_with_or_without_cargo_prefix() {
        assert_eq!(
            parse_args(arguments(&["serve"])).unwrap(),
            Action::Serve(Serve::default())
        );

        let expected = Action::Serve(Serve {
            dir: PathBuf::from("public"),
            host: "0.0.0.0".parse().unwrap(),
            port: 3000,
        });
        let options = [
            "serve", "--dir", "public", "--host", "0.0.0.0", "--port", "3000",
        ];
        assert_eq!(parse_args(arguments(&options)).unwrap(), expected);

        let mut prefixed = vec!["myplotlib"];
        prefixed.extend(options);
        assert_eq!(parse_args(arguments(&prefixed)).unwrap(), expected);
    }

    #[test]
    fn rejects_invalid_serve_options() {
        let error = parse_args(arguments(&["serve", "--host", "localhost"])).unwrap_err();
        assert!(error.to_string().contains("requires an IP address"));

        let error = parse_args(arguments(&["serve", "--port", "65536"])).unwrap_err();
        assert!(error.to_string().contains("requires a number"));

        let error = parse_args(arguments(&["serve", "--dir"])).unwrap_err();
        assert!(error.to_string().contains("requires a path"));
    }

    #[test]
    fn rejects_unknown_and_incomplete_options() {
        let error = parse_args(arguments(&["build-web", "--unknown"])).unwrap_err();
        assert!(error.to_string().contains("unknown build-web option"));

        let error = parse_args(arguments(&["build-web", "--out-dir"])).unwrap_err();
        assert!(error.to_string().contains("requires a path"));

        let error =
            parse_args(arguments(&["build-web", "--wasm-max-memory", "1073741824"])).unwrap_err();
        assert!(error.to_string().contains("requires --wasm-threads"));

        let error = parse_args(arguments(&[
            "build-web",
            "--wasm-threads",
            "--wasm-max-memory",
            "123",
        ]))
        .unwrap_err();
        assert!(error.to_string().contains("multiple of 65536"));
    }

    #[test]
    fn parses_threaded_build_options() {
        let action = parse_args(arguments(&[
            "build-web",
            "--wasm-threads",
            "--wasm-max-memory",
            "2147483648",
        ]))
        .unwrap();
        let Action::BuildWeb(config) = action else {
            panic!("expected a build action");
        };
        assert!(config.wasm_threads);
        assert_eq!(config.wasm_max_memory, 2_147_483_648);
    }

    #[test]
    fn constructs_deterministic_wasm_pack_arguments() {
        let config = BuildWeb {
            manifest_path: PathBuf::from("Cargo.toml"),
            out_dir: PathBuf::from("pkg"),
            release: true,
            locked: true,
            wasm_threads: false,
            wasm_max_memory: threaded::DEFAULT_MAX_MEMORY,
        };
        let actual = wasm_pack_arguments(Path::new("/work/app"), Path::new("/work/pkg"), &config);
        let expected = arguments(&[
            "build",
            "/work/app",
            "--target",
            "web",
            "--out-dir",
            "/work/pkg",
            "--out-name",
            "bindings",
            "--no-typescript",
            "--no-pack",
            "--release",
            "--",
            "--locked",
        ]);

        assert_eq!(actual, expected);
    }

    #[test]
    fn appends_threaded_cargo_options_after_wasm_pack_options() {
        let config = BuildWeb {
            wasm_threads: true,
            locked: true,
            ..BuildWeb::default()
        };
        let actual = wasm_pack_arguments(Path::new("/work/app"), Path::new("/work/pkg"), &config);
        let separator = actual.iter().position(|value| value == "--").unwrap();
        assert_eq!(actual[separator + 1], "--config");
        assert!(
            actual[separator + 2]
                .to_string_lossy()
                .contains("link-arg=--shared-memory")
        );
        assert_eq!(actual[separator + 3], "-Z");
        assert_eq!(actual[separator + 4], "build-std=panic_abort,std");
        assert_eq!(actual[separator + 5], "--locked");
    }

    #[test]
    fn reports_a_missing_wasm_pack_executable() {
        let error = run_wasm_pack(
            OsStr::new("/definitely/missing/cargo-myplotlib-wasm-pack"),
            &[],
        )
        .unwrap_err();
        assert!(error.to_string().contains("failed to run"));
        assert!(error.to_string().contains("install wasm-pack"));
    }

    #[test]
    fn writes_the_public_entry_and_private_loader() {
        let directory = temporary_directory();
        write_web_assets(&directory).unwrap();

        let app = fs::read_to_string(directory.join("app.js")).unwrap();
        let loader = fs::read_to_string(directory.join("myplotlib-loader.js")).unwrap();
        let notice = generated_notice();
        assert!(app.starts_with(&notice));
        assert!(app.contains(&format!(
            "export const cargoMyplotlibVersion = {VERSION:?};"
        )));
        assert!(app.contains(&format!(
            "export const myplotlibWebPackageVersion = {WEB_PACKAGE_VERSION};"
        )));
        assert!(app.contains("export function mount"));
        assert!(app.contains("./bindings.js"));
        assert!(loader.starts_with(&notice));
        assert!(loader.contains("export function mountApp"));

        fs::remove_dir_all(directory).unwrap();
    }
}

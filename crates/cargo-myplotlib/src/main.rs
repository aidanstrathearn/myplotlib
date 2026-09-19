use std::ffi::OsString;
use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    let action = match cargo_myplotlib::parse_args(arguments) {
        Ok(action) => action,
        Err(error) => {
            eprintln!("error: {error}\n\n{}", cargo_myplotlib::HELP);
            return ExitCode::FAILURE;
        }
    };

    match cargo_myplotlib::run(action) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

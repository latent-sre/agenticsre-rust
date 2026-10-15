use std::io;
use std::process::ExitCode;

fn main() -> ExitCode {
    if std::env::args_os().len() != 1 {
        eprintln!("capacity-projector accepts only the versioned stdin protocol");
        return ExitCode::FAILURE;
    }
    match capacity_projector::serve(&mut io::stdin().lock(), &mut io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("capacity-projector: {error}");
            ExitCode::FAILURE
        }
    }
}

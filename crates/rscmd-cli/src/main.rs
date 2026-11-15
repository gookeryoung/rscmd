use std::process::ExitCode;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let outcome = rscmd_core::dispatch(rscmd_tools::tools(), &argv);
    let (stdout, stderr) = rscmd_core::render(&outcome);

    if !stdout.is_empty() {
        print!("{stdout}");
    }
    if !stderr.is_empty() {
        eprint!("{stderr}");
    }

    ExitCode::from(outcome.exit_code().clamp(0, 255) as u8)
}

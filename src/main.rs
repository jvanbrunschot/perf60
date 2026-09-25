use std::io::Write;
use std::process::ExitCode;

use perf60::cli::{self, Command};
use perf60::report;
use perf60::source::FsSource;

fn main() -> ExitCode {
    let opts = match cli::parse(std::env::args().skip(1)) {
        Ok(Command::Run(o)) => o,
        Ok(Command::Help) => {
            println!("{}", cli::USAGE);
            return ExitCode::SUCCESS;
        }
        Ok(Command::Version) => {
            println!("perf60 {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("perf60: {e}\n\n{}", cli::USAGE);
            return ExitCode::from(3);
        }
    };

    if !cfg!(target_os = "linux") {
        eprintln!("perf60 supports Linux only");
        return ExitCode::from(3);
    }

    let r = perf60::analyze(&FsSource::root(), opts.interval, opts.count);
    let out = if opts.json {
        report::json::render(&r)
    } else {
        let color = opts.color && std::env::var_os("NO_COLOR").is_none() && stdout_is_tty();
        report::text::render(&r, color, opts.verbose)
    };
    // Ignore EPIPE (e.g. `perf60 | head`): the exit code still reflects the system state.
    let _ = std::io::stdout().lock().write_all(out.as_bytes());
    ExitCode::from(r.overall.exit_code() as u8)
}

fn stdout_is_tty() -> bool {
    // SAFETY: isatty only inspects the file descriptor.
    unsafe { libc::isatty(libc::STDOUT_FILENO) == 1 }
}

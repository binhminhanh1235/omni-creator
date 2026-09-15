use std::{env, io, process};

#[path = "mcp_phase19_p2.rs"]
mod mcp;
mod phase19_cli;

#[tokio::main]
async fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if mcp::is_mcp_invocation_v1(&args) {
        if let Err(error) = mcp::serve_mcp_stdio_phase19_p2_from_args_v1(args).await {
            eprintln!("{error}");
            process::exit(70);
        }
        return;
    }

    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let result = phase19_cli::maybe_run_phase19_work_cli_v1(&args, &mut stdin)
        .unwrap_or_else(|| omnicreator_cli::run_cli_v1(args, &mut stdin));
    println!("{}", result.output);
    process::exit(result.exit_code);
}

use std::{env, io, process};

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let result = omnicreator_cli::run_cli_v1(args, &mut stdin);
    println!("{}", result.output);
    process::exit(result.exit_code);
}

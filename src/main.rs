use clap::Parser;

use kvstore::cli::Cli;

fn main() {
    let cli = Cli::parse();
    if let Err(error) = kvstore::run(cli) {
        eprintln!("Error: {error}");
        std::process::exit(1);
    }
}

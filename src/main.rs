use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[clap(name = "gpg-bridge", version)]
pub struct App {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Server {
        #[clap(flatten)]
        global_opts: GlobalOpts,
    },
}

#[derive(Debug, Args)]
pub struct GlobalOpts {
    path: Option<String>,
}

fn main() {
    let app = App::parse();

    match app.command {
        Command::Server { global_opts: opts } => {
            println!("Starting server using config {opts:?}");
        }
    }
}

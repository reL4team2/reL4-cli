mod install;
use clap::Parser;

#[derive(Debug, Parser)]
pub struct Options {
    /// The command to run
    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug, Parser)]
enum Command {
    /// Install develop dependency, such as reL4 kernel, reL4-linux-kit, libseL4
    #[command(about = "Install develop dependency, such as reL4 kernel, reL4-linux-kit, libseL4")]
    Install(install::InstallOptions),
}

fn main() -> anyhow::Result<()> {
    let opts = Options::parse();
    match opts.command {
        Command::Install(install_opts) => {
            install::install(install_opts)?;
        }
    }
    Ok(())
}
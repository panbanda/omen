use async_process::Command as AsyncProcessCommand;
use async_std::process::Command as AsyncStdCommand;
use clap::Command as Cli;
use smol::process::Command as SmolCommand;
use tokio::process::Command;

pub async fn tokio_command(program: &str) {
    tokio::process::Command::new(program);
    Command::new("sh").arg("-c").arg(program);
    tokio::process::id();
    let text = "tokio::process::Command::new(program)";
    // tokio::process::Command::new(program)
}

pub async fn smol_command(program: &str) {
    smol::process::Command::new(program);
    SmolCommand::new(program);
    Cli::new("smol");
    let text = "smol::process::Command::new(program)";
    // smol::process::Command::new(program)
}

pub async fn async_process_command(program: &str) {
    async_process::Command::new(program);
    AsyncProcessCommand::new(program);
    let text = "async_process::Command::new(program)";
    // async_process::Command::new(program)
}

pub async fn async_std_command(program: &str) {
    async_std::process::Command::new(program);
    AsyncStdCommand::new(program);
    let text = "async_std::process::Command::new(program)";
    // async_std::process::Command::new(program)
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn commands() {
        let program = "sh";
        tokio::process::Command::new(program);
        smol::process::Command::new(program);
        async_process::Command::new(program);
        async_std::process::Command::new(program);
    }
}

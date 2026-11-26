use std::{path::Path, process::Command};

use clap::ValueEnum;

#[derive(Clone, Copy, Default, ValueEnum)]
pub enum ContainerRuntime {
    #[default]
    Docker,
    Kiri,
}

impl ContainerRuntime {
    pub fn executable(self) -> &'static str {
        match self {
            Self::Docker => "docker",
            Self::Kiri => "kiri",
        }
    }

    pub fn compose(self, project: &Path) -> Command {
        let mut command = Command::new(self.executable());
        command.current_dir(project).arg("compose");
        command
    }
}

use crate::mapping;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

const SERVICE_NAME: &str = "hm-cx.service";
const SYSTEMD_SERVICE_PORT: u16 = 8085;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    GenerateConfig,
    Install,
    Uninstall,
}

pub fn parse_command(args: &[String]) -> Result<Option<Command>, String> {
    match args {
        [] => Ok(None),
        [command] => match command.as_str() {
            "generate-config" => Ok(Some(Command::GenerateConfig)),
            "install" => Ok(Some(Command::Install)),
            "uninstall" => Ok(Some(Command::Uninstall)),
            _ => Err(format!(
                "unknown command '{command}'; expected one of: generate-config, install, uninstall"
            )),
        },
        [command, ..] => Err(format!(
            "command '{command}' does not accept extra arguments"
        )),
    }
}

pub fn run_command(command: Command) -> Result<(), String> {
    match command {
        Command::GenerateConfig => mapping::generate_config(),
        Command::Install => install_systemd_user_service(),
        Command::Uninstall => uninstall_systemd_user_service(),
    }
}

#[derive(Debug, Clone)]
struct ServiceContext {
    home_dir: PathBuf,
    executable_path: PathBuf,
    working_directory: PathBuf,
}

impl ServiceContext {
    fn from_environment() -> Result<Self, String> {
        let home_dir = env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| "HOME is not set".to_string())?;
        let executable_path =
            env::current_exe().map_err(|err| format!("current executable: {err}"))?;
        let working_directory =
            env::current_dir().map_err(|err| format!("current working directory: {err}"))?;

        Ok(Self {
            home_dir,
            executable_path,
            working_directory,
        })
    }

    fn service_dir(&self) -> PathBuf {
        self.home_dir.join(".config/systemd/user")
    }

    fn service_file(&self) -> PathBuf {
        self.service_dir().join(SERVICE_NAME)
    }
}

trait SystemctlRunner {
    fn run_systemctl_user(&mut self, args: &[&str]) -> Result<(), String>;
}

struct RealSystemctlRunner;

impl SystemctlRunner for RealSystemctlRunner {
    fn run_systemctl_user(&mut self, args: &[&str]) -> Result<(), String> {
        let output = ProcessCommand::new("systemctl")
            .arg("--user")
            .args(args)
            .output()
            .map_err(|err| format!("failed to run systemctl --user {}: {err}", args.join(" ")))?;

        if output.status.success() {
            return Ok(());
        }

        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let details = if stderr.is_empty() {
            format!("exit status {}", output.status)
        } else {
            stderr
        };

        Err(format!(
            "systemctl --user {} failed: {}",
            args.join(" "),
            details
        ))
    }
}

fn install_systemd_user_service() -> Result<(), String> {
    let context = ServiceContext::from_environment()?;
    let mut runner = RealSystemctlRunner;
    install_systemd_user_service_with(&context, &mut runner)
}

fn uninstall_systemd_user_service() -> Result<(), String> {
    let context = ServiceContext::from_environment()?;
    let mut runner = RealSystemctlRunner;
    uninstall_systemd_user_service_with(&context, &mut runner)
}

fn install_systemd_user_service_with(
    context: &ServiceContext,
    runner: &mut impl SystemctlRunner,
) -> Result<(), String> {
    let service_dir = context.service_dir();
    let service_file = context.service_file();

    fs::create_dir_all(&service_dir)
        .map_err(|err| format!("create {}: {err}", service_dir.display()))?;
    // Keep the install-time working directory so config.yml and overload.json resolve consistently.
    let service_contents =
        render_systemd_user_service(&context.executable_path, &context.working_directory);
    fs::write(&service_file, service_contents)
        .map_err(|err| format!("write {}: {err}", service_file.display()))?;

    runner.run_systemctl_user(&["daemon-reload"])?;
    runner.run_systemctl_user(&["enable", "--now", SERVICE_NAME])?;
    // Restart after rewriting the unit so reinstalling applies updated environment settings.
    runner.run_systemctl_user(&["restart", SERVICE_NAME])?;

    println!(
        "Installed {} at {} and enabled it for future login sessions.",
        SERVICE_NAME,
        service_file.display()
    );
    Ok(())
}

fn uninstall_systemd_user_service_with(
    context: &ServiceContext,
    runner: &mut impl SystemctlRunner,
) -> Result<(), String> {
    let service_file = context.service_file();

    if !service_file.exists() {
        println!(
            "{} is not installed at {}.",
            SERVICE_NAME,
            service_file.display()
        );
        return Ok(());
    }

    runner.run_systemctl_user(&["disable", "--now", SERVICE_NAME])?;
    fs::remove_file(&service_file)
        .map_err(|err| format!("remove {}: {err}", service_file.display()))?;
    runner.run_systemctl_user(&["daemon-reload"])?;

    println!("Removed {} from {}.", SERVICE_NAME, service_file.display());
    Ok(())
}

fn render_systemd_user_service(executable_path: &Path, working_directory: &Path) -> String {
    format!(
        "[Unit]\nDescription=hm-cx sensor server\nAfter=default.target\n\n[Service]\nType=simple\nWorkingDirectory={}\nEnvironment=PORT={}\nExecStart={}\nRestart=on-failure\nRestartSec=5\n\n[Install]\nWantedBy=default.target\n",
        systemd_path_value(working_directory),
        SYSTEMD_SERVICE_PORT,
        systemd_path_value(executable_path),
    )
}

fn systemd_path_value(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[derive(Default)]
    struct RecordingSystemctlRunner {
        calls: Vec<Vec<String>>,
    }

    impl SystemctlRunner for RecordingSystemctlRunner {
        fn run_systemctl_user(&mut self, args: &[&str]) -> Result<(), String> {
            self.calls
                .push(args.iter().map(|arg| (*arg).to_string()).collect());
            Ok(())
        }
    }

    #[test]
    fn install_command_writes_service_and_enables_it() {
        let temp = tempdir().expect("create temp dir for install test");
        let home_dir = temp.path().join("home");
        let working_directory = temp.path().join("workspace");
        let executable_path = temp.path().join("bin/hm_cx");
        fs::create_dir_all(&working_directory).expect("create working directory for install test");
        fs::create_dir_all(executable_path.parent().expect("binary parent exists"))
            .expect("create binary directory for install test");
        fs::write(&executable_path, "#!/bin/sh\n")
            .expect("create fake executable for install test");

        let context = ServiceContext {
            home_dir: home_dir.clone(),
            executable_path: executable_path.clone(),
            working_directory: working_directory.clone(),
        };
        let mut runner = RecordingSystemctlRunner::default();

        install_systemd_user_service_with(&context, &mut runner)
            .expect("install command should succeed");

        let service_file = home_dir.join(".config/systemd/user").join(SERVICE_NAME);
        let service_contents = fs::read_to_string(&service_file)
            .expect("service file should be written by install command");

        assert!(service_contents.contains(&format!(
            "WorkingDirectory={}",
            systemd_path_value(&working_directory)
        )));
        assert!(service_contents.contains(&format!("Environment=PORT={}", SYSTEMD_SERVICE_PORT)));
        assert!(service_contents.contains(&format!(
            "ExecStart={}",
            systemd_path_value(&executable_path)
        )));
        assert_eq!(
            runner.calls,
            vec![
                vec!["daemon-reload".to_string()],
                vec![
                    "enable".to_string(),
                    "--now".to_string(),
                    SERVICE_NAME.to_string(),
                ],
                vec!["restart".to_string(), SERVICE_NAME.to_string()],
            ]
        );
    }

    #[test]
    fn uninstall_command_disables_service_and_removes_file() {
        let temp = tempdir().expect("create temp dir for uninstall test");
        let home_dir = temp.path().join("home");
        let service_dir = home_dir.join(".config/systemd/user");
        let service_file = service_dir.join(SERVICE_NAME);
        fs::create_dir_all(&service_dir).expect("create service directory for uninstall test");
        fs::write(&service_file, "[Unit]\nDescription=test\n")
            .expect("create fake service file for uninstall test");

        let context = ServiceContext {
            home_dir,
            executable_path: temp.path().join("bin/hm_cx"),
            working_directory: temp.path().join("workspace"),
        };
        let mut runner = RecordingSystemctlRunner::default();

        uninstall_systemd_user_service_with(&context, &mut runner)
            .expect("uninstall command should succeed");

        assert!(
            !service_file.exists(),
            "uninstall command should remove the service file"
        );
        assert_eq!(
            runner.calls,
            vec![
                vec![
                    "disable".to_string(),
                    "--now".to_string(),
                    SERVICE_NAME.to_string(),
                ],
                vec!["daemon-reload".to_string()],
            ]
        );
    }

    #[test]
    fn parse_command_accepts_install_and_uninstall() {
        assert_eq!(
            parse_command(&["install".to_string()]).expect("install should parse"),
            Some(Command::Install)
        );
        assert_eq!(
            parse_command(&["uninstall".to_string()]).expect("uninstall should parse"),
            Some(Command::Uninstall)
        );
    }
}

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    Status,
    PlanInstall { artifacts: PathBuf },
    Install { artifacts: PathBuf },
    PlanUninstall,
    Uninstall,
    Repair { backup: PathBuf },
}

pub fn authorize_command(command: &Command, effective_uid: u32) -> Result<(), CliError> {
    let mutating = matches!(
        command,
        Command::Install { .. } | Command::Uninstall | Command::Repair { .. }
    );
    if mutating && effective_uid != 0 {
        Err(CliError::RootRequired)
    } else {
        Ok(())
    }
}

pub fn parse_args<I, S>(arguments: I) -> Result<Command, CliError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let arguments: Vec<String> = arguments
        .into_iter()
        .map(|argument| argument.as_ref().to_owned())
        .collect();
    let Some(command) = arguments.first().map(String::as_str) else {
        return Err(CliError::Usage);
    };
    let options = &arguments[1..];
    match command {
        "status" if options.is_empty() => Ok(Command::Status),
        "plan-uninstall" if options.is_empty() => Ok(Command::PlanUninstall),
        "uninstall" => {
            require_exact_flag(options, "--apply")?;
            Ok(Command::Uninstall)
        }
        "plan-install" => {
            let artifacts = one_path_option(options, "--artifacts", false)?;
            Ok(Command::PlanInstall { artifacts })
        }
        "install" => {
            let artifacts = one_path_option(options, "--artifacts", true)?;
            Ok(Command::Install { artifacts })
        }
        "repair" => {
            let backup = one_path_option(options, "--backup", true)?;
            validate_explicit_backup(&backup)?;
            Ok(Command::Repair { backup })
        }
        _ => Err(CliError::Usage),
    }
}

fn require_exact_flag(options: &[String], required: &str) -> Result<(), CliError> {
    if options == [required] {
        Ok(())
    } else {
        Err(CliError::Usage)
    }
}

fn one_path_option(
    options: &[String],
    path_option: &str,
    require_apply: bool,
) -> Result<PathBuf, CliError> {
    let expected_len = if require_apply { 3 } else { 2 };
    if options.len() != expected_len {
        return Err(CliError::Usage);
    }
    let mut path = None;
    let mut apply = false;
    let mut index = 0;
    while index < options.len() {
        match options[index].as_str() {
            "--apply" if require_apply && !apply => {
                apply = true;
                index += 1;
            }
            value if value == path_option && path.is_none() => {
                let Some(candidate) = options.get(index + 1) else {
                    return Err(CliError::Usage);
                };
                if candidate.starts_with('-') {
                    return Err(CliError::Usage);
                }
                path = Some(PathBuf::from(candidate));
                index += 2;
            }
            _ => return Err(CliError::Usage),
        }
    }
    if require_apply && !apply {
        return Err(CliError::ApplyRequired);
    }
    let path = path.ok_or(CliError::Usage)?;
    validate_absolute_clean_path(&path)?;
    Ok(path)
}

fn validate_explicit_backup(path: &Path) -> Result<(), CliError> {
    validate_absolute_clean_path(path)
}

fn validate_absolute_clean_path(path: &Path) -> Result<(), CliError> {
    if !path.is_absolute()
        || path == Path::new("/")
        || path
            .components()
            .any(|component| !matches!(component, Component::RootDir | Component::Normal(_)))
    {
        return Err(CliError::UnsafePath);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CliError {
    Usage,
    ApplyRequired,
    UnsafePath,
    RootRequired,
}

impl Display for CliError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage => formatter.write_str(
                "usage: repose-unlockctl status | plan-install --artifacts <absolute-dir> | install --artifacts <absolute-dir> --apply | plan-uninstall | uninstall --apply | repair --apply --backup <absolute-path>",
            ),
            Self::ApplyRequired => formatter.write_str("mutating commands require the exact --apply flag"),
            Self::UnsafePath => formatter.write_str("paths must be absolute and contain no traversal"),
            Self::RootRequired => {
                formatter.write_str("mutating --apply commands require effective uid 0")
            }
        }
    }
}

impl Error for CliError {}

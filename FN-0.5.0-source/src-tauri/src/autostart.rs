use crate::error::{AppError, AppResult};

const TASK_NAME: &str = "FN Autostart";

#[cfg(windows)]
fn run_schtasks(args: &[&str]) -> AppResult<std::process::Output> {
    use std::os::windows::process::CommandExt;

    std::process::Command::new("schtasks.exe")
        .args(args)
        .creation_flags(crate::proc::CREATE_NO_WINDOW)
        .output()
        .map_err(Into::into)
}

#[cfg(windows)]
pub fn is_enabled() -> bool {
    run_schtasks(&["/Query", "/TN", TASK_NAME]).is_ok_and(|output| output.status.success())
}

#[cfg(not(windows))]
pub fn is_enabled() -> bool {
    false
}

#[cfg(windows)]
pub fn set_enabled(enabled: bool) -> AppResult<()> {
    if !enabled {
        if !is_enabled() {
            return Ok(());
        }
        let output = run_schtasks(&["/Delete", "/TN", TASK_NAME, "/F"])?;
        return command_result(output, "не удалось отключить автозапуск FN");
    }

    let executable = std::env::current_exe()?;
    let task_command = quote_task_command(&executable.to_string_lossy());
    let account = current_account()?;
    let output = run_schtasks(&[
        "/Create",
        "/TN",
        TASK_NAME,
        "/TR",
        &task_command,
        "/SC",
        "ONLOGON",
        "/RL",
        "HIGHEST",
        "/RU",
        &account,
        "/IT",
        "/DELAY",
        "0000:10",
        "/F",
    ])?;
    command_result(output, "не удалось включить автозапуск FN")
}

#[cfg(not(windows))]
pub fn set_enabled(_enabled: bool) -> AppResult<()> {
    Err(AppError::Msg(
        "автозапуск поддерживается только в Windows".into(),
    ))
}

#[cfg(windows)]
fn command_result(output: std::process::Output, context: &str) -> AppResult<()> {
    if output.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if detail.is_empty() {
        Err(AppError::Msg(context.into()))
    } else {
        Err(AppError::Msg(format!("{context}: {detail}")))
    }
}

fn quote_task_command(path: &str) -> String {
    format!("\"{path}\" --autostart")
}

fn current_account() -> AppResult<String> {
    let username = std::env::var("USERNAME")
        .map_err(|_| AppError::Msg("не удалось определить пользователя Windows".into()))?;
    let domain = std::env::var("USERDOMAIN").unwrap_or_default();
    if domain.is_empty() {
        Ok(username)
    } else {
        Ok(format!("{domain}\\{username}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_command_quotes_paths_with_spaces() {
        assert_eq!(
            quote_task_command(r"C:\Program Files\FN\fn-app.exe"),
            r#""C:\Program Files\FN\fn-app.exe" --autostart"#
        );
    }
}

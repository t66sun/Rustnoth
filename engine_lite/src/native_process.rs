//! Runs the original referee as an isolated audit process.

use std::{
    fs::File,
    io,
    path::{self, Path},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

pub const STDOUT_FILE: &str = "native.stdout";
pub const STDERR_FILE: &str = "native.stderr";

const EXPECTED_VERSION: &str = "1.18.8";
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Checks the version reported by the original Wesnoth executable.
pub fn check_version(executable: &Path) -> io::Result<()> {
    let output = command(executable)?.arg("--simple-version").output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "Wesnoth version query exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let version = String::from_utf8_lossy(&output.stdout);
    if version.trim() != EXPECTED_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected Wesnoth {EXPECTED_VERSION}, got {:?}", version.trim()),
        ));
    }
    Ok(())
}

/// Runs the `rustnoth_trace` scenario from the isolated run directory's userdata.
///
/// Both streams go directly to files in `run_dir`. A normal process exit is
/// returned with its original status. On timeout the child is killed and reaped.
pub fn run(
    executable: &Path,
    run_dir: &Path,
    seed: u32,
    timeout: Duration,
) -> io::Result<ExitStatus> {
    let run_dir = path::absolute(run_dir)?;
    let mut command = oracle_command(executable, &run_dir, seed)?;
    let stdout = File::create(run_dir.join(STDOUT_FILE))?;
    let stderr = File::create(run_dir.join(STDERR_FILE))?;
    let mut child = command.stdout(stdout).stderr(stderr).spawn()?;
    wait_for_exit(&mut child, timeout)
}

fn command(executable: &Path) -> io::Result<Command> {
    let executable = path::absolute(executable)?;
    let directory = executable.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "Wesnoth executable needs a parent directory")
    })?;
    let mut command = Command::new(&executable);
    command
        .current_dir(directory)
        .stdin(Stdio::null())
        .arg("--no-log-to-file");
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.arg("--wnoconsole").creation_flags(CREATE_NO_WINDOW);
    }
    Ok(command)
}

fn oracle_command(executable: &Path, run_dir: &Path, seed: u32) -> io::Result<Command> {
    let userdata = run_dir.join("userdata");
    let mut command = command(executable)?;
    command
        .args([
            "--unit=rustnoth_trace",
            "--noreplaycheck",
            "--nobanner",
            "--nosound",
            "--log-debug=random",
            "--log-info=wml",
        ])
        .arg(format!("--rng-seed={seed}"))
        .arg("--userdata-dir")
        .arg(&userdata)
        .arg("--userconfig-dir")
        .arg(userdata)
        .arg("--usercache-dir")
        .arg(run_dir.join("cache"));
    Ok(command)
}

fn wait_for_exit(child: &mut Child, timeout: Duration) -> io::Result<ExitStatus> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            if let Err(error) = child.kill() {
                return child.try_wait()?.ok_or(error);
            }
            child.wait()?;
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("Wesnoth audit timed out after {timeout:?}"),
            ));
        }
        thread::sleep(remaining.min(POLL_INTERVAL));
    }
}

#[cfg(test)]
mod tests {
    use std::{env, ffi::OsStr};

    use super::*;

    #[test]
    fn oracle_arguments_use_the_installed_data_and_isolated_profile() {
        let root = env::current_dir().unwrap();
        let installation = root.join("Wesnoth installation");
        let executable = installation.join("wesnoth");
        let run_dir = root.join("audit output");
        let userdata = run_dir.join("userdata");
        let cache = run_dir.join("cache");
        let command = oracle_command(&executable, &run_dir, 5489).unwrap();

        let mut expected: Vec<&OsStr> = vec![OsStr::new("--no-log-to-file")];
        #[cfg(windows)]
        expected.push(OsStr::new("--wnoconsole"));
        expected.extend([
            OsStr::new("--unit=rustnoth_trace"),
            OsStr::new("--noreplaycheck"),
            OsStr::new("--nobanner"),
            OsStr::new("--nosound"),
            OsStr::new("--log-debug=random"),
            OsStr::new("--log-info=wml"),
            OsStr::new("--rng-seed=5489"),
            OsStr::new("--userdata-dir"),
            userdata.as_os_str(),
            OsStr::new("--userconfig-dir"),
            userdata.as_os_str(),
            OsStr::new("--usercache-dir"),
            cache.as_os_str(),
        ]);
        assert_eq!(command.get_program(), executable.as_os_str());
        assert_eq!(command.get_current_dir(), Some(installation.as_path()));
        assert_eq!(command.get_args().collect::<Vec<_>>(), expected);
    }
}

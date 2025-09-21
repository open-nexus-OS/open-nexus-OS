// src/services/process_manager.rs
// Process spawning and management logic

use std::process::{Child, Command};
use std::io::{ErrorKind, Result as IoResult};
use log::{debug, error, info};

/// Convert exec string to Command with proper argument parsing
pub fn exec_to_command(exec: &str, path_opt: Option<&str>) -> Option<Command> {
    let args_vec: Vec<String> = shlex::split(exec)?;
    let mut args = args_vec.iter();
    let mut command = Command::new(args.next()?);
    
    for arg in args {
        if arg.starts_with('%') {
            match arg.as_str() {
                "%f" | "%F" | "%u" | "%U" => {
                    if let Some(path) = &path_opt { 
                        command.arg(path); 
                    }
                }
                _ => {
                    log::warn!("unsupported Exec code {:?} in {:?}", arg, exec);
                    return None;
                }
            }
        } else {
            command.arg(arg);
        }
    }
    Some(command)
}

/// Spawn a process from exec string
pub fn spawn_exec(exec: &str, path_opt: Option<&str>) {
    match exec_to_command(exec, path_opt) {
        Some(mut command) => {
            if let Err(err) = command.spawn() {
                error!("failed to launch {}: {}", exec, err);
            }
        }
        None => error!("failed to parse {}", exec),
    }
}

/// Spawn a process and return the child
pub fn spawn_exec_with_child(exec: &str, path_opt: Option<&str>) -> Result<Child, String> {
    match exec_to_command(exec, path_opt) {
        Some(mut command) => {
            match command.spawn() {
                Ok(child) => Ok(child),
                Err(err) => Err(format!("failed to spawn {}: {}", exec, err)),
            }
        }
        None => Err(format!("failed to parse {}", exec)),
    }
}

/// Wait for child processes (non-blocking)
#[cfg(not(target_os = "redox"))]
pub fn wait(status: &mut i32) -> IoResult<usize> {
    extern crate libc;
    use std::io::Error;
    let pid = unsafe { libc::waitpid(0, status as *mut i32, libc::WNOHANG) };
    if pid < 0 {
        Err(std::io::Error::new(ErrorKind::Other, format!("waitpid failed: {}", Error::last_os_error())))
    } else {
        Ok(pid as usize)
    }
}

#[cfg(target_os = "redox")]
pub fn wait(status: &mut i32) -> IoResult<usize> {
    use libredox::call;
    use libc;
    call::waitpid(0, status, libc::WNOHANG).map_err(|e| {
        std::io::Error::new(ErrorKind::Other, format!("Error in waitpid(): {}", e.to_string()))
    })
}

/// Reap all zombie processes
pub fn reap_all_zombies() {
    debug!("Reaping all zombie processes");
    let mut status = 0;
    while wait(&mut status).is_ok() {}
}

/// Kill all child processes and wait for them
pub fn cleanup_children(children: &mut Vec<(String, Child)>) {
    info!("Cleaning up {} child processes", children.len());
    for (exec, child) in children.iter_mut() {
        let pid = child.id();
        match child.kill() {
            Ok(()) => info!("Successfully killed child: {}", pid),
            Err(err) => error!("failed to kill {} ({}): {}", exec, pid, err),
        }
        match child.wait() {
            Ok(status) => info!("{} ({}) exited with {}", exec, pid, status),
            Err(err) => error!("failed to wait for {} ({}): {}", exec, pid, err),
        }
    }
    children.clear();
}

/// Reap finished child processes
pub fn reap_children(children: &mut Vec<(String, Child)>) {
    let mut i = 0;
    while i < children.len() {
        let remove = match children[i].1.try_wait() {
            Ok(None) => false,
            Ok(Some(status)) => {
                info!("{} ({}) exited with {}", 
                      children[i].0, 
                      children[i].1.id(), 
                      status);
                true
            }
            Err(err) => {
                error!("failed to wait for {} ({}): {}", 
                       children[i].0, 
                       children[i].1.id(), 
                       err);
                true
            }
        };
        if remove { 
            children.remove(i); 
        } else { 
            i += 1; 
        }
    }
}

/// Check if a process is running by exec string
pub fn is_child_running(children: &[(String, Child)], exec: &str) -> bool {
    children.iter().any(|(child_exec, _)| child_exec == exec)
}

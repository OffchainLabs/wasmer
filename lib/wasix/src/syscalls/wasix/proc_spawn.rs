use virtual_fs::Pipe;
use wasmer_wasix_types::wasi::ProcessHandles;

use super::*;
use crate::syscalls::*;

/// Spawns a new process within the context of this machine
///
/// ## Parameters
///
/// * `name` - Name of the process to be spawned
/// * `chroot` - Indicates if the process will chroot or not
/// * `args` - List of the arguments to pass the process
///   (entries are separated by line feeds)
/// * `preopen` - List of the preopens for this process
///   (entries are separated by line feeds)
/// * `stdin` - How will stdin be handled
/// * `stdout` - How will stdout be handled
/// * `stderr` - How will stderr be handled
/// * `working_dir` - Working directory where this process should run
///   (passing '.' will use the current directory)
///
/// ## Return
///
/// Returns a bus process id that can be used to invoke calls
#[instrument(level = "debug", skip_all, fields(name = field::Empty, working_dir = field::Empty), ret)]
pub fn proc_spawn<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: WasmPtr<u8, M>,
    name_len: M::Offset,
    chroot: Bool,
    args: WasmPtr<u8, M>,
    args_len: M::Offset,
    preopen: WasmPtr<u8, M>,
    preopen_len: M::Offset,
    stdin: WasiStdioMode,
    stdout: WasiStdioMode,
    stderr: WasiStdioMode,
    working_dir: WasmPtr<u8, M>,
    working_dir_len: M::Offset,
    ret_handles: WasmPtr<ProcessHandles, M>,
) -> Result<Errno, WasiError> {
    let env = ctx.data();
    let control_plane = &env.control_plane;
    let memory = unsafe { env.memory_view(&ctx) };
    let name = unsafe { get_input_str_ok!(&memory, name, name_len) };
    let args = unsafe { get_input_str_ok!(&memory, args, args_len) };
    let preopen = unsafe { get_input_str_ok!(&memory, preopen, preopen_len) };
    let working_dir = unsafe { get_input_str_ok!(&memory, working_dir, working_dir_len) };

    Span::current()
        .record("name", name.as_str())
        .record("working_dir", working_dir.as_str());

    if chroot == Bool::True {
        warn!("chroot is not currently supported",);
        return Ok(Errno::Notsup);
    }

    let args: Vec<_> = args
        .split(&['\n', '\r'])
        .map(|a| a.to_string())
        .filter(|a| !a.is_empty())
        .collect();

    let preopen: Vec<_> = preopen
        .split(&['\n', '\r'])
        .map(|a| a.to_string())
        .filter(|a| !a.is_empty())
        .collect();

    let (handles, ctx) = match proc_spawn_internal(
        ctx,
        name,
        Some(args),
        Some(preopen),
        Some(working_dir),
        stdin,
        stdout,
        stderr,
    )? {
        Ok(a) => a,
        Err(err) => {
            return Ok(err);
        }
    };

    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    wasi_try_mem_ok!(ret_handles.write(&memory, handles));
    Ok(Errno::Success)
}

pub fn proc_spawn_internal(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: String,
    args: Option<Vec<String>>,
    preopen: Option<Vec<String>>,
    working_dir: Option<String>,
    stdin: WasiStdioMode,
    stdout: WasiStdioMode,
    stderr: WasiStdioMode,
) -> Result<Result<(ProcessHandles, FunctionEnvMut<'_, WasiEnv>), Errno>, WasiError> {
    let env = ctx.data();

    // Build a new store that will be passed to the thread
    let new_store = ctx.data().runtime.new_store();

    // Fork the current environment and set the new arguments
    let (mut child_env, handle) = match ctx.data().fork() {
        Ok(x) => x,
        Err(err) => {
            // TODO: evaluate the appropriate error code, document it in the spec.
            return Ok(Err(Errno::Access));
        }
    };
    let child_process = child_env.process.clone();
    if let Some(args) = args {
        let mut child_state = env.state.fork();
        child_state.args = args;
        child_env.state = Arc::new(child_state);
    }

    // Take ownership of this child
    ctx.data_mut().owned_handles.push(handle);
    let env = ctx.data();

    // Preopen
    if let Some(preopen) = preopen {
        if !preopen.is_empty() {
            for preopen in preopen {
                warn!(
                    "preopens are not yet supported for spawned processes [{}]",
                    preopen
                );
            }
            return Ok(Err(Errno::Notsup));
        }
    }

    // Change the current directory
    if let Some(working_dir) = working_dir {
        child_env.state.fs.set_current_dir(working_dir.as_str());
    }

    // Replace the STDIO
    let (stdin, stdout, stderr) = {
        let (child_state, child_inodes) = child_env.get_wasi_state_and_inodes();
        let mut conv_stdio_mode = |mode: WasiStdioMode, fd: WasiFd| -> Result<OptionFd, Errno> {
            match mode {
                WasiStdioMode::Piped => {
                    let (pipe1, pipe2) = Pipe::channel();
                    let inode1 = child_state.fs.create_inode_with_default_stat(
                        child_inodes,
                        Kind::Pipe { pipe: pipe1 },
                        false,
                        "pipe".into(),
                    );
                    let inode2 = child_state.fs.create_inode_with_default_stat(
                        child_inodes,
                        Kind::Pipe { pipe: pipe2 },
                        false,
                        "pipe".into(),
                    );

                    let rights = crate::net::socket::all_socket_rights();
                    let pipe = ctx.data().state.fs.create_fd(
                        rights,
                        rights,
                        Fdflags::empty(),
                        0,
                        inode1,
                    )?;
                    child_state.fs.create_fd_ext(
                        rights,
                        rights,
                        Fdflags::empty(),
                        0,
                        inode2,
                        fd,
                    )?;

                    trace!("fd_pipe (fd1={}, fd2={})", pipe, fd);
                    Ok(OptionFd {
                        tag: OptionTag::Some,
                        fd: pipe,
                    })
                }
                WasiStdioMode::Inherit => Ok(OptionFd {
                    tag: OptionTag::None,
                    fd: u32::MAX,
                }),
                _ => {
                    child_state.fs.close_fd(fd);
                    Ok(OptionFd {
                        tag: OptionTag::None,
                        fd: u32::MAX,
                    })
                }
            }
        };
        let stdin = match conv_stdio_mode(stdin, 0) {
            Ok(a) => a,
            Err(err) => return Ok(Err(err)),
        };
        let stdout = match conv_stdio_mode(stdout, 1) {
            Ok(a) => a,
            Err(err) => return Ok(Err(err)),
        };
        let stderr = match conv_stdio_mode(stderr, 2) {
            Ok(a) => a,
            Err(err) => return Ok(Err(err)),
        };
        (stdin, stdout, stderr)
    };

    // Create the new process
    let bin_factory = Box::new(ctx.data().bin_factory.clone());
    let child_pid = child_env.pid();

    let mut new_store = Some(new_store);
    let mut builder = Some(child_env);

    // First we try the built in commands
    let mut process =
        match bin_factory.try_built_in(name.clone(), Some(&ctx), &mut new_store, &mut builder) {
            Ok(a) => a,
            Err(err) => {
                if !err.is_not_found() {
                    error!("builtin failed - {}", err);
                }
                // Now we actually spawn the process
                let child_work =
                    bin_factory.spawn(name, new_store.take().unwrap(), builder.take().unwrap());

                match __asyncify(&mut ctx, None, async move { Ok(child_work.await) })?
                    .map_err(|err| Errno::Unknown)
                {
                    Ok(Ok(a)) => a,
                    Ok(Err(err)) => return Ok(Err(conv_spawn_err_to_errno(&err))),
                    Err(err) => return Ok(Err(err)),
                }
            }
        };

    // Add the process to the environment state
    {
        let mut inner = ctx.data().process.inner.write().unwrap();
        inner.children.push(child_process);
    }
    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };

    let handles = ProcessHandles {
        pid: child_pid.raw(),
        stdin,
        stdout,
        stderr,
    };
    Ok(Ok((handles, ctx)))
}

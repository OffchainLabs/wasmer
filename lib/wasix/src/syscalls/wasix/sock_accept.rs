use std::task::Waker;

use super::*;
use crate::{net::socket::TimeType, syscalls::*};

/// ### `sock_accept()`
/// Accept a new incoming connection.
/// Note: This is similar to `accept` in POSIX.
///
/// ## Parameters
///
/// * `fd` - The listening socket.
/// * `flags` - The desired values of the file descriptor flags.
///
/// ## Return
///
/// New socket connection
#[instrument(level = "debug", skip_all, fields(%sock, fd = field::Empty), ret)]
pub fn sock_accept<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    fd_flags: Fdflags,
    ro_fd: WasmPtr<WasiFd, M>,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    let env = ctx.data();
    let (memory, state, _) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };

    let nonblocking = fd_flags.contains(Fdflags::NONBLOCK);

    let (fd, addr) = wasi_try_ok!(sock_accept_internal(env, sock, fd_flags, nonblocking));

    wasi_try_mem_ok!(ro_fd.write(&memory, fd));

    Ok(Errno::Success)
}

/// ### `sock_accept_v2()`
/// Accept a new incoming connection.
/// Note: This is similar to `accept` in POSIX.
///
/// ## Parameters
///
/// * `fd` - The listening socket.
/// * `flags` - The desired values of the file descriptor flags.
/// * `ro_addr` - Returns the address and port of the client
///
/// ## Return
///
/// New socket connection
#[instrument(level = "debug", skip_all, fields(%sock, fd = field::Empty), ret)]
pub fn sock_accept_v2<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    fd_flags: Fdflags,
    ro_fd: WasmPtr<WasiFd, M>,
    ro_addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    let env = ctx.data();
    let (memory, state, _) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };

    let nonblocking = fd_flags.contains(Fdflags::NONBLOCK);

    let (fd, addr) = wasi_try_ok!(sock_accept_internal(env, sock, fd_flags, nonblocking));

    wasi_try_mem_ok!(ro_fd.write(&memory, fd));
    wasi_try_ok!(crate::net::write_ip_port(
        &memory,
        ro_addr,
        addr.ip(),
        addr.port()
    ));

    Ok(Errno::Success)
}

pub fn sock_accept_internal(
    env: &WasiEnv,
    sock: WasiFd,
    mut fd_flags: Fdflags,
    mut nonblocking: bool,
) -> Result<(WasiFd, SocketAddr), Errno> {
    let state = env.state();
    let inodes = &state.inodes;

    let tasks = env.tasks().clone();
    let (child, addr, fd_flags) = __sock_asyncify(
        env,
        sock,
        Rights::SOCK_ACCEPT,
        move |socket, fd| async move {
            if fd.flags.contains(Fdflags::NONBLOCK) {
                fd_flags.set(Fdflags::NONBLOCK, true);
                nonblocking = true;
            }
            let timeout = socket
                .opt_time(TimeType::AcceptTimeout)
                .ok()
                .flatten()
                .unwrap_or(Duration::from_secs(30));
            socket
                .accept(tasks.deref(), nonblocking, Some(timeout))
                .await
                .map(|a| (a.0, a.1, fd_flags))
        },
    )?;

    let kind = Kind::Socket {
        socket: InodeSocket::new(InodeSocketKind::TcpStream {
            socket: child,
            write_timeout: None,
            read_timeout: None,
        })
        .map_err(net_error_into_wasi_err)?,
    };
    let inode = state
        .fs
        .create_inode_with_default_stat(inodes, kind, false, "socket".into());

    let mut new_flags = Fdflags::empty();
    if fd_flags.contains(Fdflags::NONBLOCK) {
        new_flags.set(Fdflags::NONBLOCK, true);
    }

    let mut new_flags = Fdflags::empty();
    if fd_flags.contains(Fdflags::NONBLOCK) {
        new_flags.set(Fdflags::NONBLOCK, true);
    }

    let rights = Rights::all_socket();
    let fd = state.fs.create_fd(rights, rights, new_flags, 0, inode)?;
    Span::current().record("fd", fd);

    Ok((fd, addr))
}

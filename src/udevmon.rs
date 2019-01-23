// flatkvm-agent
// Copyright (C) 2019  Sergio Lopez <slp@sinrega.org>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <http://www.gnu.org/licenses/>.

//
// Based on udev-rs monitor example
//

use std::io;
use std::process::Command;
use std::ptr;
use std::thread;
use std::time::Duration;

use std::os::unix::io::AsRawFd;

use libc::{c_int, c_short, c_ulong, c_void};
use shlex::split;

#[repr(C)]
struct pollfd {
    fd: c_int,
    events: c_short,
    revents: c_short,
}

#[repr(C)]
struct sigset_t {
    __private: c_void,
}

#[allow(non_camel_case_types)]
type nfds_t = c_ulong;

const POLLIN: c_short = 0x0001;

extern "C" {
    fn ppoll(
        fds: *mut pollfd,
        nfds: nfds_t,
        timeout_ts: *mut libc::timespec,
        sigmask: *const sigset_t,
    ) -> c_int;
}

pub fn monitor() -> io::Result<()> {
    let context = udev::Context::new()?;
    let monitor = udev::MonitorBuilder::new(&context)?;
    let mut socket = monitor.listen()?;
    let mut fds = vec![pollfd {
        fd: socket.as_raw_fd(),
        events: POLLIN,
        revents: 0,
    }];

    loop {
        let result = unsafe {
            ppoll(
                (&mut fds[..]).as_mut_ptr(),
                fds.len() as nfds_t,
                ptr::null_mut(),
                ptr::null(),
            )
        };

        if result < 0 {
            return Err(io::Error::last_os_error());
        }

        let event = match socket.next() {
            Some(evt) => evt,
            None => {
                thread::sleep(Duration::from_millis(10));
                continue;
            }
        };

        if event.sysname().to_str().unwrap_or("") == "card0" {
            let argsline = "--output Virtual-1 --auto";
            let args = split(&argsline).unwrap();

            let exit_status = Command::new("xrandr").args(args).status().unwrap();
            let exit_code = match exit_status.code() {
                Some(code) => code,
                None => -1,
            };
            println!("xrandr exit code: {}", exit_code);
        }

        println!(
            "{}: {} {} (subsystem={}, sysname={}, devtype={})",
            event.sequence_number(),
            event.event_type(),
            event.syspath().to_str().unwrap_or("---"),
            event.subsystem().map_or("", |s| s.to_str().unwrap_or("")),
            event.sysname().to_str().unwrap_or(""),
            event.devtype().map_or("", |s| s.to_str().unwrap_or(""))
        );
    }
}

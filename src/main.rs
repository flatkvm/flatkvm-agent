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

use std::fs::create_dir_all;
use std::fs::File;
use std::path::PathBuf;
use std::process::{exit, Child, Command};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::thread;

use clap::{crate_authors, crate_version, App, Arg};
use log::{debug, error, info};
use shlex::split;
use simplelog::{CombinedLogger, Config, LevelFilter, WriteLogger};
use x11_clipboard::{Clipboard, Source};

use flatkvm_qemu::agent::*;
use flatkvm_qemu::clipboard::*;
use flatkvm_qemu::runner::{QemuSharedDir, QemuSharedDirType};

mod dbus_listener;
mod message;
mod udevmon;

fn do_mount_request(agent: &mut AgentGuest, dir: QemuSharedDir) -> Result<(), String> {
    let target = match dir.dir_type {
        QemuSharedDirType::FlatpakSystemDir => "/var/lib/flatpak".to_string(),
        QemuSharedDirType::FlatpakUserDir => "/root/.local/share/flatpak".to_string(),
        QemuSharedDirType::FlatpakAppDir => format!("/root/.var/app/{}", dir.app_name),
    };

    create_dir_all(&target).map_err(|err| err.to_string())?;

    let argsline = format!(
        "-t 9p -o trans=virtio,version=9p2000.L {} {}",
        dir.tag, target
    );
    let args = match split(&argsline) {
        Some(args) => args,
        None => return Err("can't format arguments".to_string()),
    };

    let exit_status = Command::new("mount")
        .args(args)
        .status()
        .map_err(|err| err.to_string())?;

    let exit_code = match exit_status.code() {
        Some(code) => code,
        None => -1,
    };

    agent.send_ack(exit_code)?;
    Ok(())
}

fn do_run_request(
    agent: &mut AgentGuest,
    sender: Sender<message::Message>,
    rr: AgentRunRequest,
) -> Result<(), String> {
    let mut child = match spawn_app(rr) {
        Ok(child) => child,
        Err(err) => {
            agent.send_ack(-1)?;
            return Err(err.to_string());
        }
    };

    agent.send_ack(0)?;

    thread::spawn(move || {
        let exit_code = match child.wait() {
            Ok(exit_status) => match exit_status.code() {
                Some(code) => code,
                None => -1,
            },
            Err(_) => -1,
        };
        sender.send(message::Message::AppExit(exit_code)).unwrap();
    });

    Ok(())
}

fn spawn_app(rr: AgentRunRequest) -> Result<Child, String> {
    let mut args = vec!["run"];

    if rr.user {
        args.push("--user");
    }

    // It's safe to expose the session-bus here as it's the one from the VM.
    // Notifications to the Host are filtered and relayed by ourselves.
    if rr.dbus_session {
        args.push("--socket=session-bus");
    }

    // Don't share HOME, as it's volatile. This actually increases the changes
    // that app's data gets preserved, as it should be stored on the flatpak
    // app directory.
    args.push("--nofilesystem=home");
    args.push(&rr.app);

    debug!("running app with args: {:?}", args);
    let proc = Command::new("flatpak")
        .args(args)
        .env("DISPLAY", ":0")
        .spawn()
        .map_err(|err| err.to_string())?;

    Ok(proc)
}

struct HostListener {
    agent: AgentGuest,
    sender: Sender<message::Message>,
}

impl HostListener {
    pub fn new(agent: AgentGuest, sender: Sender<message::Message>) -> HostListener {
        HostListener { agent, sender }
    }

    pub fn get_and_process_event(&mut self) -> Result<(), String> {
        let event = self.agent.get_event()?;

        match event {
            AgentMessage::AgentMountRequest(mr) => {
                debug!("Agentmessage::Message::AgentMountRequest");
                self.sender
                    .send(message::Message::MountRequest(mr.shared_dir))
                    .unwrap();
            }
            AgentMessage::AgentRunRequest(rr) => {
                debug!("AgentRunRequest");
                self.sender.send(message::Message::RunRequest(rr)).unwrap();
            }
            AgentMessage::ClipboardEvent(ce) => {
                debug!("AgentClipboardEvent");
                self.sender
                    .send(message::Message::RemoteClipboardEvent(ce))
                    .unwrap();
            }
            _ => return Err("Protocol error".to_string()),
        }

        Ok(())
    }
}

fn main() {
    CombinedLogger::init(vec![WriteLogger::new(
        LevelFilter::Debug,
        Config::default(),
        File::create("/var/log/flatkvm-agent.log").unwrap(),
    )])
    .unwrap();

    let cmd_args = App::new("flatkvm-agent")
        .version(crate_version!())
        .author(crate_authors!())
        .about("FlatKvm Agent")
        .arg(
            Arg::with_name("vsock")
                .short("v")
                .long("vsock")
                .help("vsock port")
                .takes_value(true)
                .required(true),
        )
        .get_matches();

    let vsock_path = cmd_args
        .value_of("vsock")
        .map(|s| PathBuf::from(s))
        .unwrap();

    let mut agent = match AgentGuest::new(vsock_path) {
        Ok(agent) => agent,
        Err(err) => {
            error!("error creating agent: {}", err.to_string());
            exit(-1);
        }
    };

    let mut agent_writer = agent.try_clone().unwrap();

    info!("Doing handshake");
    match agent.do_handshake(crate_version!()) {
        Ok(_) => (),
        Err(err) => {
            error!("error in handshake with agent: {}", err.to_string());
            exit(-1);
        }
    };
    info!("Handshake done");

    let (common_sender, common_receiver) = channel();
    let (clipboard_sender, clipboard_receiver) = channel();

    // Spawn a thread to listen for clipboard events.
    let cb_used_flag = Arc::new(AtomicBool::new(false));
    ClipboardListener::new(clipboard_sender.clone(), cb_used_flag.clone()).spawn_thread();

    // Translate clipboard messages into our own kind.
    let sender = common_sender.clone();
    thread::spawn(move || loop {
        for msg in &clipboard_receiver {
            match msg {
                ClipboardMessage::ClipboardEvent(ce) => {
                    sender
                        .send(message::Message::LocalClipboardEvent(ce))
                        .unwrap();
                }
            }
        }
    });

    // Spawn a thread to listen for udev events.
    // We use this to detect video resolution changes.
    thread::spawn(move || loop {
        match udevmon::monitor() {
            Ok(()) => (),
            Err(err) => debug!("udev error: {}", err.to_string()),
        }
    });

    // Spawn a thread waiting for messages coming from the Host.
    let mut host_listener = HostListener::new(agent, common_sender.clone());
    thread::spawn(move || loop {
        info!("Waiting for events from Host");
        match host_listener.get_and_process_event() {
            Ok(_) => (),
            Err(err) => {
                error!("error processing host events: {}", err.to_string());
                exit(-1);
            }
        }
    });

    let dbus_sender = common_sender.clone();
    thread::spawn(move || {
        dbus_listener::handle_dbus_notifications(dbus_sender);
    });

    // Create another clipboard instance to store values.
    let clipboard = Clipboard::new(Source::Clipboard).unwrap();

    // Process events coming from spawned threads.
    for msg in common_receiver {
        match msg {
            message::Message::LocalClipboardEvent(ce) => {
                debug!("Clipboard event: {}", ce.data);
                agent_writer.send_clipboard_event(ce).unwrap();
            }
            message::Message::RemoteClipboardEvent(ce) => {
                debug!("RemoteClipboard: {}", ce.data);
                cb_used_flag.store(true, Ordering::Relaxed);
                match clipboard.store(
                    clipboard.setter.atoms.clipboard,
                    clipboard.setter.atoms.utf8_string,
                    ce.data.as_bytes(),
                ) {
                    Ok(_) => (),
                    Err(err) => {
                        error!("can't store value in clipboard: {}", err.to_string());
                        exit(-1);
                    }
                }
            }
            message::Message::DbusNotification(dn) => {
                debug!("DbusNotification");
                agent_writer.send_dbus_notification(dn).unwrap();
            }
            message::Message::AppExit(ec) => {
                debug!("AppExit");
                match agent_writer.send_exit_code(ec) {
                    Ok(_) => (),
                    Err(err) => {
                        error!("can't send exit code: {}", err.to_string());
                        exit(-1);
                    }
                }
                break;
            }
            message::Message::MountRequest(dir) => {
                debug!("MountRequest");
                match do_mount_request(&mut agent_writer, dir) {
                    Ok(_) => (),
                    Err(err) => {
                        error!("error servicing mount request: {}", err.to_string());
                        exit(-1);
                    }
                }
            }
            message::Message::RunRequest(rr) => {
                debug!("RunRequest");
                match do_run_request(&mut agent_writer, common_sender.clone(), rr) {
                    Ok(_) => (),
                    Err(err) => {
                        error!("error sevicing run request: {}", err.to_string());
                        exit(-1);
                    }
                }
            }
        }
    }

    debug!("Ending!");
}

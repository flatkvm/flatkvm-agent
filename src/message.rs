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

use flatkvm_qemu::agent::AgentRunRequest;
use flatkvm_qemu::clipboard::ClipboardEvent;
use flatkvm_qemu::dbus_notifications::{DbusNotification, DbusNotificationClosed};
use flatkvm_qemu::runner::QemuSharedDir;

pub enum Message {
    LocalClipboardEvent(ClipboardEvent),
    RemoteClipboardEvent(ClipboardEvent),
    DbusNotification(DbusNotification),
    DbusNotificationClosed(DbusNotificationClosed),
    MountRequest(QemuSharedDir),
    RunRequest(AgentRunRequest),
    AppExit(i32),
}

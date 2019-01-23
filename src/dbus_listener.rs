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

use crate::message::Message;
use dbus::arg::{RefArg, Variant};
use dbus::tree;
use dbus::{BusType, Connection};
use flatkvm_qemu::dbus_codegen::*;
use flatkvm_qemu::dbus_notifications::DbusNotification;
use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::sync::Mutex;

static mut DBUS_SENDER: Option<Mutex<Sender<Message>>> = None;

#[derive(Copy, Clone, Default, Debug)]
struct TData;
impl tree::DataType for TData {
    type Tree = ();
    type ObjectPath = Arc<Notification>;
    type Property = ();
    type Interface = ();
    type Method = ();
    type Signal = ();
}

#[derive(Copy, Clone, Default, Debug)]
struct Notification;
impl OrgFreedesktopNotifications for Notification {
    type Err = dbus::tree::MethodErr;
    fn close_notification(&self, _id: u32) -> Result<(), Self::Err> {
        Ok(())
    }

    fn get_capabilities(&self) -> Result<Vec<String>, Self::Err> {
        let capabilities: Vec<String> = vec![
            "actions".to_string(),
            "body".to_string(),
            "body-hyperlinks".to_string(),
            "body-markup".to_string(),
            "icon-static".to_string(),
            "sound".to_string(),
            "persistence".to_string(),
            "action-icons".to_string(),
        ];

        Ok(capabilities)
    }

    fn get_server_information(&self) -> Result<(String, String, String, String), Self::Err> {
        Ok((
            "dummy".to_string(),
            "dummy".to_string(),
            "dummy".to_string(),
            "dummy".to_string(),
        ))
    }

    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        _actions: Vec<&str>,
        _hints: HashMap<&str, Variant<Box<RefArg>>>,
        expire_timeout: i32,
    ) -> Result<u32, Self::Err> {
        println!(
            "notification: app_name={}, replaces_id={}, app_icon={}, summary={}, body={}",
            app_name, replaces_id, app_icon, summary, body
        );

        // Safe because the Option is only changed in handle_dbus_notifications,
        // and the Sender is protected by a Mutex.
        unsafe {
            if let Some(sender_mutex) = &DBUS_SENDER {
                let sender = sender_mutex.lock().unwrap();
                sender
                    .send(Message::DbusNotification(DbusNotification {
                        summary: summary.to_string(),
                        body: body.to_string(),
                        expire_timeout,
                    }))
                    .unwrap();
            }
        }

        Ok(0)
    }
}

fn dbus_create_iface() -> tree::Interface<tree::MTFn<TData>, TData> {
    let f = tree::Factory::new_fn();
    org_freedesktop_notifications_server(&f, (), |m| {
        let a: &Arc<Notification> = m.path.get_data();
        let b: &Notification = &a;
        b
    })
}

pub fn handle_dbus_notifications(sender: Sender<Message>) {
    unsafe {
        DBUS_SENDER = Some(Mutex::new(sender));
    }

    let notification = Notification;

    let f = tree::Factory::new_fn();
    let iface = dbus_create_iface();

    let mut tree = f.tree(());
    tree = tree.add(
        f.object_path("/org/freedesktop/Notifications", Arc::new(notification))
            .introspectable()
            .add(iface),
    );

    let c = Connection::get_private(BusType::Session).unwrap();
    c.register_name("org.freedesktop.Notifications", 0).unwrap();
    tree.set_registered(&c, true).unwrap();

    c.add_handler(tree);
    loop {
        c.iter(-1).next();
    }
}

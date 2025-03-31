use std::{
    collections::HashSet,
    fs,
    ops::Deref,
    sync::{Arc, RwLock},
};

use clap::{Parser, ValueEnum};

use config_manager::CONFIG_MANAGER;
use key::{Key, ModifierKey};
use rdev::{grab, Event, EventType};
use serde::Serialize;
use utils::get_user;

mod config_manager;
mod key;
mod utils;

async fn hotkeyd() {
    let _ = CONFIG_MANAGER.deref();
    let held_modifiers: Arc<RwLock<HashSet<ModifierKey>>> = Arc::new(RwLock::new(HashSet::new()));
    grab(move |event: Event| -> Option<Event> {
        match event.event_type {
            EventType::KeyPress(key) => match Key::new_from_rdev(key) {
                Key::Modifier(modifier_key) => {
                    held_modifiers
                        .write()
                        .expect("held_modifiers was poisoned")
                        .insert(modifier_key);
                }
                Key::Keyboard(key) => {
                    if CONFIG_MANAGER
                        .exec(&*held_modifiers.read().expect("poisoned"), &key)
                        .is_none()
                    {
                        return Some(event);
                    } else {
                        return None;
                    }
                }
            },
            EventType::KeyRelease(key) => match Key::new_from_rdev(key) {
                Key::Modifier(modifier_key) => {
                    held_modifiers
                        .write()
                        .expect("held_modifiers was poisoned")
                        .remove(&modifier_key);
                }
                Key::Keyboard(_) => {}
            },
            _ => {}
        }
        return Some(event);
    })
    .expect("fuck");
}

#[derive(ValueEnum, Clone, Debug, Serialize)]
pub enum Command {
    #[serde(rename = "install")]
    Install,

    #[serde(rename = "daemon")]
    Daemon,
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[clap(value_enum)]
    pub command: Command,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    match args.command {
        Command::Daemon => hotkeyd().await,
        Command::Install => {
            let user = get_user().expect("couldn't get user");
            let path = format!("/Users/{}/Library/LaunchAgents/hotkeyd.plist", user);
            fs::write(
                path,
                format!("
                    <?xml version=\"1.0\" encoding=\"UTF-8\"?>
                    <!DOCTYPE plist PUBLIC \"-//Apple Computer//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">
                    <plist version=\"1.0\">
                    <dict>
                        <key>Label</key>
                        <string>hotkeyd</string>
                        <key>ProgramArguments</key>
                        <array>
                            <string>{}</string>
                            <string>daemon</string>
                        </array>
                        <key>RunAtLoad</key>
                        <true/>
                            <key>KeepAlive</key>
                        <dict>
                            <key>SuccessfulExit</key>
                           <false/>
                           <key>Crashed</key>
                           <true/>
                        </dict>
                        <key>EnvironmentVariables</key>
                        <dict>
                          <key>HOTKEYD_CONFIG</key>
                          <string>/Users/{}/.config/hotkeyd/hotkeyd.toml</string>
                        </dict>
                        <key>ProcessType</key>
                        <string>Interactive</string>
                        <key>Nice</key>
                        <integer>-20</integer>
                    </dict>
                    </plist>
                ", std::env::current_exe().unwrap().to_str().unwrap(), user),
            )
            .expect("failed to write to LaunchAgents");
        }
    }
}

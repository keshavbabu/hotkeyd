use std::{
    collections::{BTreeSet, HashMap, HashSet},
    env::var,
    fs::read_to_string,
    path::Path,
    process::Command,
    sync::{Arc, RwLock},
    time::Duration,
};

use lazy_static::lazy_static;
use notify::{FsEventWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use toml::{map::Map, Table, Value};

use crate::{
    key::{KeyboardKey, ModifierKey},
    utils::get_user,
};

#[derive(Debug, Clone)]
enum Action {
    Cmd { command: String },
}

impl Action {
    pub fn execute(&self) {
        match self {
            Action::Cmd { command } => {
                let mut envs = HashMap::new();

                // we want to set the USER env var to the currently logged in user
                if let Some(user) = get_user() {
                    envs.insert("USER", user);
                }

                // im fairly sure its good practice to check $SHELL instead
                // of blindly using sh but SHELL isnt accessible for some
                // reason when using `var("SHELL")`
                let b = Command::new("sh").args(["-c", command]).envs(envs).output();
                println!("output: {:?}", b);
            }
        }
    }

    fn new_from_config_map(config_map: &Map<String, Value>) -> Option<Self> {
        let type_val = match config_map.get("type") {
            Some(t) => t,
            None => {
                eprintln!("error parsing config: action does not have a property `type`");
                return None;
            }
        };

        let type_str = match type_val {
            Value::String(s) => s,
            _ => return None,
        };

        let action = match type_str.as_str() {
            "cmd" => {
                let command_val = match config_map.get("command") {
                    Some(cmd) => cmd,
                    None => return None,
                };

                let command_str = match command_val {
                    Value::String(s) => s,
                    _ => return None,
                };

                Action::Cmd {
                    command: command_str.to_string(),
                }
            }
            _ => return None,
        };

        Some(action)
    }
}

#[derive(Debug, Eq, PartialEq, Hash, Clone)]
struct Bind {
    modifiers: BTreeSet<ModifierKey>,
    key: KeyboardKey,
}

#[derive(Debug)]
struct Config {
    macros: HashMap<Bind, Action>,
}

impl Config {
    pub fn new() -> Self {
        let macros = HashMap::new();
        Config { macros }
    }

    fn new_from_file(path: String) -> Self {
        // parse file here
        //
        let content = match read_to_string(path) {
            Ok(content) => content,
            Err(error) => {
                eprintln!("error reading config file: {}", error);
                return Config::new();
            }
        };

        //parse that shit
        let parsed_config = match content.parse::<Table>() {
            Ok(config) => config,
            Err(error) => {
                eprintln!("error parsing config: {}", error);
                return Config::new();
            }
        };

        // check if [binds] was defined in the config
        let binds_content = match parsed_config.get("binds") {
            Some(binds_content) => binds_content,
            None => {
                eprintln!("error parsing config: [binds] does not exist in config file");
                // maybe not exit here? maybe its better to just assume no binds were set instead
                // of failing with an error.
                return Config::new();
            }
        };

        // check that [binds] is a table
        let binds_table = match binds_content {
            toml::Value::Table(table) => table,
            _ => {
                eprintln!("error parsing config: [binds] exists but it is not a table");
                return Config::new();
            }
        };

        let mut macros = HashMap::new();

        // parse all the binds
        for (keys, value) in binds_table.clone() {
            let mut modifiers: BTreeSet<ModifierKey> = BTreeSet::new();
            let mut keyboard_key: Option<KeyboardKey> = None;
            for key in keys.split(" + ").collect::<Vec<&str>>() {
                let modifier_key_conv = ModifierKey::from_config_kebab(key);
                let keyboard_key_conv = KeyboardKey::from_config_kebab(key);
                match (modifier_key_conv, keyboard_key_conv) {
                    (Some(key), None) => {
                        let ins = modifiers.insert(key);
                        if !ins {
                            panic!(
                                "multiple instances of the same modifier in macro: {:?}",
                                key
                            );
                        }
                    }
                    (None, Some(key)) => {
                        if keyboard_key != None {
                            panic!("multiple keyboard keys in macro: {:?}", key);
                        }
                        keyboard_key = Some(key);
                    }
                    _ => panic!("key is both modifier and keyboard key: {}", key),
                }
            }

            if keyboard_key.is_none() {}

            if modifiers.len() == 0 {
                panic!("no modifier keys in macro: {:?}", keys);
            }

            let bind = {
                let some_keyboard_key = match keyboard_key {
                    Some(k_key) => k_key,
                    None => panic!("no keyboard key in macro: {:?}", keys),
                };

                Bind {
                    modifiers,
                    key: some_keyboard_key,
                }
            };

            let value_table = match value {
                Value::Table(t) => t,
                _ => panic!("value of bind is not a table: {:?}", value),
            };

            let action = match Action::new_from_config_map(&value_table) {
                Some(action) => action,
                None => {
                    eprintln!("error parsing config: invalid action");
                    return Config::new();
                }
            };

            if macros.insert(bind.clone(), action).is_some() {
                panic!("bind already exists: {:?}", bind);
            }
        }

        return Config { macros };
    }
}

pub struct ConfigManager {
    config: Arc<RwLock<Config>>,

    _fs_watcher_handle: Debouncer<FsEventWatcher>,
}

impl ConfigManager {
    fn new(config_file_path: String) -> Self {
        let config = Arc::new(RwLock::new(Config::new_from_file(config_file_path.clone())));
        let cfg = config.clone();
        let fp = config_file_path.clone();
        let mut _fs_watcher_handle = new_debouncer(
            Duration::from_secs(1),
            move |events: DebounceEventResult| {
                let events = match events {
                    Ok(e) => e,
                    Err(err) => {
                        println!("error reading file: {}", err);
                        return;
                    }
                };

                for event in events {
                    match event.kind {
                        notify_debouncer_mini::DebouncedEventKind::Any => {
                            *config.write().expect("poisoned") = Config::new_from_file(fp.clone());
                            return;
                        }
                        _ => {}
                    }
                }
            },
        )
        .expect("config");

        if let Err(err) = _fs_watcher_handle
            .watcher()
            .watch(Path::new(&config_file_path), RecursiveMode::Recursive)
        {
            println!("watcher failed: {}", err);
        }
        Self {
            config: cfg,
            _fs_watcher_handle,
        }
    }

    pub fn exec(&self, modifiers: &HashSet<ModifierKey>, key: &KeyboardKey) -> Option<()> {
        let config = self.config.read().expect("poisonsed");
        let b = Bind {
            modifiers: modifiers.into_iter().map(|v| *v).collect(),
            key: *key,
        };
        let Some(action) = config.macros.get(&b) else {
            return None;
        };

        Some(action.execute())
    }
}

lazy_static! {
    pub static ref CONFIG_MANAGER: ConfigManager =
        ConfigManager::new(var("HOTKEYD_CONFIG").expect("no config").to_string());
}

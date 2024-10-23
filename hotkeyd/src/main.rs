use std::{collections::{BTreeSet, HashMap}, env::var, fs::read_to_string, path::Path, sync::{mpsc::{sync_channel, Receiver, SyncSender}, Arc, RwLock}, thread::{sleep, spawn, JoinHandle}, time::Duration};

use key::Key;
use notify::FsEventWatcher;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, DebouncedEvent, DebouncedEventKind, Debouncer};
use rdev::{grab, Event, GrabError};
use toml::{map::Map, Table, Value};

mod key;

#[derive(Debug)]
struct ConfigState {
    binds: HashMap<BTreeSet<Key>, Action>
}

impl ConfigState {
    fn new() -> Self {
        return ConfigState {
            binds: HashMap::new()
        }
    }

    fn new_from_file(path: String) -> Self {
        // parse file here
        //
        let content = match read_to_string(path) {
            Ok(content) => content,
            Err(error) => {
                eprintln!("error reading config file: {}", error);
                return ConfigState::new()
            }
        };

        //parse that shit
        let parsed_config = match content.parse::<Table>() {
            Ok(config) => config,
            Err(error) => {
                eprintln!("error parsing config: {}", error);
                return ConfigState::new();
            }
        };

        // check if [binds] was defined in the config
        let binds_content = match parsed_config.get("binds") {
            Some(binds_content) => binds_content,
            None => {
                eprintln!("error parsing config: [binds] does not exist in config file");
                // maybe not exit here? maybe its better to just assume no binds were set instead
                // of failing with an error.
                return ConfigState::new();
            }
        };

        // check that [binds] is a table
        let binds_table = match binds_content {
            toml::Value::Table(table) => table,
            _ => {
                eprintln!("error parsing config: [binds] exists but it is not a table");
                return ConfigState::new();
            }
        };

        let mut binds = HashMap::new();

        // parse all the binds
        for (key, value) in binds_table.clone() {

            // some sweet sweet functional chaining
            let keys: BTreeSet<Key> = key
                .split(" + ")
                .filter_map(|key| {
                    let key_result = Key::from_config_kebab(key);
                    if key_result.is_none() {
                        eprintln!("error parsing config: {} is not a valid key", key);
                        // maybe fail here?
                    }
                    key_result
                })
                .collect();

            if keys.len() == 0 {
                eprintln!("error parsing config: no keys in bind");
                return ConfigState::new();
            }

            // check that the value is a table
            let value_table = match value {
                toml::Value::Table(value_table) => value_table,
                _ => {
                    eprintln!("error parsing config: value is not a table");
                    return ConfigState::new();
                }
            };

            let action = match Action::new_from_config_map(&value_table) {
                Some(action) => action,
                None => {
                    eprintln!("error parsing config: invalid action");
                    return ConfigState::new();
                }
            };

            binds.insert(keys, action);
        }

        return ConfigState {
            binds
        }
    }
}

#[derive(Debug)]
struct ConfigFileWatcher {
    fs_watcher: Debouncer<FsEventWatcher>,

    config_reloader_handler: JoinHandle<()>
}

#[derive(Debug)]
struct Config {
    state: Arc<RwLock<ConfigState>>,

    config_file_watcher: Option<ConfigFileWatcher>
}

impl Config {
    fn new() -> Self {
        // we are going to start a watcher here that basically just listens for changes to the
        // config file 

        let config_file_path = match var("HOTKEYD_CONFIG") {
            Ok(path) => Some(path),
            Err(err) => {
                eprintln!("error getting config path: {}, maybe HOTKEYD_CONFIG was not set?", err);
                None
            }
        };

        let config_res = match config_file_path.clone() {
            Some(config_fp) => {
                // channel to notify to relaod the config
                let (tx, rx) = sync_channel::<String>(0);
                let cloned_config_fp = config_fp.clone();
                let w = new_debouncer(Duration::from_secs(1), move |res: DebounceEventResult| {
                    let event: DebouncedEvent = match res {
                        Ok(e) => {
                            if let Some(ev) = e.last() {
                                ev.clone()
                            } else {
                                return
                            }
                        },
                        Err(err) => {
                            eprintln!("error watching file: {}", err);

                            return
                        }
                    };

                    match event.kind {
                        DebouncedEventKind::Any => {
                            let send_result = tx.send(cloned_config_fp.clone());
                            match send_result {
                                Ok(_) => {},
                                Err(err) => eprintln!("error sending notification to reload config: {}", err)
                            }
                        },
                        _ => {}
                    }
                });

                match w {
                    Ok(mut wat) => {
                        let res = wat.watcher().watch(Path::new(&config_fp), notify::RecursiveMode::NonRecursive);

                        match res {
                            Ok(_) => Some((rx, wat)),
                            Err(err) => {
                                eprintln!("error starting watcher: {}", err);

                                None
                            }
                        }
                    },
                    Err(error) => {
                        eprintln!("unable to start config file watcher: {}", error);

                        None
                    }
                }
            },
            None => None
        };

        let config_state_lock = match config_file_path.clone() {
            Some(file_path) => {
                Arc::new(RwLock::new(ConfigState::new_from_file(file_path)))
            },
            None => {
                Arc::new(RwLock::new(ConfigState::new()))
            }
        };

        let config_file_watcher = match config_res {
            Some((rx, watcher)) => {
                let cs_l = Arc::clone(&config_state_lock);
                let handler = spawn(move || {
                    loop {
                        match rx.recv() {
                            Ok(config_fp) => {
                                println!("reloading config... ");

                                match cs_l.write() {
                                    Ok(mut c) => {
                                        *c = ConfigState::new_from_file(config_fp)
                                    },
                                    Err(err) => {
                                        eprintln!("error config state lock: {}", err);
                                    }
                                }
                            }
                            Err(err) => eprintln!("error receiving config reload notification: {}", err)
                        }
                    }
                });

                Some(ConfigFileWatcher {
                    fs_watcher: watcher,
                    config_reloader_handler: handler
                })
            },
            None => None
        }; 

        return Config { 
            state: config_state_lock,
            config_file_watcher
        }
    }

    fn action_for_state(&mut self, key_state: KeyState) -> Vec<Action> {
        let state = match self.state.read() {
            Ok(state) => state,
            Err(err) => {
                eprintln!("error getting lock for config state: {}", err);

                return vec![];
            }
        };
        let mut actions: Vec<Action> = vec![];
        // check key_binds
        if let Some(action) = state.binds.get(&key_state.keys_down) { 
            actions.push(action.clone());
        }

        actions
    }
}

#[derive(Debug, Clone)]
enum Action {
    Cmd { command: String },
    Macro { r#macro: Vec<BTreeSet<Key>> },
    ScrollModifier { x_mul: i64, y_mul: i64 },
    MouseModifier { x_mul: f64, y_mul: f64 }
}

impl Action {
    pub fn execute(&self) {
        match self {
            Action::Cmd { command } => println!("command: {}", command),
            Action::Macro { r#macro } => println!("macro: {:?}", r#macro),
            Action::MouseModifier { x_mul, y_mul } => println!("mouse-modifier: x_mul: {}, y_mul: {}", x_mul, y_mul),
            Action::ScrollModifier { x_mul, y_mul } => println!("scroll-modifier: x_mul: {}, y_mul: {}", x_mul, y_mul)
        }
    }
}

impl Action {
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
            _ => return None
        };

        let action = match type_str.as_str() {
            "cmd" => {
                let command_val = match config_map.get("command") {
                    Some(cmd) => cmd,
                    None => return None
                };

                let command_str = match command_val {
                    Value::String(s) => s,
                    _ => return None
                };

                Action::Cmd { command: command_str.to_string() }
            },
            "macro" => {
                let marco_val = match config_map.get("macro") {
                    Some(mv) => mv,
                    None => return None
                };

                let macro_vec = match marco_val {
                    Value::Array(v) => v,
                    _ => return None
                };

                let mut macro_keys = vec![];

                for macro_val in macro_vec {
                    let macro_str = match macro_val {
                        Value::String(s) => s,
                        _ => return None
                    };

                    let keys: BTreeSet<Key> = macro_str
                        .split(" + ")
                        .filter_map(|key| {
                            let key_result = Key::from_config_kebab(key);
                            if key_result.is_none() {
                                eprintln!("error parsing config: {} is not a valid key", key);
                                // maybe fail here?
                            }
                            key_result
                        })
                        .collect();

                    if keys.len() == 0 {
                        eprintln!("error parsing config: no keys in bind");
                        return None;
                    }

                    macro_keys.push(keys);
                }

                Action::Macro { r#macro: macro_keys }
            }
            _ => return None
        };

        Some(action)
    }
}

#[derive(Debug, Clone)]
struct MousePosition {
    x: f64,
    y: f64
}

#[derive(Debug, Clone)]
struct WheelScroll {
    delta_x: i64,
    delta_y: i64
}

#[derive(Debug, Clone)]
struct KeyState {
    keys_down: BTreeSet<Key>,
    last_mouse_position: Option<MousePosition>,
    scrolling_speed: WheelScroll
}

impl KeyState {
    fn new() -> Self {
        // this could cause issues since we are assuming that nothing is pressed when the program
        // is started. 
        //
        // if possible we should capture the current state and init with that.
        return KeyState {
            keys_down: BTreeSet::new(),
            last_mouse_position: None,
            scrolling_speed: WheelScroll { delta_x: 0, delta_y: 0 }
        }
    }

    fn key_down(&mut self, key: Key) {
        self.keys_down.insert(key);
    }

    fn key_up(&mut self, key: Key) {
        self.keys_down.remove(&key);
    }

    fn scroll_speed(&mut self, delta_x: i64, delta_y: i64) {
        self.scrolling_speed = WheelScroll { delta_x, delta_y };
    }

    fn mouse_position(&mut self, x: f64, y: f64) {
        self.last_mouse_position = Some(MousePosition { x, y });
    }
}

#[derive(Debug)]
struct KeyListener {
    state: KeyState,

    config: Config
}

#[derive(Debug)]
enum KeyListenerError {
    InvalidPermissions,
    Unknown(GrabError)
}

#[derive(Debug)]
enum KeyListenerEvent {
    Event(rdev::Event),
    Error(KeyListenerError),
    Exit
}

impl KeyListener {
    fn new() -> Self {
        return KeyListener{
            state: KeyState::new(),
            config: Config::new()
        }
    }

    fn handle_event(&mut self, event: Event) -> Option<Event> {
        // here we need to check the config to see if we need to handle anything
        let actions = self.config.action_for_state(self.state.clone());
        
        // run actions
        for action in &actions {
            action.execute();
        }

        // block original event if there were actions 
        // (also we may allow this to be an option in the config in the future)
        if actions.len() == 0 {
            Some(event)
        } else {
            None
        }
    }

    // blocking
    fn start(&mut self) {
        let (event_sender, event_receiver): (SyncSender<KeyListenerEvent>, Receiver<KeyListenerEvent>) = sync_channel(0);

        let (result_sender, result_receiver): (SyncSender<Option<Event>>, Receiver<Option<Event>>) = sync_channel(0);
        let handle = spawn(move || {
            let sender = event_sender.clone();
            let res = grab(move |event: Event| -> Option<Event> { 
                // handle extra shit here
                let _ = sender.send(KeyListenerEvent::Event(event.clone()));

                result_receiver.recv().expect("failed to receive the response")
            }); 
            let event = match res {
                Err(e) => KeyListenerEvent::Error(match e {
                    rdev::GrabError::EventTapError => KeyListenerError::InvalidPermissions,
                    _ => KeyListenerError::Unknown(e)
                }),
                Ok(_) => KeyListenerEvent::Exit,
            };
            let _ = event_sender.send(event);
        });

        loop {
            match event_receiver.recv() {
                Ok(r) => {
                    match r {
                        KeyListenerEvent::Event(event) => {
                            match event.event_type {
                                rdev::EventType::KeyPress(key) => {
                                    self.state.key_down(Key::new_from_rdev_key(key));
                                },
                                rdev::EventType::KeyRelease(key) => {
                                    self.state.key_up(Key::new_from_rdev_key(key));
                                },
                                rdev::EventType::Wheel { delta_x, delta_y } => {
                                    self.state.scroll_speed(delta_x, delta_y);
                                },
                                rdev::EventType::MouseMove { x, y } => {
                                    self.state.mouse_position(x, y);
                                },
                                rdev::EventType::ButtonPress(button) => {
                                    self.state.key_down(Key::new_from_rdev_button(button));
                                },
                                rdev::EventType::ButtonRelease(button) => {
                                    self.state.key_up(Key::new_from_rdev_button(button));
                                },
                            }
                            let _ = result_sender.send(self.handle_event(event.clone()));
                        },
                        KeyListenerEvent::Error(error) => match error {
                            KeyListenerError::InvalidPermissions => eprintln!("Error: invalid permissions (maybe we don't have access to the Accessibility API)"),
                            KeyListenerError::Unknown(e) => eprintln!("Error: unknown ({:?})", e)
                        },
                        KeyListenerEvent::Exit => println!("exiting...")
                    }
                },
                Err(e) => {
                    eprintln!("channel receive error: {:?}", e);
                    break
                }
            }
        }

        let _ = handle.join();
    }
}

fn main() {
    println!("start daemon");

    let mut key_listener = KeyListener::new();
    key_listener.start();
    println!("keeping daemon alive so we don't die too often and get throttled by launchd");
    loop {
        sleep(Duration::new(1, 0));
        println!("ping")
    }
}

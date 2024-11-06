use core::panic;
use std::{collections::{BTreeSet, HashMap}, env::var, fs::read_to_string, os::macos::raw::stat, path::Path, process::Command, sync::{mpsc::{channel, sync_channel, Receiver, Sender, SyncSender}, Arc, RwLock}, thread::{sleep, spawn, JoinHandle}, time::{self, Duration}};

use key::{Key, KeyboardKey, ModifierKey};
use notify::FsEventWatcher;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, DebouncedEvent, DebouncedEventKind, Debouncer};
use rdev::{grab, listen, simulate, Event, EventType, ListenError};
use toml::{map::Map, Table, Value};

mod key;
/*

#[derive(Debug)]
struct ConfigState {
    macros: HashMap<Bind, Action>
}

impl ConfigState {
    fn new() -> Self {
        return ConfigState {
            macros: HashMap::new()
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
struct Config_ {
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



impl Action {
    fn send(event_type: &EventType) {
        let delay = time::Duration::from_millis(20);
        match simulate(event_type) {
            Ok(()) => (),
            Err(simulate_error) => {
                println!("We could not send {:?}: {:?}", event_type, simulate_error);
            }
        }
        sleep(delay);
    }

    pub fn execute(&self) {
        match self {
            Action::Cmd { command } => {
                let mut command_split: Vec<&str> = command.split(" ").collect();
                let command_split_clone = command_split.clone();
                let program = match command_split_clone.get(0) {
                    Some(p) => p,
                    None => {
                        eprintln!("invalid command: {}", command);
                        return
                    }
                };

                command_split.drain(0..1);

                let mut envs = HashMap::new();

                // we want to set the USER env var to the currently logged in user
                match Command::new("/usr/bin/stat").args(["-f", "\"%Su\"", "/dev/console"]).output() {
                    Ok(o) => {
                        if o.status.success() {
                            match String::from_utf8(o.stdout) {
                                Ok(s) => {
                                    let user = s.replace(|c| !char::is_alphabetic(c), "");

                                    envs.insert("USER", user);
                                },
                                Err(e) => eprintln!("error converting stdout to string: {}", e)
                            }
                        }
                    },
                    Err(e) => eprintln!("error getting currently logged in user: {}", e)
                };

                let b = Command::new(program)
                    .args(command_split)
                    .envs(envs)
                    .output();
                println!("output: {:?}", b)
            },
            Action::Macro { r#macro } => {
                for m in r#macro {
                    // key down
                    println!("{:?}", m);
                    for key in m {
                        let r = key.to_rdev();
                        match r {
                            (None, Some(b)) => {
                                Self::send(&EventType::ButtonPress(b));
                            },
                            (Some(k), None) => {
                                Self::send(&EventType::KeyPress(k));
                            },
                            _ => eprintln!("bad state: {:?}", r),
                        }
                    }

                    // key up
                    for key in m {
                        let r = key.to_rdev();
                        match r {
                            (None, Some(b)) => {
                                Self::send(&EventType::ButtonRelease(b));
                            },
                            (Some(k), None) => {
                                Self::send(&EventType::KeyRelease(k));
                            },
                            _ => eprintln!("bad state: {:?}", r),
                        }
                    }
                }
            },
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
    Unknown(ListenError)
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

        let handle = spawn(move || {
            let sender = event_sender.clone();
            let res = listen(move |event: Event| { 
                // handle extra shit here
                let _ = sender.send(KeyListenerEvent::Event(event.clone()));
            }); 
            let event = match res {
                Err(e) => KeyListenerEvent::Error(match e {
                    rdev::ListenError::EventTapError => KeyListenerError::InvalidPermissions,
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
                                    self.state.key_down(Key::new_from_rdev(key));
                                },
                                rdev::EventType::KeyRelease(key) => {
                                    self.state.key_up(Key::new_from_rdev(key));
                                },
                                rdev::EventType::Wheel { delta_x, delta_y } => {
                                },
                                rdev::EventType::MouseMove { x, y } => {
                                },
                                rdev::EventType::ButtonPress(button) => {
                                },
                                rdev::EventType::ButtonRelease(button) => {
                                },
                            }
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
*/

#[derive(Debug, Clone)]
enum Action {
    Cmd { command: String },
}

impl Action {
    pub fn execute(&self) {
        match self {
            Action::Cmd { command } => {
                let mut command_split: Vec<&str> = command.split(" ").collect();
                let command_split_clone = command_split.clone();
                let program = match command_split_clone.get(0) {
                    Some(p) => p,
                    None => {
                        eprintln!("invalid command: {}", command);
                        return
                    }
                };

                command_split.drain(0..1);

                let mut envs = HashMap::new();

                // we want to set the USER env var to the currently logged in user
                match Command::new("/usr/bin/stat").args(["-f", "\"%Su\"", "/dev/console"]).output() {
                    Ok(o) => {
                        if o.status.success() {
                            match String::from_utf8(o.stdout) {
                                Ok(s) => {
                                    let user = s.replace(|c| !char::is_alphabetic(c), "");

                                    envs.insert("USER", user);
                                },
                                Err(e) => eprintln!("error converting stdout to string: {}", e)
                            }
                        }
                    },
                    Err(e) => eprintln!("error getting currently logged in user: {}", e)
                };

                let b = Command::new(program)
                    .args(command_split)
                    .envs(envs)
                    .output();
                println!("output: {:?}", b)
            },
            /*
            Action::Macro { r#macro } => {
                for m in r#macro {
                    // key down
                    println!("{:?}", m);
                    for key in m {
                        let r = key.to_rdev();
                        match r {
                            (None, Some(b)) => {
                                Self::send(&EventType::ButtonPress(b));
                            },
                            (Some(k), None) => {
                                Self::send(&EventType::KeyPress(k));
                            },
                            _ => eprintln!("bad state: {:?}", r),
                        }
                    }

                    // key up
                    for key in m {
                        let r = key.to_rdev();
                        match r {
                            (None, Some(b)) => {
                                Self::send(&EventType::ButtonRelease(b));
                            },
                            (Some(k), None) => {
                                Self::send(&EventType::KeyRelease(k));
                            },
                            _ => eprintln!("bad state: {:?}", r),
                        }
                    }
                }
            },
            Action::MouseModifier { x_mul, y_mul } => println!("mouse-modifier: x_mul: {}, y_mul: {}", x_mul, y_mul),
            Action::ScrollModifier { x_mul, y_mul } => println!("scroll-modifier: x_mul: {}, y_mul: {}", x_mul, y_mul)
    */
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
            /*
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
        */
            _ => return None
        };

        Some(action)
    }
}

#[derive(Debug, Eq, PartialEq, Hash, Clone)]
struct Bind {
    modifiers: BTreeSet<ModifierKey>,
    key: KeyboardKey
}

#[derive(Debug, Clone)]
struct HIDState {
    modifiers: BTreeSet<ModifierKey>,
}

impl HIDState {
    pub fn new() -> Self {
        return HIDState{
            modifiers: BTreeSet::new(),
        }
    }
}

struct EventReceiver {
    state: HIDState,
    config_stream_sender: Sender<ConfigManagerMessage>,
    event_blocking_sender: Sender<Option<Event>>
}

#[derive(Debug)]
enum EventReceiverMessage {
    Event(rdev::Event)
}

impl EventReceiver {
    pub fn start(&mut self, event_stream: Receiver<EventReceiverMessage>) {
        loop {
            match event_stream.recv() {
                Ok(e) => {
                    match e {
                        EventReceiverMessage::Event(event) => {
                            match event.event_type {
                                EventType::KeyPress(rdevkey) => {
                                    let key = Key::new_from_rdev(rdevkey);
                                    match key {
                                        Key::Modifier(modifier_key) => {
                                            let _ = self.state.modifiers.insert(modifier_key);
                                            self.event_blocking_sender.send(Some(event));
                                        },
                                        Key::Keyboard(keyboard_key) => {
                                            let _ = self.config_stream_sender.send(ConfigManagerMessage::Event(event, self.state.clone(), keyboard_key));
                                        }
                                    }
                                },
                                EventType::KeyRelease(rdevkey) => {
                                    let key = Key::new_from_rdev(rdevkey);
                                    if let Key::Modifier(modifier_key) = key {
                                        let _ = self.state.modifiers.remove(&modifier_key);
                                    }

                                    self.event_blocking_sender.send(Some(event));
                                }
                                _ => {
                                    let _ = self.event_blocking_sender.send(Some(event));
                                }
                            }
                        }
                    }
                },
                Err(err) => eprintln!("[EventReceiver] {:?}", err)
            }
        }
    } 

    pub fn new(
        config_manager_stream: Sender<ConfigManagerMessage>,
        event_blocking_sender: Sender<Option<Event>>
    ) -> Self {
        EventReceiver {
            state: HIDState::new(),
            config_stream_sender: config_manager_stream,
            event_blocking_sender
        }
    }
}

#[derive(Debug)]
enum ConfigManagerMessage {
    Event(Event, HIDState, KeyboardKey),
    ConfigUpdate(Config)
}

#[derive(Debug)]
struct Config {
    macros: HashMap<Bind, Action>
}

impl Config {
    pub fn new() -> Self {
        let mut macros = HashMap::new();
        let mut modifiers = BTreeSet::new();

        modifiers.insert(ModifierKey::ShiftLeft);

        let bind = Bind {
            modifiers,
            key: KeyboardKey::KeyQ
        };

        let action = Action::Cmd { command: "bruh".to_string() };
        macros.insert(bind, action);
        Config {
            macros
        }
    }

    fn new_from_file(path: String) -> Self {
        // parse file here
        //
        let content = match read_to_string(path) {
            Ok(content) => content,
            Err(error) => {
                eprintln!("error reading config file: {}", error);
                return Config::new()
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
                            panic!("multiple instances of the same modifier in macro: {:?}", key);
                        }
                    },
                    (None, Some(key)) => {
                        if keyboard_key != None {
                            panic!("multiple keyboard keys in macro: {:?}", key);
                        }
                        keyboard_key = Some(key);
                    },
                    _ => panic!("key is both modifier and keyboard key: {}", key)
                }
            }

            if keyboard_key.is_none() {
                
            }

            if modifiers.len() == 0 {
                panic!("no modifier keys in macro: {:?}", keys);
            }

            let bind = {
                let some_keyboard_key = match keyboard_key {
                    Some(k_key) => k_key,
                    None => panic!("no keyboard key in macro: {:?}", keys)
                };

                Bind {
                    modifiers,
                    key: some_keyboard_key
                }
            };

            let value_table = match value {
                Value::Table(t) => t,
                _ => panic!("value of bind is not a table: {:?}", value)
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

        println!("macros: {:?}", macros);

        return Config {
            macros
        }
    }
}

struct ConfigManager {
    config: Config,
    action_executor_stream: Sender<ActionExecutorMessage>,
    event_blocking_sender: Sender<Option<Event>>,

    fs_watcher_handle: Option<Debouncer<FsEventWatcher>>,
    config_file_path: Option<String>
}

impl ConfigManager {

    fn get_action(&self, state: HIDState, keyboard_key: KeyboardKey) -> Option<Action> {
        let bind = Bind {
            modifiers: state.modifiers,
            key: keyboard_key
        };
        self.config.macros.get(&bind).cloned()
    }

    pub fn start_fs_watcher(&mut self, config_manager_sender: Sender<ConfigManagerMessage>) { 

        let fs_watcher_handle = match self.config_file_path.clone() {
            Some(config_fp) => {
                // channel to notify to relaod the config
                let config_fp_cloned = config_fp.clone();
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
                            let send_result = config_manager_sender.send(ConfigManagerMessage::ConfigUpdate(Config::new_from_file(config_fp_cloned.clone())));
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
                            Ok(_) => Some(wat),
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

        self.fs_watcher_handle = fs_watcher_handle;
    }

    pub fn start(&mut self, config_manager_receiver: Receiver<ConfigManagerMessage>) {
        // fs watcher here
        loop {
            match config_manager_receiver.recv() {
                Ok(msg) => {
                    match msg {
                        ConfigManagerMessage::ConfigUpdate(cfg) => {
                            self.config = cfg;
                        },
                        ConfigManagerMessage::Event(event, state, keyboard_key) => {
                            let action = self.get_action(state, keyboard_key);
                            match &action {
                                Some(a) => {
                                    let _ = self.action_executor_stream.send(ActionExecutorMessage::ExecuteAction(a.clone()));
                                },
                                None => {},
                            };

                            let response_event = if action.is_some() { None } else { Some(event) };

                            let _ = self.event_blocking_sender.send(response_event);
                        }
                    } 
                },
                Err(err) => eprintln!("[ConfigManager] {:?}", err)
            }
        }
    }

    pub fn new(
        action_executor_stream: Sender<ActionExecutorMessage>,
        event_blocking_sender: Sender<Option<Event>>
    ) -> Self {

        let config_file_path = match var("HOTKEYD_CONFIG") {
            Ok(path) => Some(path),
            Err(err) => {
                eprintln!("error getting config path: {}, maybe HOTKEYD_CONFIG was not set?", err);
                None
            }
        };

        let config = match config_file_path.clone() {
            Some(fp) => Config::new_from_file(fp),
            None => Config::new()
        };

        return ConfigManager {
            config,
            action_executor_stream,
            event_blocking_sender,
            fs_watcher_handle: None,
            config_file_path
        }
    }
}

#[derive(Debug)]
enum ActionExecutorMessage {
    ExecuteAction(Action)
}

struct ActionExecutor {
}

impl ActionExecutor {
    pub fn new() -> Self {
        ActionExecutor { }
    }

    pub fn start(&self, action_executor_receiver: Receiver<ActionExecutorMessage>) {
        loop {
            match action_executor_receiver.recv() {
                Ok(msg) => {
                    match msg {
                        ActionExecutorMessage::ExecuteAction(action) => {
                            println!("[ActionExecutor] executing action: {:?}", action);
                            action.execute();
                        }
                    }
                },
                Err(err) => eprintln!("[ActionExecutor] {:?}", err)
            }
        }
    }
}

fn main() {
    println!("start daemon");

    println!("starting action executor");
    let action_executor = ActionExecutor::new();
    let (action_executor_sender, action_executor_receiver) = channel::<ActionExecutorMessage>();
    let _action_executor_handle = spawn(move || {
        action_executor.start(action_executor_receiver);
    });

    println!("starting config manager");
    let (event_blocking_sender, event_blocking_receiver) = channel::<Option<Event>>();

    let mut config_manager = ConfigManager::new(action_executor_sender, event_blocking_sender.clone());
    let (config_manager_sender, config_manager_receiver) = channel::<ConfigManagerMessage>();
    let config_manager_sender_cloned = config_manager_sender.clone();
    let _config_manager_handle = spawn(move || {
        config_manager.start_fs_watcher(config_manager_sender_cloned);
        config_manager.start(config_manager_receiver); 
    });

    println!("starting event receiver");
    let mut event_receiver = EventReceiver::new(config_manager_sender, event_blocking_sender);
    let (event_receiver_sender, event_receiver_receiver) = channel::<EventReceiverMessage>();
    let _event_receiver_handle = spawn(move || {
        event_receiver.start(event_receiver_receiver);
    });

    println!("starting event listener");
    let _ = grab(move |event: Event| -> Option<Event> {
        let _ = event_receiver_sender.send(EventReceiverMessage::Event(event.clone()));

        let res = event_blocking_receiver.recv();
        match res {
            Ok(e) => e,
            Err(err) => {
                eprintln!("error receiving {:?}", err);

                Some(event)
            }
        }
    });

    // keep alive
    println!("keeping daemon alive so we don't die too often and get throttled by launchd");
    loop {
        sleep(Duration::new(1, 0));
        println!("ping")
    }
}

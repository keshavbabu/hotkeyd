use core::panic;
use std::{collections::{BTreeSet, HashMap}, env::var, fs::read_to_string, path::Path, process::Command, sync::mpsc::{channel, Receiver, Sender}, thread::{sleep, spawn}, time::Duration};

use key::{Key, KeyboardKey, ModifierKey};
use notify::FsEventWatcher;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, DebouncedEvent, DebouncedEventKind, Debouncer};
use rdev::{grab, Event, EventType};
use toml::{map::Map, Table, Value};

mod key;

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

                let commands: Vec<&str> = command.split(" && ").collect();
                for cmd in commands {
                    let mut command_split: Vec<&str> = cmd.split(" ").collect();
                    let command_split_clone = command_split.clone();
                    let program = match command_split_clone.get(0) {
                        Some(p) => p,
                        None => {
                            eprintln!("invalid command: {}", cmd);
                            return
                        }
                    };

                    command_split.drain(0..1);

                    

                    let b = Command::new(program)
                        .args(command_split)
                        .envs(envs.clone())
                        .output();
                    println!("output: {:?}", b)
                } 
            },
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
                                            let _ = self.event_blocking_sender.send(Some(event));
                                            println!("modifier: {:?}", modifier_key);
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

                                    let _ = self.event_blocking_sender.send(Some(event));
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

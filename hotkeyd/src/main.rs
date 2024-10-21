use std::{collections::{BTreeSet, HashMap}, env::var, fs::read_to_string, sync::mpsc::{sync_channel, Receiver, SyncSender}, thread::{self, sleep}, time::Duration};

use button_mapping::button_hash;
use key_mapping::key_hash;
use rdev::{grab, Button, Event, GrabError, Key};
use serde::Deserialize;

mod key_mapping;
mod button_mapping;

#[derive(Debug)]
struct ConfigState {
    // using this because
    binds: HashMap<BTreeSet<u32>, Action>
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

        // parse that shit

        return ConfigState {
            binds: HashMap::new()
        }
    }
}

#[derive(Debug)]
struct Config {
    state: ConfigState
}

impl Config {
    fn new() -> Self {
        // we are going to start a watcher here that basically just listens for changes to the
        // config file
        let config_state = match var("HOTKEYD_CONFIG") {
            Ok(file_path) => {
                ConfigState::new_from_file(file_path)
            },
            Err(error) => {
                eprintln!("Error: {}, maybe HOTKEYD_CONFIG was not set? using empty config.", error);
                ConfigState::new()
            }
        };

        return Config { 
            state: config_state 
        }
    }

    fn action_for_state(&mut self, key_state: KeyState) -> Vec<Action> {
        let mut actions: Vec<Action> = vec![];
        // check key_binds
        if let Some(action) = self.state.binds.get(&key_state.keys_down) { 
            actions.push(action.clone());
        }

        actions
    }
}

#[derive(Debug, Deserialize, Clone)]
enum Action {
    Cmd { command: String },
    Macro { r#macro: Vec<BTreeSet<u32>>},
    ScrollModifier { x_mul: i64, y_mul: i64 },
    MouseModifier { x_mul: f64, y_mul: f64 }
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
    keys_down: BTreeSet<u32>,
    buttons_down: BTreeSet<u32>,
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
            buttons_down: BTreeSet::new(),
            last_mouse_position: None,
            scrolling_speed: WheelScroll { delta_x: 0, delta_y: 0 }
        }
    }

    fn key_down(&mut self, key: Key) -> bool {
        self.keys_down.insert(key_hash(key))
    }

    fn key_up(&mut self, key: Key) -> bool {
        self.keys_down.remove(&key_hash(key))
    }

    fn scroll_speed(&mut self, delta_x: i64, delta_y: i64) -> bool {
        self.scrolling_speed = WheelScroll { delta_x, delta_y };
        true
    }

    fn mouse_position(&mut self, x: f64, y: f64) -> bool {
        self.last_mouse_position = Some(MousePosition { x, y });
        true
    }

    fn button_down(&mut self, button: Button) -> bool {
        self.buttons_down.insert(button_hash(button))
    }

    fn button_up(&mut self, button: Button) -> bool {
        self.buttons_down.remove(&button_hash(button))
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
            match action {
                Action::Cmd { command } => println!("command: {}", command),
                Action::Macro { r#macro } => println!("macro: {:?}", r#macro),
                Action::MouseModifier { x_mul, y_mul } => println!("mouse-modifier: x_mul: {}, y_mul: {}", x_mul, y_mul),
                Action::ScrollModifier { x_mul, y_mul } => println!("scroll-modifier: x_mul: {}, y_mul: {}", x_mul, y_mul)
            }
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
        let handle = thread::spawn(move || {
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
                                    self.state.key_down(key);
                                },
                                rdev::EventType::KeyRelease(key) => {
                                    self.state.key_up(key);
                                },
                                rdev::EventType::Wheel { delta_x, delta_y } => {
                                    self.state.scroll_speed(delta_x, delta_y);
                                },
                                rdev::EventType::MouseMove { x, y } => {
                                    self.state.mouse_position(x, y);
                                },
                                rdev::EventType::ButtonPress(button) => {
                                    self.state.button_down(button);
                                },
                                rdev::EventType::ButtonRelease(button) => {
                                    self.state.button_up(button);
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

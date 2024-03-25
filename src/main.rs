#![warn(clippy::all, clippy::nursery, clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::redundant_pub_crate)]

use crate::parser::HotkeyBinding;
use crate::whkdrc::Shell;
use crate::whkdrc::Whkdrc;
use clap::Parser;
use color_eyre::eyre::eyre;
use color_eyre::eyre::Result;
use global_hotkey::hotkey;
use global_hotkey::hotkey::Code;
use global_hotkey::hotkey::HotKey;
use global_hotkey::hotkey::Modifiers;
use global_hotkey::GlobalHotKeyEvent;
use global_hotkey::GlobalHotKeyManager;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::ChildStdin;
use std::process::Command;
use std::process::Stdio;
use std::str::FromStr;
use std::sync::Arc;
use windows_hotkeys::error::HkError;
use winit::event_loop::EventLoopBuilder;

mod parser;
mod whkdrc;

lazy_static! {
    static ref WHKDRC: Whkdrc = {
        // config file defaults to `~/.config/whkdrc`, or `<WHKD_CONFIG_HOME>/whkdrc`
        let mut home  = std::env::var("WHKD_CONFIG_HOME").map_or_else(
            |_| dirs::home_dir().expect("no home directory found").join(".config"),
            |home_path| {
                let home = PathBuf::from(&home_path);

                if home.as_path().is_dir() {
                    home
                } else {
                    panic!(
                        "$Env:WHKD_CONFIG_HOME is set to '{home_path}', which is not a valid directory",
                    );
                }
            },
        );
        home.push("whkdrc");
        Whkdrc::load(&home).unwrap_or_else(|_| panic!("could not load whkdrc from {home:?}"))
    };
    static ref SESSION_STDIN: Mutex<Option<ChildStdin>> = Mutex::new(None);
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HkmData {
    pub mode: Option<String>,
    pub mod_keys: Option<Modifiers>,
    pub vkey: hotkey::Code,
    pub command: Option<String>,
    pub internal_action: Option<Option<String>>,
    pub process_name: Option<String>,
}

impl TryFrom<&HotkeyBinding> for HkmData {
    type Error = HkError;

    fn try_from(value: &HotkeyBinding) -> Result<Self, Self::Error> {
        let (trigger, mods) = value.keys.split_last().unwrap();
        let mut mod_keys = Modifiers::empty();
        let vkey = key_code_from_string(&trigger).unwrap();
        for m in mods {
            mod_keys |= modifier_from_string(m);
        }

        let mod_keys = if mod_keys.is_empty() {
            None
        } else {
            Some(mod_keys)
        };

        Ok(Self {
            mode: value.mode.clone(),
            mod_keys,
            vkey,
            command: value.command.clone(),
            internal_action: value.internal_action.clone(),
            process_name: value.process_name.clone(),
        })
    }
}

fn key_code_from_string(key: &str) -> Option<Code> {
    match key.to_lowercase().as_str() {
        "a" => Some(Code::KeyA),
        "b" => Some(Code::KeyB),
        "c" => Some(Code::KeyC),
        "d" => Some(Code::KeyD),
        "e" => Some(Code::KeyE),
        "f" => Some(Code::KeyF),
        "g" => Some(Code::KeyG),
        "h" => Some(Code::KeyH),
        "i" => Some(Code::KeyI),
        "j" => Some(Code::KeyJ),
        "k" => Some(Code::KeyK),
        "l" => Some(Code::KeyL),
        "m" => Some(Code::KeyM),
        "n" => Some(Code::KeyN),
        "o" => Some(Code::KeyO),
        "p" => Some(Code::KeyP),
        "q" => Some(Code::KeyQ),
        "r" => Some(Code::KeyR),
        "s" => Some(Code::KeyS),
        "t" => Some(Code::KeyT),
        "u" => Some(Code::KeyU),
        "v" => Some(Code::KeyV),
        "w" => Some(Code::KeyW),
        "x" => Some(Code::KeyX),
        "y" => Some(Code::KeyY),
        "z" => Some(Code::KeyZ),
        "escape" => Some(Code::Escape),
        _ => Code::from_str(key).ok(),
    }
}

fn modifier_from_string(modifier: &str) -> Modifiers {
    match modifier {
        "ctrl" => Modifiers::CONTROL,
        "alt" => Modifiers::ALT,
        "shift" => Modifiers::SHIFT,
        "super" => Modifiers::SUPER,
        _ => Modifiers::empty(),
    }
}

#[derive(Parser)]
#[clap(author, about, version)]
struct Cli {
    /// Path to whkdrc
    #[clap(action, short, long)]
    config: Option<PathBuf>,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let cli = Cli::parse();

    let whkdrc = cli.config.map_or_else(
        || WHKDRC.clone(),
        |config| {
            Whkdrc::load(&config)
                .unwrap_or_else(|_| panic!("could not load whkdrc from {config:?}"))
        },
    );

    let shell_binary = whkdrc.shell.to_string();

    match whkdrc.shell {
        Shell::Powershell | Shell::Pwsh => {
            let mut process = Command::new(&shell_binary)
                .stdin(Stdio::piped())
                .args(["-Command", "-"])
                .spawn()?;

            let mut stdin = process
                .stdin
                .take()
                .ok_or_else(|| eyre!("could not take stdin from powershell session"))?;

            writeln!(stdin, "$wshell = New-Object -ComObject wscript.shell")?;

            let mut session_stdin = SESSION_STDIN.lock();
            *session_stdin = Option::from(stdin);
        }
        Shell::Cmd => {
            let mut process = Command::new(&shell_binary)
                .stdin(Stdio::piped())
                .args(["-"])
                .spawn()?;

            let mut stdin = process
                .stdin
                .take()
                .ok_or_else(|| eyre!("could not take stdin from cmd session"))?;

            writeln!(stdin, "prompt $S")?;

            let mut session_stdin = SESSION_STDIN.lock();
            *session_stdin = Option::from(stdin);
        }
    }

    /*     let mut hkm = HotkeyManager::new();
    hkm.set_no_repeat(false);

    let mut mapped = HashMap::new();
    for (keys, app_bindings) in &whkdrc.app_bindings {
        for binding in app_bindings {
            let data = HkmData::try_from(binding)?;
            mapped
                .entry(keys.join("+"))
                .or_insert_with(Vec::new)
                .push(data);
        }
    }

    for (_, v) in mapped {
        let vkey = v[0].vkey;
        let mod_keys = v[0].mod_keys.as_slice();

        let v = v.clone();
        hkm.register(vkey, mod_keys, move || {
            if let Some(session_stdin) = SESSION_STDIN.lock().as_mut() {
                for e in &v {
                    let cmd = &e.command;
                    if let Some(proc) = &e.process_name {
                        match active_win_pos_rs::get_active_window() {
                            Ok(window) => {
                                if window.app_name == *proc {
                                    if let Some(cmd) = cmd {
                                        if matches!(whkdrc.shell, Shell::Pwsh | Shell::Powershell) {
                                            println!("{cmd}");
                                        }

                                        writeln!(session_stdin, "{cmd}")
                                            .expect("failed to execute command");
                                    }
                                }
                            }
                            Err(error) => {
                                dbg!(error);
                            }
                        }
                    }
                }
            }
        })?;
    } */

    let mode_manager = ModeManager::new(&whkdrc.bindings)?;
    mode_manager.activate_mode(&None)?;

    let event_loop = EventLoopBuilder::new().build().unwrap();
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);

    let channel = GlobalHotKeyEvent::receiver();

    event_loop
        .run(move |_event, _| {
            if let Ok(event) = channel.try_recv() {
                println!("{event:?}");

                let hotkey = {
                    let hotkeys = mode_manager.hotkeys.lock();
                    hotkeys
                        .iter()
                        .find(|(_, v)| v.id() == event.id)
                        .unwrap()
                        .0
                        .clone()
                };

                if let Some(cmd) = &hotkey.command {
                    if let Some(session_stdin) = SESSION_STDIN.lock().as_mut() {
                        if matches!(whkdrc.shell, Shell::Pwsh | Shell::Powershell) {
                            println!("{cmd}");
                        }

                        writeln!(session_stdin, "{cmd}").expect("failed to execute command");
                    }
                }

                if let Some(cmd) = &hotkey.internal_action {
                    println!("setting mode to {cmd:?}");
                    mode_manager.activate_mode(cmd).unwrap();
                }
            }
        })
        .unwrap();

    // hkm.event_loop();

    Ok(())
}

#[derive(Clone)]
struct ModeManager {
    mode: Arc<Mutex<Option<String>>>,
    binding_map: Arc<HashMap<Option<String>, Vec<HkmData>>>,
    hotkeys: Arc<Mutex<HashMap<HkmData, HotKey>>>,
    hotkeys_manager: Arc<GlobalHotKeyManager>,
}

impl ModeManager {
    fn new(bindings: &Vec<HotkeyBinding>) -> Result<Self, HkError> {
        let mut binding_map = HashMap::new();
        let mut hotkeys = HashMap::new();

        for binding in bindings {
            let data = HkmData::try_from(binding)?;
            binding_map
                .entry(data.mode.clone())
                .or_insert_with(Vec::new)
                .push(data.clone());

            let hotkey = HotKey::new(data.mod_keys, data.vkey);
            hotkeys.insert(data, hotkey);
        }

        Ok(Self {
            mode: Arc::new(Mutex::new(None)),
            binding_map: Arc::new(binding_map),
            hotkeys: Arc::new(Mutex::new(hotkeys)),
            hotkeys_manager: Arc::new(GlobalHotKeyManager::new().unwrap()),
        })
    }

    fn activate_mode(&self, mode: &Option<String>) -> Result<(), HkError> {
        let lock = &self.hotkeys.lock();

        self.hotkeys_manager.unregister_all(
            self.binding_map
                .get(&self.mode.lock())
                .map_or(Vec::new(), |v| {
                    v.iter()
                        .map(|h| lock.get(h).unwrap())
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .as_slice(),
        );

        *self.mode.lock() = mode.clone();

        self.hotkeys_manager.register_all(
            self.binding_map
                .get(mode)
                .map_or(Vec::new(), |v| {
                    v.iter()
                        .map(|h| lock.get(h).unwrap())
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .as_slice(),
        );

        Ok(())
    }
}

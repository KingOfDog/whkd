use crate::whkdrc::Shell;
use crate::whkdrc::Whkdrc;
use chumsky::prelude::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyBinding {
    pub mode: Option<String>,
    pub keys: Vec<String>,
    pub command: Option<String>,
    pub internal_action: Option<Option<String>>,
    pub process_name: Option<String>,
}

#[must_use]
pub fn parser() -> impl Parser<char, Whkdrc, Error = Simple<char>> {
    let comment = just::<_, _, Simple<char>>("#")
        .then(take_until(text::newline()))
        .padded()
        .ignored();

    let shell = just(".shell")
        .padded()
        .ignore_then(choice((just("pwsh"), just("powershell"), just("cmd"))))
        .repeated()
        .exactly(1)
        .collect::<String>()
        .map(Shell::from);

    let mode_delimiter = just(">").padded();
    let mode_selector = (text::ident().padded().then_ignore(mode_delimiter))
        .or_not()
        .map(|a| {
            if Some(String::from("default")) == a {
                None
            } else {
                a
            }
        });

    let change_mode_delimiter = just(";").padded();
    let change_mode = text::ident()
        .padded()
        .map(|a| if a == "default" { None } else { Some(a) });

    let hotkeys = choice((text::ident(), text::int(10)))
        .padded()
        .separated_by(just("+"))
        .collect::<Vec<String>>();

    let delimiter = just(":").padded();

    let command = choice((
        comment,
        text::newline(),
        change_mode_delimiter.ignored(),
        end(),
    ))
    .not()
    .repeated()
    .padded()
    .collect::<String>();

    let process_name = text::ident()
        .padded()
        .repeated()
        .at_least(1)
        .map(|a| a.join(" "));

    let process_mapping = process_name
        .then_ignore(delimiter)
        .then(command.clone())
        .padded()
        .padded_by(comment.repeated())
        .repeated()
        .at_least(1);

    let process_command_map = just("[")
        .ignore_then(process_mapping)
        .padded()
        .padded_by(comment.repeated())
        .then_ignore(just("]"))
        .collect::<Vec<(String, String)>>();

    let action = choice((
        delimiter
            .ignore_then(command)
            .then(change_mode_delimiter.ignore_then(change_mode).or_not())
            .map(|(a, b)| (Some(a), b)),
        change_mode_delimiter
            .ignore_then(change_mode)
            .map(|a| (None, Some(a))),
    ));

    let binding = mode_selector.then(hotkeys).then(action);
    let process_bindings = hotkeys.then(process_command_map);

    shell
        .then(
            process_bindings
                .map(|(keys, apps_commands)| {
                    let mut collected = vec![];
                    for (app, command) in apps_commands {
                        collected.push(HotkeyBinding {
                            mode: None,
                            keys: keys.clone(),
                            command: Some(command),
                            internal_action: None,
                            process_name: Option::from(app),
                        });
                    }

                    (keys, collected)
                })
                .padded()
                .padded_by(comment.repeated())
                .repeated()
                .at_least(0),
        )
        .then(
            binding
                .map(|((mode, keys), (command, internal_action))| HotkeyBinding {
                    mode,
                    keys,
                    command,
                    internal_action,
                    process_name: None,
                })
                .padded()
                .padded_by(comment.repeated())
                .repeated()
                .at_least(1),
        )
        .map(|((shell, app_bindings), bindings)| Whkdrc {
            shell,
            app_bindings,
            bindings,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_line_parse() {
        let src = r#"
.shell pwsh # can be one of cmd | pwsh | powershell

alt + h : echo "Hello""#;

        let output = parser().parse(src);
        let expected = Whkdrc {
            shell: Shell::Pwsh,
            app_bindings: vec![],
            bindings: vec![HotkeyBinding {
                mode: None,
                keys: vec![String::from("alt"), String::from("h")],
                command: Some(String::from("echo \"Hello\"")),
                internal_action: None,
                process_name: None,
            }],
        };

        assert_eq!(output.unwrap(), expected);
    }

    #[test]
    fn test_multi_mode() {
        let src = r#"
.shell pwsh # can be one of cmd | pwsh | powershell

alt + h ; window
window > esc ; default

window > m : echo "Hello"
window > c : echo "Test" ; default"#;

        let output = parser().parse(src);
        let expected = Whkdrc {
            shell: Shell::Pwsh,
            app_bindings: vec![],
            bindings: vec![
                HotkeyBinding {
                    mode: None,
                    keys: vec![String::from("alt"), String::from("h")],
                    command: None,
                    internal_action: Some(Some(String::from("window"))),
                    process_name: None,
                },
                HotkeyBinding {
                    mode: Some(String::from("window")),
                    keys: vec![String::from("esc")],
                    command: None,
                    internal_action: Some(None),
                    process_name: None,
                },
                HotkeyBinding {
                    mode: Some(String::from("window")),
                    keys: vec![String::from("m")],
                    command: Some(String::from("echo \"Hello\"")),
                    internal_action: None,
                    process_name: None,
                },
                HotkeyBinding {
                    mode: Some(String::from("window")),
                    keys: vec![String::from("c")],
                    command: Some(String::from("echo \"Test\"")),
                    internal_action: Some(None),
                    process_name: None,
                },
            ],
        };

        assert_eq!(output.unwrap(), expected);
    }

    #[test]
    fn test_parse() {
        let src = r#"
.shell cmd

# Specify different behaviour depending on the app
alt + n [
    # ProcessName as shown by `Get-Process`
    Firefox       : echo "hello firefox"

    # Spaces are fine, no quotes required
    Google Chrome : echo "hello chrome"
]

# leading newlines are fine
# line comments should parse and be ignored
alt + h     : komorebic focus left # so should comments at the end of a line
alt + j     : komorebic focus down
alt + k     : komorebic focus up
alt + l     : komorebic focus right

# so should empty lines
alt + 1 : komorebic focus-workspace 0 # digits are fine in the hotkeys section

# trailing newlines are fine


"#;

        let output = parser().parse(src);
        let expected = Whkdrc {
            shell: Shell::Cmd,
            app_bindings: vec![(
                vec![String::from("alt"), String::from("n")],
                vec![
                    HotkeyBinding {
                        mode: None,
                        keys: vec![String::from("alt"), String::from("n")],
                        command: Some(String::from(r#"echo "hello firefox""#)),
                        internal_action: None,
                        process_name: Option::from("Firefox".to_string()),
                    },
                    HotkeyBinding {
                        mode: None,
                        keys: vec![String::from("alt"), String::from("n")],
                        command: Some(String::from(r#"echo "hello chrome""#)),
                        internal_action: None,
                        process_name: Option::from("Google Chrome".to_string()),
                    },
                ],
            )],
            bindings: vec![
                HotkeyBinding {
                    mode: None,
                    keys: vec![String::from("alt"), String::from("h")],
                    command: Some(String::from("komorebic focus left")),
                    internal_action: None,
                    process_name: None,
                },
                HotkeyBinding {
                    mode: None,
                    keys: vec![String::from("alt"), String::from("j")],
                    command: Some(String::from("komorebic focus down")),
                    internal_action: None,
                    process_name: None,
                },
                HotkeyBinding {
                    mode: None,
                    keys: vec![String::from("alt"), String::from("k")],
                    command: Some(String::from("komorebic focus up")),
                    internal_action: None,
                    process_name: None,
                },
                HotkeyBinding {
                    mode: None,
                    keys: vec![String::from("alt"), String::from("l")],
                    command: Some(String::from("komorebic focus right")),
                    internal_action: None,
                    process_name: None,
                },
                HotkeyBinding {
                    mode: None,
                    keys: vec![String::from("alt"), String::from("1")],
                    command: Some(String::from("komorebic focus-workspace 0")),
                    internal_action: None,
                    process_name: None,
                },
            ],
        };

        assert_eq!(output.unwrap(), expected);
    }
}

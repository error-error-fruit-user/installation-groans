//! The one question an install asks: [`prompt_user`].

use ratatui::{
    Frame,
    crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{
    Installer, Location, Mode, Places, places::NO_MODE,
    command_line::{self, apply},
    journal, note,
    places::plan,
    screen::STATE,
    warn,
};

/// Asks the one question: what saying yes does under a red warning, and "Are you sure? [y/n]" in
/// the voice's words. On the screen, the install's parts are ticked or not and the mode switched
/// before the y; without one, it is the journal's question, whose y can carry any of the installer's
/// flags. `--accept` (or being run by another script) is yes unasked. Returns where the install
/// goes, or `None` for no (said, and the run ends in failure).
pub fn prompt_user<C: Installer>(terminal: &mut crate::Screen, cli: &mut C) -> Option<Location<C>> {
    let asked = match terminal.terminal() {
        Some(terminal) if !cli.accept => ask(terminal, cli),
        _ => None,
    };
    terminal.leave();
    journal::speak_in(crate::TOILING_THROUGH_HELL, crate::COMPUTER_SPEAK, cli.computer_speak);
    match asked {
        // 🖥️ asked on the screen: written down, and said no to aloud
        Some(yes) => {
            for line in plan(cli, false) {
                journal::write(&line);
            }
            journal::write(if yes {
                "asked on the screen, the answer was yes"
            } else {
                "asked on the screen, the answer was no"
            });
            match Places::of(cli).ok().filter(|_| yes) {
                Some(places) => Some(Location::new(cli, places)),
                None => no(),
            }
        }
        // ❓ the journal's question, or none at all
        None => {
            journal::warning(&plan(cli, true));
            let Some(added) = journal::confirm(cli.accept, &flag_strs(cli)) else {
                return no();
            };
            if !added.is_empty() {
                note!("with the flags given at the question: {}", added.join(" "));
                if let Err(problem) = apply(cli, added) {
                    warn!("{problem}");
                    return no();
                }
            }
            // 📍 never a guess at where: --locally or --user-wide, from the command line or the answer
            match Places::of(cli) {
                Ok(places) => Some(Location::new(cli, places)),
                Err(problem) => {
                    crate::fail(&problem);
                    None
                }
            }
        }
    }
}

fn flag_strs<C: Installer>(cli: &C) -> Vec<&'static str> {
    // (leaked once, for the journal's &[&str]: the question is asked once a run)
    command_line::flag_names(cli)
        .into_iter()
        .map(|flag| &*Box::leak(flag.into_boxed_str()))
        .collect()
}

/// The answer was no: said, and the run ends in failure.
fn no<T>() -> Option<T> {
    journal::declined();
    if let Ok(mut state) = STATE.lock() {
        state.declined = true;
    }
    None
}

/// A line of the question's list.
enum Item {
    Mode,
    Part(&'static str),
    Switch(&'static str),
}

/// A voice's question, its warning's name and their colours, picked once.
struct Asking {
    question: String,
    question_style: Style,
    warning: String,
    warning_style: Style,
}

impl Asking {
    fn of(source: &str) -> Self {
        let voice = source.parse::<toml::Table>().unwrap_or_default();
        let color = |key: &str| {
            sgr(voice
                .get("colors")
                .and_then(|colors| colors.get(key))
                .and_then(toml::Value::as_str)
                .unwrap_or("1"))
        };
        let questions: Vec<&str> = voice
            .get("phrases")
            .and_then(|phrases| phrases.get("confirm"))
            .and_then(toml::Value::as_array)
            .map(|phrases| phrases.iter().filter_map(toml::Value::as_str).collect())
            .unwrap_or_default();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|time| time.subsec_nanos() as usize)
            .unwrap_or_default();
        Asking {
            question: questions
                .get(nanos % questions.len().max(1))
                .unwrap_or(&"Are you sure? [y/n]")
                .to_string(),
            question_style: color("confirm"),
            warning: voice
                .get("sections")
                .and_then(|sections| sections.get("warning"))
                .and_then(toml::Value::as_str)
                .unwrap_or("Warning")
                .to_string(),
            warning_style: color("warning"),
        }
    }
}

/// Asks on the screen: `Some(true)` for yes, `Some(false)` for no.
fn ask<C: Installer>(terminal: &mut ratatui::DefaultTerminal, cli: &mut C) -> Option<bool> {
    let voices = [Asking::of(crate::TOILING_THROUGH_HELL), Asking::of(crate::COMPUTER_SPEAK)];
    let mut list = ListState::default().with_selected(Some(0));
    loop {
        let items = items(cli);
        let places = Places::of(cli);
        let asking = &voices[cli.computer_speak as usize];
        terminal
            .draw(|frame| draw(frame, cli, &items, &places, asking, &mut list))
            .ok()?;
        let Ok(Event::Key(key)) = event::read() else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        let selected = list.selected().unwrap_or(0);
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Some(false),
            KeyCode::Char('y' | 'Y') if places.is_ok() => return Some(true),
            KeyCode::Char('n' | 'N' | 'q') | KeyCode::Esc => return Some(false),
            KeyCode::Up | KeyCode::Char('k') => list.select(Some(selected.saturating_sub(1))),
            KeyCode::Down | KeyCode::Char('j') => {
                list.select(Some((selected + 1).min(items.len().saturating_sub(1))))
            }
            KeyCode::Char(' ' | 'x') | KeyCode::Enter | KeyCode::Left | KeyCode::Right => {
                match items.get(selected) {
                    Some((Item::Mode, _)) => {
                        cli.mode = Some(match cli.mode {
                            Some(Mode::InPlace) => Mode::User,
                            _ => Mode::InPlace,
                        });
                        // (the folder given was for the other mode)
                        cli.dir = None;
                    }
                    Some((Item::Part(flag), on)) => {
                        cli.set(flag, !on);
                    }
                    Some((Item::Switch(flag), on)) => {
                        if let Some(switch) = cli.switch(flag) {
                            *switch = !on;
                        }
                    }
                    None => {}
                }
            }
            _ => {}
        }
    }
}

/// The question's list: the mode, the parts that can be left out, and the switches every install
/// has, each with whether it is on.
fn items<C: Installer>(cli: &mut C) -> Vec<(Item, bool)> {
    let mut items = vec![(Item::Mode, cli.mode.is_some())];
    for part in cli.parts() {
        if let Some(on) = part.on {
            items.push((Item::Part(part.flag), on));
        }
    }
    for flag in ["reinstall", "desktop-shortcut", "menu-entry", "computer-speak"] {
        let on = cli.switch(flag).is_some_and(|on| *on);
        items.push((Item::Switch(flag), on));
    }
    items
}

fn draw<C: Installer>(
    frame: &mut Frame,
    cli: &C,
    items: &[(Item, bool)],
    places: &Result<Places, String>,
    asking: &Asking,
    list: &mut ListState,
) {
    let [title, warning, choices, question, keys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(4),
        Constraint::Length(items.len() as u16 + 2),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    let name = C::TITLE;
    frame.render_widget(
        Line::from(vec![
            Span::styled(name, Style::new().add_modifier(Modifier::BOLD)),
            Span::raw(if C::ABOUT.is_empty() { String::new() } else { format!(": {}", C::ABOUT) }),
        ]),
        title,
    );
    let plan: Vec<Line> = plan(cli, false).into_iter().map(Line::from).collect();
    frame.render_widget(
        Paragraph::new(plan).wrap(Wrap { trim: false }).block(
            Block::new()
                .borders(Borders::ALL)
                .border_style(asking.warning_style)
                .title(Span::styled(format!(" {} ", asking.warning), asking.warning_style)),
        ),
        warning,
    );
    let dir = places
        .as_ref()
        .map(|places| places.dir.display().to_string())
        .unwrap_or_default();
    let parts = cli.parts();
    let rows: Vec<ListItem> = items
        .iter()
        .map(|(item, on)| {
            let mark = if *on { "[x] " } else { "[ ] " };
            ListItem::new(match item {
                Item::Mode => match cli.mode {
                    None => "<-> Where? Not said yet: in place (--locally) or for you (--user-wide)".into(),
                    Some(Mode::InPlace) => format!("<-> In place (--locally), in {dir}"),
                    Some(Mode::User) => format!("<-> For you only (--user-wide), in {dir}"),
                },
                Item::Part(flag) => {
                    let doc = parts
                        .iter()
                        .find(|part| part.flag == *flag)
                        .map_or(*flag, |part| part.doc);
                    format!("{mark}{doc}")
                }
                Item::Switch(flag) => format!("{mark}{}", switch_label(flag, name)),
            })
        })
        .collect();
    frame.render_stateful_widget(
        List::new(rows)
            .block(Block::new().borders(Borders::ALL))
            .highlight_symbol("> ")
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED)),
        choices,
        list,
    );
    let asked = match places {
        Err(problem) if problem == NO_MODE => "Say where first: the first line, in place or for you",
        Err(_) => "It can't go there: change where",
        Ok(_) => asking.question.as_str(),
    };
    frame.render_widget(Line::styled(asked, asking.question_style), question);
    frame.render_widget(
        Line::styled(
            "↑↓ choose · space switches · y yes · n no",
            Style::new().add_modifier(Modifier::DIM),
        ),
        keys,
    );
}

/// What a switch every install has does, as the question's list says it.
fn switch_label(flag: &str, name: &str) -> String {
    match flag {
        "reinstall" => "Do everything again, rather than pick up where the last run left off".into(),
        "desktop-shortcut" => format!("Make a shortcut to {name} on the desktop"),
        "menu-entry" => format!("Make an entry for {name} in the app menu"),
        "computer-speak" => "Speak plainly (Computer Speak)".into(),
        other => other.into(),
    }
}

/// The ratatui style for terminal SGR codes (`1;31`), as the voices give their colours.
fn sgr(codes: &str) -> Style {
    let mut style = Style::new();
    let codes: Vec<u8> = codes.split(';').filter_map(|code| code.parse().ok()).collect();
    let mut index = 0;
    while let Some(&code) = codes.get(index) {
        style = match code {
            1 => style.add_modifier(Modifier::BOLD),
            2 => style.add_modifier(Modifier::DIM),
            3 => style.add_modifier(Modifier::ITALIC),
            4 => style.add_modifier(Modifier::UNDERLINED),
            30..=37 => style.fg(Color::Indexed(code - 30)),
            90..=97 => style.fg(Color::Indexed(code - 90 + 8)),
            40..=47 => style.bg(Color::Indexed(code - 40)),
            38 | 48 => {
                let color = match codes.get(index + 1) {
                    Some(5) => {
                        index += 2;
                        codes.get(index).map(|&n| Color::Indexed(n))
                    }
                    Some(2) => {
                        index += 4;
                        match codes.get(index - 2..=index) {
                            Some(&[r, g, b]) => Some(Color::Rgb(r, g, b)),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                match (code, color) {
                    (38, Some(color)) => style.fg(color),
                    (_, Some(color)) => style.bg(color),
                    _ => style,
                }
            }
            _ => style,
        };
        index += 1;
    }
    style
}

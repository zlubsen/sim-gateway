use crossterm::event::{KeyEvent, KeyCode, KeyModifiers};
use std::error::Error;

#[derive(Debug)]
pub enum Command {
    None,
    Up,
    Down,
    Left,
    Right,
    Tab,
    Enter,
    Quit,
    Key(char),
}

impl Command {
    pub fn from(key : KeyEvent) -> Command {
        // let mut lookup = HashMap::new();
        // lookup.insert(KeyEvent {code:KeyCode::Up, modifiers:KeyModifiers::NONE}, Command::Up);
        // lookup.insert(KeyEvent {code:KeyCode::Down, modifiers:KeyModifiers::NONE}, Command::Down);
        // lookup.insert(KeyEvent {code:KeyCode::Left, modifiers:KeyModifiers::NONE}, Command::Left);
        // lookup.insert(KeyEvent {code:KeyCode::Right, modifiers:KeyModifiers::NONE}, Command::Right);
        // lookup.insert(KeyEvent {code:KeyCode::Tab, modifiers:KeyModifiers::NONE}, Command::Tab);
        // lookup.insert(KeyEvent {code:KeyCode::Enter, modifiers:KeyModifiers::NONE}, Command::Enter);
        // lookup.insert(KeyEvent {code:KeyCode::Char('q'), modifiers:KeyModifiers::CONTROL}, Command::Quit);
        //
        // if lookup.contains_key(&key) {
        //     lookup.get(&key).unwrap().clone()
        // } else if key.code == KeyCode:: { Command::Key(key.code) }
        // else { Command::None }

        match key.code {
            KeyCode::Up => Command::Up,
            KeyCode::Down => Command::Down,
            KeyCode::Left => Command::Left,
            KeyCode::Right => Command::Right,
            KeyCode::Tab => Command::Tab,
            KeyCode::Enter => Command::Enter,
            KeyCode::Char(ch) => {
                if ch == 'q' && key.modifiers == KeyModifiers::CONTROL { Command::Quit }
                else { Command::Key(keycode_to_char(key.code).unwrap()) }
            }
            _ => {
                Command::None }
        }
    }
}

fn keycode_to_char(keycode : KeyCode) -> Result<char, ()> {
    match keycode {
        KeyCode::Char(ch) => Ok(ch),
        _ => Err(()),
    }
}
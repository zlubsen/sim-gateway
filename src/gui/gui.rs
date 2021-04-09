use std::sync::mpsc::Receiver;
use std::thread::{Builder, JoinHandle};
use std::io::{stdout, Error};

use crossterm::{
    event::{self, Event as CEvent, KeyCode, KeyEvent},
    terminal::{disable_raw_mode, enable_raw_mode},
};

use tui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::{
        Block, BorderType, Borders, Cell, List, ListItem, ListState, Paragraph, Row, Table, Tabs,
    },
    Terminal,
};

use crate::events::Command;
use std::time::Duration;

struct App {
    active_area : Area,
    prompt : String,
}

enum State {
}

enum Area {
    RouteList,
    RouteDetails,
    Prompt,
}

pub struct Settings {
    pub(crate) placeholder: i32,
}

pub fn start_gui(settings: Settings, rx: Receiver<Command>) -> Result<JoinHandle<()>, Error> {
    let gui_builder = Builder::new().name("GUI".into());
    gui_builder.spawn(move || {
        let stdout = stdout();
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.clear().unwrap();

        const RENDER_RATE : Duration = Duration::from_millis(100);

        let menu_titles = vec!["Home", "Pets", "Add", "Delete", "Quit"];

        loop {
            terminal.draw(|rect| {
                let size = rect.size();
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .margin(2)
                    .constraints(
                        [
                            Constraint::Length(3),
                            Constraint::Min(2),
                            Constraint::Length(3),
                        ]
                            .as_ref(),
                    )
                    .split(size);

                let copyright = Paragraph::new("pet-CLI 2020 - all rights reserved")
                    .style(Style::default().fg(Color::LightCyan))
                    .alignment(Alignment::Center)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .style(Style::default().fg(Color::White))
                            .title("Copyright")
                            .border_type(BorderType::Plain),
                    );
                rect.render_widget(copyright, chunks[2]);

                let menu = menu_titles
                    .iter()
                    .map(|t| {
                        let (first, rest) = t.split_at(1);
                        Spans::from(vec![
                            Span::styled(
                                first,
                                Style::default()
                                    .fg(Color::Yellow)
                                    .add_modifier(Modifier::UNDERLINED),
                            ),
                            Span::styled(rest, Style::default().fg(Color::White)),
                        ])
                    })
                    .collect();

                let tabs = Tabs::new(menu)
                    .select(0)
                    .block(Block::default().title("Menu").borders(Borders::ALL))
                    .style(Style::default().fg(Color::White))
                    .highlight_style(Style::default().fg(Color::Yellow))
                    .divider(Span::raw("|"));

                rect.render_widget(tabs, chunks[0]);
            }).unwrap();

            // handle inputs
            if let Ok(command) = rx.recv_timeout(RENDER_RATE) {
                match command {
                    Command::Quit => {
                        break;
                    }
                    Command::Key(char) => {
                        print!("{}", char);
                    }
                    Command::None => {
                        print!(".");
                    }
                    _ => {
                        // print!("{:?}", command)
                    }
                }
            }
        }
        disable_raw_mode().unwrap();
        terminal.show_cursor().unwrap();
        terminal.clear().unwrap();
    })
}
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

use crate::events::{Command, parse_command};
use std::time::Duration;
use tokio::sync::mpsc::Sender;
use tui::layout::Rect;

const RENDER_RATE : Duration = Duration::from_millis(1000);
const PROMPT_START : &str = " $ ";

struct App {
    active_area : Area,
    prompt : String,
    kill_signal : bool,
    to_rt_tx : Sender<Command>,
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

impl App {
    fn handle_command(&mut self, command : Command) {
        match command {
            Command::Quit => {
                self.kill_signal = true;
                self.to_rt_tx.blocking_send(Command::Quit);
            }
            Command::Key(char) => {
                self.prompt.push(char);
            }
            Command::Backspace => {
                self.prompt.pop();
            }
            Command::Enter => {
                match self.active_area {
                    Area::Prompt => {
                        let prompt_cmd = parse_command(self.prompt.as_str());
                        self.prompt.clear();
                        self.handle_command(prompt_cmd);
                    }
                    _ => {}
                }
            }
            Command::Tab => {
                self.active_area = match self.active_area {
                    Area::Prompt => Area::RouteList,
                    Area::RouteList => Area::RouteDetails,
                    Area::RouteDetails => Area::Prompt,
                }
            }
            Command::None => {
            }
            _ => {
            }
        }
    }
}

pub fn start_gui(settings: Settings, rx: Receiver<Command>, tx: Sender<Command>) -> Result<JoinHandle<()>, Error> {
    let mut app = App {
        active_area : Area::Prompt,
        prompt : String::default(),
        kill_signal : false,
        to_rt_tx : tx,
    };

    let gui_builder = Builder::new().name("GUI".into());
    gui_builder.spawn(move || {
        let stdout = stdout();
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.clear().unwrap();

        let menu_titles = vec!["Home", "Pets", "Add", "Delete", "Quit"];

        while !app.kill_signal {
            terminal.draw(|rect| {
                let size = rect.size();
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .margin(2)
                    .constraints(
                        [
                            Constraint::Length(3),
                            Constraint::Min(2),
                            Constraint::Length(6),
                        ]
                            .as_ref(),
                    )
                    .split(size);

                let prompt = Paragraph::new(format!("{}{}", PROMPT_START, app.prompt))
                    .style(Style::default().fg(Color::LightGreen))
                    .alignment(Alignment::Left)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .style(Style::default().fg(Color::White))
                            .title("Console:")
                            .border_type(BorderType::Plain),
                    );
                rect.render_widget(prompt, chunks[2]);

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

                let middle_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Ratio(1, 3), Constraint::Ratio(2, 3)].as_ref())
                    .split(Rect {
                        x: 0,
                        y: 0,
                        width: 9,
                        height: 2,
                    });
            }).unwrap();

            // handle inputs
            if let Ok(command) = rx.recv_timeout(RENDER_RATE) {
                app.handle_command(command);
            }
        }
        disable_raw_mode().unwrap();
        terminal.show_cursor().unwrap();
        terminal.clear().unwrap();
    })
}
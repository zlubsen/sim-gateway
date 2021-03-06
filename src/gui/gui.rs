use std::thread::{Builder, JoinHandle};
use std::io::{stdout, Error, Stdout};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::broadcast::{Sender, Receiver};

use crossterm::{
    event::{self, Event as CEvent, KeyCode, KeyEvent},
    terminal::{disable_raw_mode, enable_raw_mode},
};

use tui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::{
        Block, BorderType, Borders, Cell, List, ListItem, ListState, Paragraph, Row, Table, Tabs,
    },
    Terminal,
};

use log::{debug};

use crate::events::{Command, parse_command, Event};
use crate::model::config::Config;

const PROMPT_START : &str = " $ ";

struct App {
    active_area : Area,
    prompt : String,
    kill_signal : bool,
    selected_index : usize,
    command_tx : Sender<Command>,
}

// enum State {
// }

enum Area {
    RouteList,
    RouteDetails,
    Prompt,
}

impl App {
    fn handle_command(&mut self, command : Command) {
        match command {
            Command::Quit => {
                self.kill_signal = true;
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
                        let _result = self.command_tx.send(prompt_cmd);
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

pub fn start_gui(mut command_rx: Receiver<Command>, command_tx : Sender<Command>,
                 mut event_rx: Receiver<Event>) -> Result<JoinHandle<()>, Error> {
    let mut app = App {
        active_area : Area::Prompt,
        prompt : String::default(),
        kill_signal : false,
        selected_index: 0,
        command_tx
    };

    let config = Config::current();
    let gui_builder = Builder::new().name("GUI".into());
    gui_builder.spawn(move || {
        let stdout = stdout();
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend).expect("Failed to create terminal.");
        terminal.clear().expect("Failed to clear terminal.");

        let mut terminal = gui_loop(config, app, terminal, command_rx, event_rx);

        disable_raw_mode().expect("Failed to disable raw mode.");
        terminal.show_cursor().expect("Terminal failed to show cursor.");
        terminal.clear().expect("Failed to clear terminal.");
    })
}

fn gui_loop(config: Arc<Config>, mut app: App, mut terminal: Terminal<CrosstermBackend<Stdout>>,
            mut command_rx: Receiver<Command>,
            mut event_rx: Receiver<Event>) -> Terminal<CrosstermBackend<Stdout>> {
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

            // let menu = config.routes
            //     .iter()
            //     .map(|r| {
            //         let (first, rest) = r.split_at(1);
            //         Spans::from(vec![
            //             Span::styled(
            //                 first,
            //                 Style::default()
            //                     .fg(Color::Yellow)
            //                     .add_modifier(Modifier::UNDERLINED),
            //             ),
            //             Span::styled(rest, Style::default().fg(Color::White)),
            //         ])
            //     })
            //     .collect();

            // let tabs = Tabs::new(menu)
            //     .select(0)
            //     .block(Block::default().title("Menu").borders(Borders::ALL))
            //     .style(Style::default().fg(Color::White))
            //     .highlight_style(Style::default().fg(Color::Yellow))
            //     .divider(Span::raw("|"));
            // rect.render_widget(tabs, chunks[0]);

            let header = Paragraph::new("Simulation Gateway - (c) 2021, Zeeger Lubsen")
                .style(Style::default().fg(Color::White))
                .block(Block::default().borders(Borders::ALL));
            rect.render_widget(header, chunks[0]);

            let middle_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Ratio(1, 3), Constraint::Ratio(2, 3)].as_ref())
                .split(Rect {
                    x: 0,
                    y: 0,
                    width: 9,
                    height: 2,
                });

            let routes = config.routes.iter().map(|r|ListItem::new(r.name.clone()));
            // let route_list = List::new(routes)
            //     .block(Block::default().title("Routes").borders(Borders::ALL))
            //     .style(Style::default().fg(Color::White))
            //     .highlight_style(Style::default().add_modifier(Modifier::ITALIC));
            // rect.render_widget(middle_chunks, chunks[1]);
            // rect.render_widget(route_list, middle_chunks[1]);
            // rect.render_widget(middle_chunks, chunks[1]);
        }).unwrap();

        // handle inputs from other threads
        // TODO handle refresh rate with a sleep or so.
        if let Ok(command) = command_rx.try_recv() {
            app.handle_command(command);
        }

        // handle incoming data
        match event_rx.try_recv() {
            Ok(event) => { debug!("gui received event: {:?}", event) }
            Err(err) => { debug!("gui data_rx.try_recv() gave an error {:?}", err)}
        }
    }
    terminal
}
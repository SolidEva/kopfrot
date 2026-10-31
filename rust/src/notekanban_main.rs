use clap::Parser;
use std::ops::DerefMut;

use anyhow::{Context, Error, Result};
use chrono::Local;

use std::{fs, path::Path, path::PathBuf};

use serde::{Deserialize, Serialize};
use serde_yaml;

mod shared_lib;

#[derive(Parser)]
#[command(version)]
struct Cli {
    /// path to the root of the vault, otherwise assumes we are in the vault
    #[arg(short, long)]
    directory: Option<PathBuf>,
}

#[derive(Clone, PartialEq, Debug)]
enum TaskState {
    Open,
    InProgress,
    Done,
}

impl TaskState {
    pub fn from_string(raw_str: &str) -> Result<TaskState, Error> {
        match raw_str.to_lowercase().as_str() {
            "open" => Ok(TaskState::Open),
            "inprogress" => Ok(TaskState::InProgress),
            "done" => Ok(TaskState::Done),
            _ => Err(Error::msg(format!("unsupported task state {raw_str}"))),
        }
    }
}

#[derive(PartialEq, Debug, Clone)]
struct Task {
    title: String,
    state: TaskState,
    path: PathBuf,
}

impl Task {
    pub fn from_path(path: &PathBuf) -> Result<Option<Task>, Error> {
        let file_string = fs::read_to_string(path)?;
        Task::from_string(file_string, path.clone())
    }

    pub fn from_string(raw_str: String, path: PathBuf) -> Result<Option<Task>, Error> {
        let raw = shared_lib::RawNote::from_string(raw_str)?;

        let state_str: String = match raw.properties.get("state") {
            Some(state) => serde_yaml::from_value(state.clone())?,
            None => {
                println!("ignoring note at path {path:?} as it does not have a task state");
                return Ok(None);
            }
        };

        let state = TaskState::from_string(&state_str)?;

        // first heading on note is assumed to be the title
        let title = raw
            .content
            .lines()
            .filter(|line| line.starts_with("# "))
            .next()
            .context("unable to find first header to treat as title")?
            .strip_prefix("# ")
            .context("unable to remove prefix from title line")?
            .to_string();

        Ok(Some(Task { title, state, path }))
    }

    const fn ref_array(&self) -> [&String; 1] {
        [&self.title]
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn state(&self) -> &TaskState {
        &self.state
    }
}

fn tasks_from_vault(directory: PathBuf) -> Result<Vec<Task>, Error> {
    let note_paths = shared_lib::recurse_files(directory, false)?
        .iter()
        .filter_map(|path| {
            if path.extension().map_or(false, |ext| ext == "md") {
                Some(path)
            } else {
                None
            }
        })
        .filter_map(|path| {
            if path.file_name().map_or(false, |file_name| {
                file_name.to_string_lossy().starts_with(".")
            }) {
                None
            } else {
                Some(path.to_owned())
            }
        })
        .collect::<Vec<_>>();

    println!("{note_paths:?}");

    let tasks = note_paths
        .iter()
        .filter_map(|path| {
            let res = Task::from_path(path);
            match res {
                Ok(res) => Some(res),
                Err(res) => {
                    eprintln!("note at path {path:?} is malformed: {res}");
                    None
                }
            }
        })
        .flatten()
        .collect::<Vec<Task>>();
    for task in tasks.clone() {
        println!("Read YAML string: {:?}", task);
    }

    Ok(tasks)
}

fn main() -> Result<(), Error> {
    let cli = Cli::try_parse().unwrap_or_else(|error| error.exit());

    let directory = match cli.directory {
        Some(directory) => directory,
        None => std::env::current_dir().unwrap_or_default(),
    };

    tasks_from_vault(directory)?;

    color_eyre::install();
    ratatui::run(|terminal| App::new().run(terminal));

    Ok(())
}

use color_eyre::Result as eyre_Result;
use crossterm::event::{self, KeyCode, KeyModifiers};
use itertools::Itertools;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{self, Color, Modifier, Style, Stylize};
use ratatui::text::Text;
use ratatui::widgets::{
    Block, BorderType, Cell, HighlightSpacing, Paragraph, Row, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Table, TableState,
};
use ratatui::{DefaultTerminal, Frame};
use style::palette::tailwind;
use textwrap::{fill, wrap};
use unicode_width::UnicodeWidthStr;

const PALETTES: [tailwind::Palette; 4] = [
    tailwind::BLUE,
    tailwind::EMERALD,
    tailwind::INDIGO,
    tailwind::RED,
];
const INFO_TEXT: [&str; 2] = [
    "(Esc) quit | (↑) move up | (↓) move down | (←) move left | (→) move right",
    "(Shift + →) next color | (Shift + ←) previous color",
];

const ITEM_HEIGHT: usize = 4;

struct TableColors {
    buffer_bg: Color,
    header_bg: Color,
    header_fg: Color,
    row_fg: Color,
    selected_row_style_fg: Color,
    selected_column_style_fg: Color,
    selected_cell_style_fg: Color,
    normal_row_color: Color,
    alt_row_color: Color,
    footer_border_color: Color,
}

impl TableColors {
    const fn new(color: &tailwind::Palette) -> Self {
        Self {
            buffer_bg: tailwind::SLATE.c950,
            header_bg: color.c900,
            header_fg: tailwind::SLATE.c200,
            row_fg: tailwind::SLATE.c200,
            selected_row_style_fg: color.c400,
            selected_column_style_fg: color.c400,
            selected_cell_style_fg: color.c600,
            normal_row_color: tailwind::SLATE.c950,
            alt_row_color: tailwind::SLATE.c900,
            footer_border_color: color.c400,
        }
    }
}

struct App {
    open_state: TableState,
    inprogress_state: TableState,
    done_state: TableState,
    selected_table: SelectedTable,
    items: Vec<Task>,
    longest_item_lens: (u16, u16, u16), // order is (Open, InProgress, Done)
    scroll_state: ScrollbarState,
    colors: TableColors,
    color_index: usize,
}

enum SelectedTable {
    Open,
    InProgress,
    Done,
}

impl App {
    fn new() -> Self {
        let data_vec = generate_notes();
        Self {
            open_state: TableState::default().with_selected(0),
            inprogress_state: TableState::default(),
            done_state: TableState::default(),
            selected_table: SelectedTable::Open,
            longest_item_lens: constraint_len_calculator(&data_vec),
            scroll_state: ScrollbarState::new((data_vec.len() - 1) * ITEM_HEIGHT),
            colors: TableColors::new(&PALETTES[0]),
            color_index: 0,
            items: data_vec,
        }
    }

    pub const fn next_row(&mut self) {
        match self.selected_table {
            SelectedTable::Open => {
                let i = match self.open_state.selected() {
                    Some(i) => {
                        if i >= self.items.len() - 1 {
                            0
                        } else {
                            i + 1
                        }
                    }
                    None => 0,
                };
                self.open_state.select(Some(i));
                self.scroll_state = self.scroll_state.position(i * ITEM_HEIGHT);
            }
            SelectedTable::InProgress => {
                let i = match self.inprogress_state.selected() {
                    Some(i) => {
                        if i >= self.items.len() - 1 {
                            0
                        } else {
                            i + 1
                        }
                    }
                    None => 0,
                };
                self.inprogress_state.select(Some(i));
                self.scroll_state = self.scroll_state.position(i * ITEM_HEIGHT);
            }
            SelectedTable::Done => {
                let i = match self.done_state.selected() {
                    Some(i) => {
                        if i >= self.items.len() - 1 {
                            0
                        } else {
                            i + 1
                        }
                    }
                    None => 0,
                };
                self.done_state.select(Some(i));
                self.scroll_state = self.scroll_state.position(i * ITEM_HEIGHT);
            }
        }
    }

    pub const fn previous_row(&mut self) {
        match self.selected_table {
            SelectedTable::Open => {
                let i = match self.open_state.selected() {
                    Some(i) => {
                        if i == 0 {
                            self.items.len() - 1
                        } else {
                            i - 1
                        }
                    }
                    None => 0,
                };
                self.open_state.select(Some(i));
                self.scroll_state = self.scroll_state.position(i * ITEM_HEIGHT);
            }
            SelectedTable::InProgress => {
                let i = match self.inprogress_state.selected() {
                    Some(i) => {
                        if i == 0 {
                            self.items.len() - 1
                        } else {
                            i - 1
                        }
                    }
                    None => 0,
                };
                self.inprogress_state.select(Some(i));
                self.scroll_state = self.scroll_state.position(i * ITEM_HEIGHT);
            }
            SelectedTable::Done => {
                let i = match self.done_state.selected() {
                    Some(i) => {
                        if i == 0 {
                            self.items.len() - 1
                        } else {
                            i - 1
                        }
                    }
                    None => 0,
                };
                self.done_state.select(Some(i));
                self.scroll_state = self.scroll_state.position(i * ITEM_HEIGHT);
            }
        }
    }

    pub fn next_column(&mut self) {
        match self.selected_table {
            SelectedTable::Open => {
                self.inprogress_state.select(self.open_state.selected());
                self.open_state.select(None);
                self.selected_table = SelectedTable::InProgress;
            }
            SelectedTable::InProgress => {
                self.done_state.select(self.inprogress_state.selected());
                self.inprogress_state.select(None);
                self.selected_table = SelectedTable::Done;
            }
            SelectedTable::Done => {
                self.open_state.select(self.done_state.selected());
                self.done_state.select(None);
                self.selected_table = SelectedTable::Open;
            }
        }
    }

    pub fn previous_column(&mut self) {
        match self.selected_table {
            SelectedTable::Open => {
                self.done_state.select(self.open_state.selected());
                self.open_state.select(None);
                self.selected_table = SelectedTable::Done;
            }
            SelectedTable::InProgress => {
                self.open_state.select(self.inprogress_state.selected());
                self.inprogress_state.select(None);
                self.selected_table = SelectedTable::Open;
            }
            SelectedTable::Done => {
                self.inprogress_state.select(self.done_state.selected());
                self.done_state.select(None);
                self.selected_table = SelectedTable::InProgress;
            }
        }
    }

    pub const fn next_color(&mut self) {
        self.color_index = (self.color_index + 1) % PALETTES.len();
    }

    pub const fn previous_color(&mut self) {
        let count = PALETTES.len();
        self.color_index = (self.color_index + count - 1) % count;
    }

    pub const fn set_colors(&mut self) {
        self.colors = TableColors::new(&PALETTES[self.color_index]);
    }

    fn run(mut self, terminal: &mut DefaultTerminal) -> eyre_Result<()> {
        loop {
            terminal.draw(|frame| self.render(frame))?;

            if let Some(key) = event::read()?.as_key_press_event() {
                let shift_pressed = key.modifiers.contains(KeyModifiers::SHIFT);
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Char('j') | KeyCode::Down => self.next_row(),
                    KeyCode::Char('k') | KeyCode::Up => self.previous_row(),
                    KeyCode::Char('L') | KeyCode::Right if shift_pressed => self.next_color(),
                    KeyCode::Char('H') | KeyCode::Left if shift_pressed => {
                        self.previous_color();
                    }
                    KeyCode::Char('l') | KeyCode::Right => self.next_column(),
                    KeyCode::Char('h') | KeyCode::Left => self.previous_column(),
                    _ => {}
                }
            }
        }
    }

    fn render(&mut self, frame: &mut Frame) {
        let outer_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Min(5), Constraint::Length(4)])
            .split(frame.area());

        let inner_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![
                Constraint::Percentage(33),
                Constraint::Min(10),
                Constraint::Percentage(33),
            ])
            .split(outer_layout[0]);

        self.set_colors();

        self.render_table(frame, inner_layout[0], TaskState::Open);
        self.render_table(frame, inner_layout[1], TaskState::InProgress);
        self.render_table(frame, inner_layout[2], TaskState::Done);

        // self.render_scrollbar(frame, outer_layout[0]);
        self.render_footer(frame, outer_layout[1]);
    }

    fn render_table(&mut self, frame: &mut Frame, area: Rect, task_state: TaskState) {
        let header_style = Style::default()
            .fg(self.colors.header_fg)
            .bg(self.colors.header_bg);
        let selected_row_style = Style::default()
            .add_modifier(Modifier::REVERSED)
            .fg(self.colors.selected_row_style_fg);
        let selected_col_style = Style::default().fg(self.colors.selected_column_style_fg);
        let selected_cell_style = Style::default()
            .add_modifier(Modifier::REVERSED)
            .fg(self.colors.selected_cell_style_fg);

        let head = match task_state {
            TaskState::Open => "Open",
            TaskState::InProgress => "In Progress",
            TaskState::Done => "Done",
        };

        let header = [head]
            .into_iter()
            .map(Cell::from)
            .collect::<Row>()
            .style(header_style)
            .height(1);
        let width = match task_state {
            TaskState::Open => self.longest_item_lens.0 + 1,
            TaskState::InProgress => self.longest_item_lens.1 + 1,
            TaskState::Done => self.longest_item_lens.2 + 1,
        };
        let rows = self
            .items
            .iter()
            .enumerate()
            .filter(|task| task.1.state.clone() == task_state)
            .map(|(i, data)| {
                let color = match i % 2 {
                    0 => self.colors.normal_row_color,
                    _ => self.colors.alt_row_color,
                };
                let item = data.ref_array();
                item.into_iter()
                    .map(|content| {
                        let wrapped_lines = fill(content, 30);
                        Cell::from(Text::from(format!("\n{wrapped_lines}\n")))
                    })
                    .map(|cell| cell)
                    .collect::<Row>()
                    .style(Style::new().fg(self.colors.row_fg).bg(color))
                    .height(4)
            });
        let bar = " █ ";
        let t = Table::new(
            rows,
            [
                // + 1 is for padding.
                Constraint::Min(width),
            ],
        )
        .header(header)
        .row_highlight_style(selected_row_style)
        .column_highlight_style(selected_col_style)
        .cell_highlight_style(selected_cell_style)
        .highlight_symbol(Text::from(vec![
            "".into(),
            bar.into(),
            bar.into(),
            "".into(),
        ]))
        .bg(self.colors.buffer_bg)
        .highlight_spacing(HighlightSpacing::Always);
        match task_state {
            TaskState::Open => frame.render_stateful_widget(t, area, &mut self.open_state),
            TaskState::InProgress => {
                frame.render_stateful_widget(t, area, &mut self.inprogress_state)
            }
            TaskState::Done => frame.render_stateful_widget(t, area, &mut self.done_state),
        }
    }

    fn render_scrollbar(&mut self, frame: &mut Frame, area: Rect) {
        frame.render_stateful_widget(
            Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None),
            area.inner(Margin {
                vertical: 1,
                horizontal: 1,
            }),
            &mut self.scroll_state,
        );
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let info_footer = Paragraph::new(Text::from_iter(INFO_TEXT))
            .style(
                Style::new()
                    .fg(self.colors.row_fg)
                    .bg(self.colors.buffer_bg),
            )
            .centered()
            .block(
                Block::bordered()
                    .border_type(BorderType::Double)
                    .border_style(Style::new().fg(self.colors.footer_border_color)),
            );
        frame.render_widget(info_footer, area);
    }
}

fn generate_notes() -> Vec<Task> {
    let mut new_task = Task {
        title: "Drifter is such a booty dog yahoo toodly doo".to_string(),
        state: TaskState::InProgress,
        path: Path::new("/fake/path").to_path_buf(),
    };

    let mut dummy: Vec<Task> = vec![new_task.clone()];

    new_task.title = "Victor is for fujos especially Paige who likes to yaoi".to_string();
    dummy.push(new_task.clone());

    new_task.state = TaskState::Open;
    new_task.title = "Silver is great, for obvious reasons I think".to_string();
    dummy.push(new_task.clone());

    new_task.state = TaskState::Done;
    new_task.title = "Abrams is tanky like everybody else with good cc".to_string();
    dummy.push(new_task.clone());

    dummy
}

fn constraint_len_calculator(items: &[Task]) -> (u16, u16, u16) {
    let open_len = items
        .iter()
        .filter(|task| matches!(task.state, TaskState::Open))
        .map(Task::title)
        .flat_map(str::lines)
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0);
    let inprogress_len = items
        .iter()
        .filter(|task| matches!(task.state, TaskState::InProgress))
        .map(Task::title)
        .flat_map(str::lines)
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0);
    let done_len = items
        .iter()
        .filter(|task| matches!(task.state, TaskState::Done))
        .map(Task::title)
        .flat_map(str::lines)
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0);

    #[expect(clippy::cast_possible_truncation)]
    (open_len as u16, inprogress_len as u16, done_len as u16)
}

// #[cfg(test)]
// mod tests {
//     use crate::Data;

//     #[test]
//     fn constraint_len_calculator() {
//         let test_data = vec![
//             Data {
//                 name: "Emirhan Tala".to_string(),
//                 address: "Cambridgelaan 6XX\n3584 XX Utrecht".to_string(),
//                 email: "tala.emirhan@gmail.com".to_string(),
//             },
//             Data {
//                 name: "thistextis26characterslong".to_string(),
//                 address: "this line is 31 characters long\nbottom line is 33 characters long"
//                     .to_string(),
//                 email: "thisemailis40caharacterslong@ratatui.com".to_string(),
//             },
//         ];
//         let longest_title_len =
//             crate::constraint_len_calculator(&test_data);

//         assert_eq!(26, longest_name_len);
//         assert_eq!(33, longest_address_len);
//         assert_eq!(40, longest_email_len);
//     }
// }

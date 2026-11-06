use anyhow::{Context, Error, Result, anyhow};
use clap::Parser;
use std::ops::DerefMut;
use std::{path::Path, path::PathBuf, process::Command};

mod note;
mod settings;
mod task;

use task::{Task, TaskState};

#[derive(Parser)]
#[command(version)]
struct Cli {
    /// path to the root of the vault, otherwise assumes we are in the vault
    #[arg(short, long)]
    config: Option<PathBuf>,
    #[clap(long, short)]
    // directory in the vault, used to limit results to just a subdirectory
    // optional
    subdir: Option<String>,
    #[clap(long, short)]
    exclude_subdir: Option<Vec<String>>,
}

fn main() -> Result<(), Error> {
    let cli = Cli::try_parse().unwrap_or_else(|error| error.exit());
    let conf = settings::load_config(cli.config.as_deref())?;

    let directory = match cli.subdir {
        None => conf.vault_directory.clone(),
        Some(dir) => conf.vault_directory.join(dir).clone(),
    };

    let excuded_subdir = match cli.exclude_subdir {
        None => vec![],
        Some(dirs) => dirs.iter().map(|dir| directory.join(dir)).collect(),
    };

    color_eyre::install();
    while ratatui::run(|terminal| {
        App::new(
            conf.clone(),
            Some(directory.clone()),
            Some(excuded_subdir.clone()),
        )
        .run(terminal)
    })? {
        // do nothing
    }

    Ok(())
}

use color_eyre::Result as eyre_Result;
use crossterm::event::{self, KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{self, Color, Modifier, Style, Stylize};
use ratatui::text::Text;
use ratatui::widgets::{
    Block, BorderType, Cell, Clear, HighlightSpacing, Paragraph, Row, Scrollbar,
    ScrollbarOrientation, ScrollbarState, Table, TableState,
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
    "(Esc) quit | (Enter) launch editor |(↑) move up | (↓) move down | (←) move left | (→) move right",
    "(/) filter | (Shift + →) next color | (Shift + ←) previous color",
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
    show_search: bool,
    conf: settings::Settings,
    directory: Option<PathBuf>,
    open_state: TableState,
    open_items: Vec<Task>,
    inprogress_state: TableState,
    inprogress_items: Vec<Task>,
    done_state: TableState,
    done_items: Vec<Task>,
    selected_table: SelectedTable,
    items: Vec<task::Task>,
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

fn enumerate_items(items: &Vec<Task>, task_state: task::TaskState) -> Vec<Task> {
    items
        .iter()
        .filter(|task| task.state == task_state)
        .cloned()
        .collect()
}

impl App {
    fn new(
        conf: settings::Settings,
        directory: Option<PathBuf>,
        excluded_subdirs: Option<Vec<PathBuf>>,
    ) -> Self {
        let data_vec = match directory.clone() {
            Some(p) => {
                let tasks = task::tasks_from_vault(p, excluded_subdirs);
                match tasks {
                    Ok(v) => v,
                    Err(_) => generate_notes(),
                }
            }
            None => generate_notes(),
        };
        assert!(data_vec.len() > 0, "no entries found!");
        Self {
            conf,
            show_search: false,
            directory: directory,
            open_state: TableState::default().with_selected(0),
            open_items: enumerate_items(&data_vec, task::TaskState::Open),
            inprogress_state: TableState::default(),
            inprogress_items: enumerate_items(&data_vec, task::TaskState::InProgress),
            done_state: TableState::default(),
            done_items: enumerate_items(&data_vec, task::TaskState::Done),
            selected_table: SelectedTable::Open,
            longest_item_lens: constraint_len_calculator(&data_vec),
            scroll_state: ScrollbarState::new((data_vec.len() - 1) * ITEM_HEIGHT),
            colors: TableColors::new(&PALETTES[0]),
            color_index: 0,
            items: data_vec,
        }
    }

    pub fn run_editor(&self) -> Result<(), Error> {
        Command::new("hx")
            .arg("--working-dir")
            .arg(self.conf.vault_directory.clone())
            .arg(self.current_selected_item()?.path.clone())
            .status()
            .context("failed running helix")?;

        Ok(())
    }

    pub fn current_selected_item(&self) -> Result<&Task, anyhow::Error> {
        match self.selected_table {
            SelectedTable::Open => Ok(self
                .open_items
                .get(
                    self.open_state
                        .selected()
                        .context("invalid selected item index in open_state")?,
                )
                .context("no item for index in open_items")?),
            SelectedTable::InProgress => Ok(self
                .inprogress_items
                .get(
                    self.inprogress_state
                        .selected()
                        .context("invalid selected item index in inprogress_state")?,
                )
                .context("no item for index in inprogress_items")?),
            SelectedTable::Done => Ok(self
                .done_items
                .get(
                    self.done_state
                        .selected()
                        .context("invalid selected item index in done_state")?,
                )
                .context("no item for index in done_items")?),
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

    fn run(mut self, terminal: &mut DefaultTerminal) -> eyre_Result<bool, Error> {
        loop {
            terminal.draw(|frame| self.render(frame))?;

            if let Some(key) = event::read()?.as_key_press_event() {
                let shift_pressed = key.modifiers.contains(KeyModifiers::SHIFT);
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(false),
                    KeyCode::Char('j') | KeyCode::Down => self.next_row(),
                    KeyCode::Char('k') | KeyCode::Up => self.previous_row(),
                    KeyCode::Char('L') | KeyCode::Right if shift_pressed => self.next_color(),
                    KeyCode::Char('H') | KeyCode::Left if shift_pressed => {
                        self.previous_color();
                    }
                    KeyCode::Char('l') | KeyCode::Right => self.next_column(),
                    KeyCode::Char('h') | KeyCode::Left => self.previous_column(),
                    KeyCode::Char('/') => self.show_search = !self.show_search,
                    KeyCode::Enter => {
                        // getting ratatui to redraw everything is hard
                        // and we would need to refresh for file changes anyway
                        // so lets be lazy and just die and re-open ourselves if on editor exit
                        // https://github.com/ratatui/ratatui/issues/1606
                        // TODO: it would be nice if the row/colomn index is remembered between editor cycles
                        self.run_editor()?;
                        return Ok(true);
                    }
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

        self.render_table(frame, inner_layout[0], task::TaskState::Open);
        self.render_table(frame, inner_layout[1], task::TaskState::InProgress);
        self.render_table(frame, inner_layout[2], task::TaskState::Done);

        // self.render_scrollbar(frame, outer_layout[0]);
        self.render_footer(frame, outer_layout[1]);

        if self.show_search {
            let popup_block = Block::bordered().title("Filter");
            let centered_area = frame
                .area()
                .centered(Constraint::Percentage(60), Constraint::Percentage(40));
            // clears out any background in the area before rendering the popup

            let search_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints(vec![Constraint::Length(4), Constraint::Min(5)])
                .split(centered_area);

            frame.render_widget(Clear, centered_area);
            let paragraph = Paragraph::new("Lorem ipsum").block(popup_block).style(
                Style::new()
                    .fg(self.colors.row_fg)
                    .bg(self.colors.alt_row_color),
            );
            frame.render_widget(paragraph.clone(), search_layout[0]);
            frame.render_widget(paragraph, search_layout[1]);

            // another solution is to use the inner area of the block
            // let inner_area = popup_block.inner(centered_area);
            // frame.render_widget(your_widget, inner_area);
        }
    }

    fn render_table(&mut self, frame: &mut Frame, area: Rect, task_state: task::TaskState) {
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
            task::TaskState::Open => "Open",
            task::TaskState::InProgress => "In Progress",
            task::TaskState::Done => "Done",
        };

        let header = [head]
            .into_iter()
            .map(Cell::from)
            .collect::<Row>()
            .style(header_style)
            .height(1);

        //TODO: table state and items should be stored in a struct together so we can pass that it, or look this up once
        let width = match task_state {
            task::TaskState::Open => self.longest_item_lens.0 + 1,
            task::TaskState::InProgress => self.longest_item_lens.1 + 1,
            task::TaskState::Done => self.longest_item_lens.2 + 1,
        };
        let table_items = match task_state {
            task::TaskState::Open => &self.open_items,
            task::TaskState::InProgress => &self.inprogress_items,
            task::TaskState::Done => &self.done_items,
        };

        let rows = table_items.iter().enumerate().map(|(i, data)| {
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
        properties: None,
        tags: vec![],
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

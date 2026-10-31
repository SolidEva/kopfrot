use clap::Parser;
use std::{fs, path::PathBuf};

use anyhow::{Context, Error, Result};
use chrono::Local;

#[derive(Parser)]
#[command(version)]
struct Cli {
    /// path to the root of the vault, otherwise assumes we are in the vault
    #[arg(short, long)]
    directory: Option<PathBuf>,
    #[clap(long, short)]
    shared: bool,
    notetype: String,
    title: String,
}

#[derive(Clone)]
enum TaskState {
    Open,
    InProgress,
    Done,
}

#[derive(Clone)]
struct Task {
    title: String,
    state: TaskState,
}

impl Task {
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

fn main() -> Result<(), Error> {
    color_eyre::install();
    ratatui::run(|terminal| App::new().run(terminal));

    let cli = Cli::try_parse().unwrap_or_else(|error| error.exit());

    let directory = match cli.directory {
        Some(directory) => directory,
        None => std::env::current_dir().unwrap_or_default(),
    };

    let template_dir = directory.join("shared/.templates");
    template_dir
        .try_exists()
        .context(format!("{template_dir:?} does not exist"))?;
    let template_file = template_dir.join(format!(".{}.template", cli.notetype));
    template_file
        .try_exists()
        .context(format!("{template_file:?} does not exist"))?;

    let template = fs::read_to_string(template_file.clone())
        .context(format!("unable to read {template_file:?}"))?;
    let out = template
        .replace("{{title}}", cli.title.as_ref())
        .replace("{{date}}", &Local::now().format("%Y-%m-%d").to_string());

    if cli.shared {
        let out_path = directory
            .join("shared")
            .join(format!("\"{}.md\"", cli.title));
        fs::write(out_path.clone(), out)?;
        print!("{out_path:?}");
    } else {
        let out_path = directory.join(format!("{}.md", cli.title));
        fs::write(out_path.clone(), out)?;
        print!("{out_path:?}");
    }

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
    state: TableState,
    items: Vec<Task>,
    longest_item_lens: (u16, u16, u16), // order is (Open, InProgress, Done)
    scroll_state: ScrollbarState,
    colors: TableColors,
    color_index: usize,
}

impl App {
    fn new() -> Self {
        let data_vec = generate_notes();
        Self {
            state: TableState::default().with_selected(0),
            longest_item_lens: constraint_len_calculator(&data_vec),
            scroll_state: ScrollbarState::new((data_vec.len() - 1) * ITEM_HEIGHT),
            colors: TableColors::new(&PALETTES[0]),
            color_index: 0,
            items: data_vec,
        }
    }

    pub const fn next_row(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
        self.scroll_state = self.scroll_state.position(i * ITEM_HEIGHT);
    }

    pub const fn previous_row(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
        self.scroll_state = self.scroll_state.position(i * ITEM_HEIGHT);
    }

    pub fn next_column(&mut self) {
        self.state.select_next_column();
    }

    pub fn previous_column(&mut self) {
        self.state.select_previous_column();
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

        // let layout = Layout::vertical([Constraint::Min(5), Constraint::Length(4)]);
        // let rects = frame.area().layout_vec(&outer_layout);

        self.set_colors();

        self.render_open(frame, inner_layout[0]);
        self.render_inprogress(frame, inner_layout[1]);
        self.render_done(frame, inner_layout[2]);

        // self.render_table(frame, rects[0]);
        // self.render_scrollbar(frame, rects[0]);
        self.render_footer(frame, outer_layout[1]);
    }
    fn render_open(&mut self, frame: &mut Frame, area: Rect) {
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

        let header = ["Open"]
            .into_iter()
            .map(Cell::from)
            .collect::<Row>()
            .style(header_style)
            .height(1);
        let rows = self
            .items
            .iter()
            .enumerate()
            .filter(|task| matches!(task.1.state, TaskState::Open))
            .map(|(i, data)| {
                let color = match i % 2 {
                    0 => self.colors.normal_row_color,
                    _ => self.colors.alt_row_color,
                };
                let item = data.ref_array();
                item.into_iter()
                    .map(|content| Cell::from(Text::from(format!("\n{content}\n"))))
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
                Constraint::Min(self.longest_item_lens.0 + 1),
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
        frame.render_stateful_widget(t, area, &mut self.state);
    }

    fn render_inprogress(&mut self, frame: &mut Frame, area: Rect) {
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

        let header = ["In Progress"]
            .into_iter()
            .map(Cell::from)
            .collect::<Row>()
            .style(header_style)
            .height(1);
        let rows = self
            .items
            .iter()
            .enumerate()
            .filter(|task| matches!(task.1.state, TaskState::InProgress))
            .map(|(i, data)| {
                let color = match i % 2 {
                    0 => self.colors.normal_row_color,
                    _ => self.colors.alt_row_color,
                };
                let item = data.ref_array();
                item.into_iter()
                    .map(|content| Cell::from(Text::from(format!("\n{content}\n"))))
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
                Constraint::Min(self.longest_item_lens.1 + 1),
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
        frame.render_stateful_widget(t, area, &mut self.state);
    }

    fn render_done(&mut self, frame: &mut Frame, area: Rect) {
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

        let header = ["Done"]
            .into_iter()
            .map(Cell::from)
            .collect::<Row>()
            .style(header_style)
            .height(1);
        let rows = self
            .items
            .iter()
            .enumerate()
            .filter(|task| matches!(task.1.state, TaskState::Done))
            .map(|(i, data)| {
                let color = match i % 2 {
                    0 => self.colors.normal_row_color,
                    _ => self.colors.alt_row_color,
                };
                let item = data.ref_array();
                item.into_iter()
                    .map(|content| Cell::from(Text::from(format!("\n{content}\n"))))
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
                Constraint::Min(self.longest_item_lens.2 + 1),
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
        frame.render_stateful_widget(t, area, &mut self.state);
    }
    // fn render_table(&mut self, frame: &mut Frame, area: Rect) {
    //     let header_style = Style::default()
    //         .fg(self.colors.header_fg)
    //         .bg(self.colors.header_bg);
    //     let selected_row_style = Style::default()
    //         .add_modifier(Modifier::REVERSED)
    //         .fg(self.colors.selected_row_style_fg);
    //     let selected_col_style = Style::default().fg(self.colors.selected_column_style_fg);
    //     let selected_cell_style = Style::default()
    //         .add_modifier(Modifier::REVERSED)
    //         .fg(self.colors.selected_cell_style_fg);

    //     let header = ["Open", "In Progress", "Done"]
    //         .into_iter()
    //         .map(Cell::from)
    //         .collect::<Row>()
    //         .style(header_style)
    //         .height(1);
    //     let rows = self.items.iter().enumerate().map(|(i, data)| {
    //         let color = match i % 2 {
    //             0 => self.colors.normal_row_color,
    //             _ => self.colors.alt_row_color,
    //         };
    //         let item = data.ref_array();
    //         item.into_iter()
    //             .map(|content| Cell::from(Text::from(format!("\n{content}\n"))))
    //             .map(|cell| cell)
    //             .collect::<Row>()
    //             .style(Style::new().fg(self.colors.row_fg).bg(color))
    //             .height(4)
    //     });
    //     let bar = " █ ";
    //     let t = Table::new(
    //         rows,
    //         [
    //             // + 1 is for padding.
    //             Constraint::Min(self.longest_item_lens.0 + 1),
    //             Constraint::Min(self.longest_item_lens.1 + 1),
    //             Constraint::Length(self.longest_item_lens.2 + 1),
    //         ],
    //     )
    //     .header(header)
    //     .row_highlight_style(selected_row_style)
    //     .column_highlight_style(selected_col_style)
    //     .cell_highlight_style(selected_cell_style)
    //     .highlight_symbol(Text::from(vec![
    //         "".into(),
    //         bar.into(),
    //         bar.into(),
    //         "".into(),
    //     ]))
    //     .bg(self.colors.buffer_bg)
    //     .highlight_spacing(HighlightSpacing::Always);
    //     frame.render_stateful_widget(t, area, &mut self.state);
    // }

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
        title: "Drifter".to_string(),
        state: TaskState::InProgress,
    };

    let mut dummy: Vec<Task> = vec![new_task.clone()];

    new_task.title = "Victor".to_string();
    dummy.push(new_task.clone());

    new_task.state = TaskState::Open;
    new_task.title = "Silver".to_string();
    dummy.push(new_task.clone());

    new_task.state = TaskState::Done;
    new_task.title = "Abrams".to_string();
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

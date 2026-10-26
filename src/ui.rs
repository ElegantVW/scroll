use crate::model::{book_pages, egg_state, load_data, Leaf, Page};
use crate::path_index::{entries, filtered, PathEntry};
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers, MouseButton,
    MouseEventKind,
};
use crossterm::terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, queue};
use std::io::{self, IsTerminal, Write};
use std::process::Command;
use std::time::{Duration, Instant};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

enum TerminalOutput {
    Stdout(io::Stdout),
    #[cfg(unix)]
    Controlling(std::fs::File),
}

impl TerminalOutput {
    fn open() -> io::Result<Self> {
        if io::stdout().is_terminal() {
            return Ok(Self::Stdout(io::stdout()));
        }
        #[cfg(unix)]
        {
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open("/dev/tty")
                .map(Self::Controlling)
        }
        #[cfg(not(unix))]
        Err(io::Error::new(
            io::ErrorKind::NotConnected,
            "no interactive terminal",
        ))
    }

    fn size(&self) -> io::Result<(u16, u16)> {
        match self {
            Self::Stdout(_) => crossterm::terminal::size(),
            #[cfg(unix)]
            Self::Controlling(file) => terminal_size(file),
        }
    }
}

impl Write for TerminalOutput {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Stdout(stdout) => stdout.write(buf),
            #[cfg(unix)]
            Self::Controlling(file) => file.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Stdout(stdout) => stdout.flush(),
            #[cfg(unix)]
            Self::Controlling(file) => file.flush(),
        }
    }
}

#[cfg(target_os = "linux")]
fn terminal_size(file: &std::fs::File) -> io::Result<(u16, u16)> {
    use std::os::fd::AsRawFd;

    #[repr(C)]
    struct WindowSize {
        rows: u16,
        columns: u16,
        x_pixels: u16,
        y_pixels: u16,
    }

    unsafe extern "C" {
        fn ioctl(fd: i32, request: usize, ...) -> i32;
    }
    const TIOCGWINSZ: usize = 0x5413;
    let mut size = WindowSize {
        rows: 0,
        columns: 0,
        x_pixels: 0,
        y_pixels: 0,
    };
    let result = unsafe { ioctl(file.as_raw_fd(), TIOCGWINSZ, &mut size) };
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok((size.columns, size.rows))
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
fn terminal_size(_file: &std::fs::File) -> io::Result<(u16, u16)> {
    crossterm::terminal::size()
}

pub enum UiResult {
    Quit,
    Fallback,
    PrintName(String),
    RunCommand(String),
}

pub struct TerminalGuard {
    output: TerminalOutput,
}

impl TerminalGuard {
    pub fn enter() -> io::Result<Self> {
        let mut output = TerminalOutput::open()?;
        terminal::enable_raw_mode()?;
        if let Err(error) = execute!(
            &mut output,
            EnterAlternateScreen,
            EnableMouseCapture,
            Hide,
            Clear(ClearType::All)
        ) {
            let _ = terminal::disable_raw_mode();
            return Err(error);
        }
        Ok(Self { output })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(
            &mut self.output,
            Show,
            DisableMouseCapture,
            LeaveAlternateScreen,
            Clear(ClearType::All)
        );
        let _ = terminal::disable_raw_mode();
    }
}

#[derive(Clone, Copy)]
enum Hit {
    Rune(usize),
    Path(usize),
    Index(usize),
}

#[derive(Clone, Copy)]
struct HitBox {
    x0: u16,
    x1: u16,
    y: u16,
    hit: Hit,
}

struct IndexMode {
    query: String,
    selected: usize,
}

struct Screen {
    lines: Vec<String>,
    hits: Vec<HitBox>,
    page_content_offset: usize,
    visible_path_count: usize,
    index_result_count: usize,
}

pub fn is_terminal() -> bool {
    io::stdin().is_terminal() && TerminalOutput::open().is_ok()
}

pub fn print_list() {
    let data = load_data();
    let leaves = book_pages(&data, egg_state());
    for leaf in leaves {
        match leaf {
            Leaf::Path => println!("[PATH]  summon / scroll --path — print name to prompt"),
            Leaf::App(page) => {
                println!("✦ {}  ({})", page.name, page.domain);
                println!("  {}", page.tagline);
                for line in page.intro {
                    println!("  {line}");
                }
                println!("  · How ·");
                for line in page.how {
                    println!("    {line}");
                }
                println!("  · CLI ·");
                for item in page.cli {
                    println!("    {}  —  {}", item.command, item.description);
                }
                println!();
            }
        }
    }
}

pub fn run(start_path: bool, initial_filter: String, mut ambience: Ambience) -> UiResult {
    let data = load_data();
    let entries = entries();
    let mut guard = match TerminalGuard::enter() {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("scroll: could not open interactive terminal: {error}");
            return UiResult::Fallback;
        }
    };
    let mut page_index = 0usize;
    let mut page_offset = 0usize;
    let mut path_filter = initial_filter;
    let mut path_selected = 0usize;
    let mut jump_mode = None;
    let mut index_mode: Option<IndexMode> = None;
    let mut last_path_click: Option<(usize, Instant)> = None;
    if start_path {
        page_index = book_pages(&data, egg_state()).len().saturating_sub(1);
    }
    ambience.start();

    let outcome = 'ui: loop {
        let state = egg_state();
        let leaves = book_pages(&data, state);
        page_index = page_index.min(leaves.len().saturating_sub(1));
        let (columns, rows) = guard.output.size().unwrap_or((80, 24));
        let screen = render(
            &leaves,
            page_index,
            page_offset,
            &path_filter,
            path_selected,
            index_mode.as_ref(),
            &entries,
            ambience.status(),
            columns,
            rows,
        );
        page_offset = screen.page_content_offset;
        path_selected = path_selected.min(screen.visible_path_count.saturating_sub(1));
        if let Some(index) = index_mode.as_mut() {
            index.selected = index
                .selected
                .min(screen.index_result_count.saturating_sub(1));
        }
        draw(&mut guard.output, &screen.lines);

        if !event::poll(Duration::from_millis(150)).unwrap_or(false) {
            continue;
        }
        let Ok(event) = event::read() else {
            continue;
        };
        match event {
            Event::Mouse(mouse) => {
                jump_mode = None;
                match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        if leaves[page_index].is_path() {
                            path_selected = path_selected.saturating_sub(3);
                        } else {
                            page_offset = page_offset.saturating_sub(3);
                        }
                    }
                    MouseEventKind::ScrollDown => {
                        if leaves[page_index].is_path() {
                            path_selected = path_selected.saturating_add(3);
                        } else {
                            page_offset = page_offset.saturating_add(3);
                        }
                    }
                    MouseEventKind::Down(MouseButton::Left) => {
                        if let Some(hit) = screen.hits.iter().find(|hit| {
                            mouse.column >= hit.x0 && mouse.column <= hit.x1 && mouse.row == hit.y
                        }) {
                            match hit.hit {
                                Hit::Rune(index) => {
                                    if let Some(page) = leaves[page_index].app_page() {
                                        if let Some(rune) = page.castable_runes().get(index) {
                                            if !rune.command.is_empty() {
                                                break 'ui UiResult::RunCommand(
                                                    rune.command.clone(),
                                                );
                                            }
                                        }
                                    }
                                }
                                Hit::Path(index) => {
                                    let matches = filtered(&entries, &path_filter);
                                    if let Some(entry) = matches.get(index) {
                                        path_selected = index;
                                        let is_double = last_path_click
                                            .map(|(previous, at)| {
                                                previous == index
                                                    && at.elapsed() <= Duration::from_millis(350)
                                            })
                                            .unwrap_or(false);
                                        last_path_click = Some((index, Instant::now()));
                                        if is_double {
                                            break 'ui UiResult::PrintName(entry.name.clone());
                                        }
                                    }
                                }
                                Hit::Index(index) => {
                                    if let Some(mode) = index_mode.as_mut() {
                                        mode.selected = index;
                                        let results =
                                            crate::model::search_indices(&leaves, &mode.query);
                                        if let Some(target) = results.get(index) {
                                            page_index = *target;
                                            page_offset = 0;
                                            path_filter.clear();
                                            index_mode = None;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Event::Key(key) => {
                if let Some(mode) = index_mode.as_mut() {
                    match key.code {
                        KeyCode::Esc => index_mode = None,
                        KeyCode::Enter => {
                            let results = crate::model::search_indices(&leaves, &mode.query);
                            if let Some(target) = results.get(mode.selected) {
                                page_index = *target;
                                page_offset = 0;
                                path_filter.clear();
                                index_mode = None;
                            }
                        }
                        KeyCode::Up => mode.selected = mode.selected.saturating_sub(1),
                        KeyCode::Down => mode.selected = mode.selected.saturating_add(1),
                        KeyCode::Backspace => {
                            mode.query.pop();
                            mode.selected = 0;
                        }
                        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            mode.query.clear();
                            mode.selected = 0;
                        }
                        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                            mode.query.push(ch);
                            mode.selected = 0;
                        }
                        _ => {}
                    }
                    continue;
                }

                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    break 'ui UiResult::Quit;
                }
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('f') {
                    index_mode = Some(IndexMode {
                        query: String::new(),
                        selected: 0,
                    });
                    continue;
                }
                if let (Some(mode), KeyCode::Char(digit)) = (jump_mode, key.code) {
                    if digit.is_ascii_digit() {
                        page_index = crate::model::page_jump(mode, digit, leaves.len())
                            .unwrap_or(page_index);
                        page_offset = 0;
                        path_filter.clear();
                        jump_mode = None;
                        continue;
                    }
                }
                jump_mode = None;
                match key.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') => break 'ui UiResult::Quit,
                    KeyCode::Char('g') => jump_mode = Some('g'),
                    KeyCode::Char('G') => jump_mode = Some('G'),
                    KeyCode::Char('m') if !leaves[page_index].is_path() => ambience.toggle(),
                    KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        ambience.toggle()
                    }
                    KeyCode::Tab => {
                        page_index = (page_index + 1) % leaves.len();
                        page_offset = 0;
                        path_filter.clear();
                        path_selected = 0;
                    }
                    KeyCode::BackTab => {
                        page_index = page_index.checked_sub(1).unwrap_or(leaves.len() - 1);
                        page_offset = 0;
                        path_filter.clear();
                        path_selected = 0;
                    }
                    KeyCode::Up | KeyCode::Char('k') if leaves[page_index].is_path() => {
                        path_selected = path_selected.saturating_sub(1)
                    }
                    KeyCode::Down | KeyCode::Char('j') if leaves[page_index].is_path() => {
                        path_selected = path_selected.saturating_add(1)
                    }
                    KeyCode::Up | KeyCode::Char('k') => page_offset = page_offset.saturating_sub(1),
                    KeyCode::Down | KeyCode::Char('j') => {
                        page_offset = page_offset.saturating_add(1)
                    }
                    KeyCode::Enter if leaves[page_index].is_path() => {
                        if let Some(entry) = filtered(&entries, &path_filter).get(path_selected) {
                            break 'ui UiResult::PrintName(entry.name.clone());
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(page) = leaves[page_index].app_page() {
                            if let Some(rune) = page.castable_runes().first() {
                                break 'ui UiResult::RunCommand(rune.command.clone());
                            }
                        }
                    }
                    KeyCode::Backspace if leaves[page_index].is_path() => {
                        path_filter.pop();
                        path_selected = 0;
                    }
                    KeyCode::Char('u')
                        if leaves[page_index].is_path()
                            && key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        path_filter.clear();
                        path_selected = 0;
                    }
                    KeyCode::Char(ch)
                        if leaves[page_index].is_path()
                            && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        path_filter.push(ch);
                        path_selected = 0;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    };
    ambience.stop();
    drop(guard);
    outcome
}

fn draw(output: &mut TerminalOutput, lines: &[String]) {
    let _ = queue!(output, MoveTo(0, 0), Clear(ClearType::All));
    for (index, line) in lines.iter().enumerate() {
        let _ = queue!(
            output,
            MoveTo(0, index as u16),
            crossterm::style::Print(line)
        );
    }
    let _ = output.flush();
}

fn render(
    leaves: &[Leaf],
    page_index: usize,
    page_offset: usize,
    path_filter: &str,
    path_selected: usize,
    index_mode: Option<&IndexMode>,
    path_entries: &[PathEntry],
    music: &str,
    columns: u16,
    rows: u16,
) -> Screen {
    let width = columns.saturating_sub(2).max(20) as usize;
    let row_budget = rows.saturating_sub(1) as usize;
    let mut lines = vec![String::new()];
    let mut hits = Vec::new();
    let leaf = &leaves[page_index];
    let mut screen = Screen {
        lines: Vec::new(),
        hits: Vec::new(),
        page_content_offset: page_offset,
        visible_path_count: 0,
        index_result_count: 0,
    };

    if let Some(mode) = index_mode {
        let results = crate::model::search_indices(leaves, &mode.query);
        screen.index_result_count = results.len();
        let header = vec![
            "Search app names, commands, topics, and page text.".to_owned(),
            format!("search: {}_", mode.query),
            format!(
                "{} matches · Ctrl+F opens search · Esc returns",
                results.len()
            ),
        ];
        let header_panel = panel("Scroll app index", &header, width, 5, None);
        lines.extend(header_panel);
        lines.push(String::new());
        let footer_height = 4;
        let list_height = row_budget
            .saturating_sub(lines.len() + footer_height)
            .max(4);
        let list_slots = list_height.saturating_sub(2).max(1);
        let start = mode
            .selected
            .saturating_sub(list_slots / 2)
            .min(results.len().saturating_sub(list_slots));
        let end = (start + list_slots).min(results.len());
        let mut body = Vec::new();
        for (visible, result_index) in (start..end).enumerate() {
            let leaf_index = results[result_index];
            let item = &leaves[leaf_index];
            let selected = result_index == mode.selected;
            let label = match item {
                Leaf::App(page) => {
                    format!("{}  [{}]  {}", item.name(), page.domain, page.tagline)
                }
                Leaf::Path => "PATH  [System]  searchable executable launcher".to_owned(),
            };
            body.push(format!("{}{}", if selected { "▶ " } else { "  " }, label));
            let y = (lines.len() + 1 + visible + 1) as u16;
            hits.push(HitBox {
                x0: 2,
                x1: columns.saturating_sub(1),
                y,
                hit: Hit::Index(result_index),
            });
        }
        if results.is_empty() {
            body.push("  (no matches)".to_owned());
        }
        let panel_lines = panel("app index", &body, width, list_height, None);
        lines.extend(panel_lines);
        lines.push(String::new());
        lines.extend(panel(
            "quick jump",
            &["↑/↓ select · Enter opens · type to search · Esc closes".to_owned()],
            width,
            footer_height,
            None,
        ));
    } else if let Leaf::Path = leaf {
        let visible = filtered(path_entries, path_filter);
        screen.visible_path_count = visible.len();
        let header = vec![
            "System launcher — type to filter · Enter prints the name to your prompt.".to_owned(),
            format!("filter: {path_filter}_"),
            format!(
                "showing {}  ·  Shift-Tab returns to the book",
                visible.len()
            ),
        ];
        lines.extend(panel(
            &format!(
                "Scroll ✦ PATH ✦  ·  {}/{}  ·  Tab wraps to start",
                page_index + 1,
                leaves.len()
            ),
            &header,
            width,
            5,
            None,
        ));
        lines.push(String::new());
        let footer_height = 4;
        let list_height = row_budget
            .saturating_sub(lines.len() + footer_height)
            .max(4);
        let list_slots = list_height.saturating_sub(2).max(1);
        let selected = path_selected.min(visible.len().saturating_sub(1));
        let start = selected
            .saturating_sub(list_slots / 2)
            .min(visible.len().saturating_sub(list_slots));
        let end = (start + list_slots).min(visible.len());
        let mut body = Vec::new();
        for (visible_index, entry_index) in (start..end).enumerate() {
            let entry = visible[entry_index];
            let selected = entry_index == selected;
            let text = format!(
                "{}{:22} {}",
                if selected { "▶ " } else { "  " },
                entry.name,
                entry.directory
            );
            body.push(text);
            let y = (lines.len() + 1 + visible_index + 1) as u16;
            hits.push(HitBox {
                x0: 2,
                x1: columns.saturating_sub(1),
                y,
                hit: Hit::Path(entry_index),
            });
        }
        if visible.is_empty() {
            body.push("  (no matches — Backspace or Ctrl+U clears filter)".to_owned());
        }
        lines.extend(panel("path commands", &body, width, list_height, None));
        lines.push(String::new());
        lines.extend(panel(
            "runes",
            &[
                "Enter / double-click prints name · ↑/↓ select · type to filter · q quit"
                    .to_owned(),
            ],
            width,
            footer_height,
            None,
        ));
    } else if let Leaf::App(page) = leaf {
        let body_height = row_budget.saturating_sub(5).max(5);
        let footer_height = 4;
        let body_slot_count = body_height.saturating_sub(2).max(1);
        let body = page_content(page, width.saturating_sub(4).max(8));
        let content_offset = page_offset.min(body.len().saturating_sub(body_slot_count));
        screen.page_content_offset = content_offset;
        let body_lines = scroll_window(&body, content_offset, body_slot_count);
        let title = format!(
            "Scroll ✦ {} ✦  ·  {}/{}  ·  Tab next{}",
            page.name,
            page_index + 1,
            leaves.len(),
            match music {
                "playing" => " · ♪",
                "paused" => " · ♪∥",
                _ => "",
            }
        );
        lines.extend(panel(&title, &body_lines, width, body_height, None));
        lines.push(String::new());
        let runes = page.castable_runes();
        let labels: Vec<String> = if runes.is_empty() {
            vec!["(no runes)".to_owned()]
        } else {
            runes
                .iter()
                .enumerate()
                .map(|(index, rune)| format!("[{}:{}]", index + 1, rune.label))
                .collect()
        };
        let rune_row = labels.join("  ");
        let footer = vec![
            rune_row.clone(),
            "1–9 cast · Ctrl+F app index · m music · Tab page · g/G jump · j/k scroll · q quit"
                .to_owned(),
        ];
        let footer_start = lines.len();
        lines.extend(panel("runes", &footer, width, footer_height, None));
        let mut x = 3usize;
        for (index, label) in labels.iter().enumerate() {
            let visible = unicode_width::UnicodeWidthStr::width(label.as_str());
            let y = (footer_start + 2) as u16;
            hits.push(HitBox {
                x0: x as u16,
                x1: x.saturating_add(visible).min(width + 1) as u16,
                y,
                hit: Hit::Rune(index),
            });
            x = x.saturating_add(visible + 2);
        }
    }

    if lines.len() > rows as usize {
        lines.truncate(rows as usize);
    }
    screen.lines = lines;
    screen.hits = hits;
    screen
}

fn page_content(page: &Page, width: usize) -> Vec<String> {
    let mut content = Vec::new();
    content.push(String::new());
    content.extend(wrap_line(
        &format!("✦ {} ✦  {}", page.name, page.tagline),
        width,
    ));
    content.push(String::new());
    for line in &page.intro {
        content.extend(wrap_line(&format!("  {line}"), width));
    }
    content.push(String::new());
    content.extend(wrap_line(&format!("✦ How to use {} ✦", page.name), width));
    content.push(String::new());
    for line in &page.how {
        content.extend(wrap_line(&format!("  {line}"), width));
    }
    content.push(String::new());
    content.push("✦ CLI commands ✦".to_owned());
    content.push(String::new());
    for entry in &page.cli {
        content.extend(wrap_line(
            &format!("  {}  —  {}", entry.command, entry.description),
            width,
        ));
    }
    content.push(String::new());
    content
}

fn scroll_window(lines: &[String], offset: usize, slots: usize) -> Vec<String> {
    if lines.len() <= slots {
        return lines.to_vec();
    }
    let mut out = Vec::new();
    let mut start = offset.min(lines.len().saturating_sub(1));
    let has_above = start > 0;
    let mut room = slots.saturating_sub(usize::from(has_above));
    let mut has_below = start + room < lines.len();
    if has_below {
        room = room.saturating_sub(1);
    }
    room = room.max(1);
    start = start.min(lines.len().saturating_sub(room));
    if start > 0 {
        out.push(format!("… ↑ {start} above · j/k or wheel …"));
    }
    out.extend(lines.iter().skip(start).take(room).cloned());
    let end = (start + room).min(lines.len());
    has_below = end < lines.len();
    if has_below {
        out.push(format!("… ↓ {} more · j/k or wheel …", lines.len() - end));
    }
    out.truncate(slots);
    out
}

fn wrap_line(line: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    if UnicodeWidthStr::width(line) <= width {
        return vec![line.to_owned()];
    }
    let indent: String = line.chars().take_while(|ch| *ch == ' ').take(4).collect();
    let mut result = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;
    for word in line.split_whitespace() {
        let word_width = UnicodeWidthStr::width(word);
        let extra = if current.is_empty() { 0 } else { 1 };
        if !current.is_empty() && current_width + extra + word_width > width {
            result.push(current);
            current = indent.clone();
            current_width = UnicodeWidthStr::width(current.as_str());
        }
        if !current.is_empty() {
            current.push(' ');
            current_width += 1;
        }
        current.push_str(word);
        current_width += word_width;
    }
    if !current.is_empty() {
        result.push(current);
    }
    if result.is_empty() {
        result.push(String::new());
    }
    result
}

fn panel(
    title: &str,
    content: &[String],
    width: usize,
    height: usize,
    _selected: Option<usize>,
) -> Vec<String> {
    let width = width.max(20);
    let inner = width.saturating_sub(2);
    let title_slot = inner.saturating_sub(2);
    let title = fit(title, title_slot).trim_end().to_owned();
    let title_width = UnicodeWidthStr::width(title.as_str());
    let remaining = inner.saturating_sub(title_width + 3);
    let mut lines = vec![format!("┌─ {title} {}┐", "─".repeat(remaining))];
    let interior = height.saturating_sub(2);
    for index in 0..interior {
        let text = content.get(index).map(String::as_str).unwrap_or("");
        lines.push(format!("│ {} │", fit(text, inner.saturating_sub(2))));
    }
    lines.push(format!("└{}┘", "─".repeat(inner)));
    lines
}

fn fit(text: &str, width: usize) -> String {
    let mut result = String::new();
    let mut used = 0;
    for character in text.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if used + character_width > width {
            break;
        }
        result.push(character);
        used += character_width;
    }
    result.push_str(&" ".repeat(width.saturating_sub(used)));
    result
}

pub fn run_command(command_line: &str) -> i32 {
    let args = shell_words(command_line);
    let Some((command, rest)) = args.split_first() else {
        return 1;
    };
    match Command::new(command).args(rest).status() {
        Ok(status) => status.code().unwrap_or(128),
        Err(error) => {
            eprintln!("scroll: {error}");
            1
        }
    }
}

fn shell_words(input: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut started = false;
    for character in input.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            started = true;
        } else if character == '\\' && quote != Some('\'') {
            escaped = true;
            started = true;
        } else if let Some(active) = quote {
            if character == active {
                quote = None;
            } else {
                current.push(character);
            }
            started = true;
        } else if character == '\'' || character == '"' {
            quote = Some(character);
            started = true;
        } else if character.is_whitespace() {
            if started {
                words.push(std::mem::take(&mut current));
                started = false;
            }
        } else {
            current.push(character);
            started = true;
        }
    }
    if escaped {
        current.push('\\');
    }
    if started {
        words.push(current);
    }
    words
}

pub struct Ambience {
    socket: Option<std::path::PathBuf>,
    track: std::path::PathBuf,
    child: Option<std::process::Child>,
    enabled: bool,
}

impl Ambience {
    pub fn new() -> Self {
        let disabled = std::env::var("FAE_AMBIENCE")
            .unwrap_or_else(|_| "1".to_owned())
            .trim()
            .to_lowercase();
        let enabled = !matches!(disabled.as_str(), "0" | "false" | "no" | "off" | "quiet");
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        let track = home
            .as_ref()
            .map(|home| home.join("Music/faeos-chiptune/route_calm_town.wav"))
            .unwrap_or_default();
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .or_else(|| home.map(|home| home.join(".cache/faeos/runtime")));
        let socket = runtime.map(|dir| dir.join("fae-ambience-scroll.sock"));
        Self {
            socket,
            track,
            child: None,
            enabled,
        }
    }

    fn send(&self, command: serde_json::Value) -> Option<serde_json::Value> {
        use std::io::{Read, Write};
        use std::os::unix::net::UnixStream;
        let socket = self.socket.as_ref()?;
        let mut stream = UnixStream::connect(socket).ok()?;
        stream
            .set_read_timeout(Some(Duration::from_millis(250)))
            .ok()?;
        stream.write_all(format!("{command}\n").as_bytes()).ok()?;
        let mut bytes = [0; 4096];
        let size = stream.read(&mut bytes).ok()?;
        serde_json::from_slice(&bytes[..size]).ok()
    }

    fn request(&self, command: serde_json::Value) -> Option<serde_json::Value> {
        self.send(serde_json::json!({ "command": command }))
    }

    pub fn start(&mut self) {
        if !self.enabled || !self.track.is_file() || self.socket.is_none() {
            return;
        }
        if self.status() != "off" {
            let _ = self.request(serde_json::json!(["loadfile", self.track, "replace"]));
            let _ = self.request(serde_json::json!(["set_property", "volume", 38]));
            let _ = self.request(serde_json::json!(["set_property", "loop-file", "inf"]));
            let _ = self.request(serde_json::json!(["set_property", "pause", false]));
            return;
        }
        let Some(socket) = self.socket.as_ref() else {
            return;
        };
        if let Some(parent) = socket.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::remove_file(socket);
        let socket_argument = format!("--input-ipc-server={}", socket.display());
        let Ok(child) = Command::new("mpv")
            .args([
                "--no-video",
                "--audio-display=no",
                "--really-quiet",
                "--no-terminal",
                "--volume=38",
                socket_argument.as_str(),
                "--ao=pulse,pipewire,alsa",
                "--audio-stream-silence=yes",
                "--loop-file=inf",
            ])
            .arg(&self.track)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        else {
            return;
        };
        self.child = Some(child);
        for _ in 0..30 {
            if self
                .send(serde_json::json!({ "command": ["get_property", "pause"] }))
                .is_some()
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn status(&self) -> &'static str {
        let Some(response) = self.request(serde_json::json!(["get_property", "pause"])) else {
            return "off";
        };
        if response.get("error").and_then(|v| v.as_str()) != Some("success") {
            return "off";
        }
        if response
            .get("data")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            "paused"
        } else {
            "playing"
        }
    }

    pub fn toggle(&mut self) {
        if !self.enabled || !self.track.is_file() {
            return;
        }
        if self.status() == "off" {
            self.start();
        } else {
            let pause = self.status() != "paused";
            let _ = self.request(serde_json::json!(["set_property", "pause", pause]));
        }
    }

    pub fn stop(&mut self) {
        if self.socket.as_ref().is_some_and(|socket| socket.exists()) {
            let _ = self.request(serde_json::json!(["quit"]));
        }
        if let Some(mut child) = self.child.take() {
            let deadline = Instant::now() + Duration::from_millis(600);
            loop {
                match child.try_wait() {
                    Ok(Some(_)) | Err(_) => break,
                    Ok(None) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(25));
                    }
                    Ok(None) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                }
            }
            if let Some(socket) = &self.socket {
                let _ = std::fs::remove_file(socket);
            }
        }
    }
}

impl Default for Ambience {
    fn default() -> Self {
        Self::new()
    }
}

impl Leaf {
    fn is_path(&self) -> bool {
        matches!(self, Leaf::Path)
    }

    fn app_page(&self) -> Option<&Page> {
        match self {
            Leaf::App(page) => Some(page),
            Leaf::Path => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_argument_parser_preserves_quoted_rune_arguments() {
        assert_eq!(
            shell_words("kur \"hello, old one\""),
            vec!["kur".to_owned(), "hello, old one".to_owned()]
        );
        assert_eq!(
            shell_words("say 'two words' escaped\\ space"),
            vec![
                "say".to_owned(),
                "two words".to_owned(),
                "escaped space".to_owned()
            ]
        );
    }

    #[test]
    fn page_content_contains_curriculum_sections() {
        let data = crate::model::load_data();
        let page = data.pages.iter().find(|page| page.id == "summon").unwrap();
        let content = page_content(page, 40).join("\n");
        assert!(content.contains("CLI commands"));
        assert!(content.contains("PATH leaf"));
        assert!(content.contains("summon --refresh"));
    }
}

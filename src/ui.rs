use crate::model::{book_pages, egg_state, load_data, Leaf, Page};
use crate::path_index::{entries, filtered, PathEntry};
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers, MouseButton,
    MouseEventKind,
};
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PageTurnDirection {
    Next,
    Previous,
}

struct PageTransition {
    from: Vec<String>,
    started_at: Instant,
    direction: PageTurnDirection,
}

const PAGE_TRANSITION_DURATION: Duration = Duration::from_millis(240);
const SPLASH_DURATION: Duration = Duration::from_millis(2400);

#[derive(Clone, Copy)]
struct Theme {
    color: bool,
    unicode: bool,
}

impl Theme {
    fn from_env() -> Self {
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty())
            || std::env::var("TERM").is_ok_and(|term| term == "dumb");
        let unicode = unicode_enabled(std::env::var("SCROLL_UNICODE").ok().as_deref());
        Self {
            color: !no_color,
            unicode,
        }
    }
}

fn unicode_enabled(setting: Option<&str>) -> bool {
    !setting.is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "ascii" | "no" | "off"
        )
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ink {
    Body,
    Border,
    Accent,
    Secondary,
    Muted,
    Active,
}

#[derive(Debug, PartialEq, Eq)]
struct StyledSegment {
    text: String,
    ink: Ink,
}

fn ink_color(ink: Ink) -> Color {
    match ink {
        Ink::Body => Color::Rgb {
            r: 0xff,
            g: 0xeb,
            b: 0xf2,
        },
        Ink::Border => Color::Rgb {
            r: 0x9d,
            g: 0x5c,
            b: 0x75,
        },
        Ink::Accent => Color::Rgb {
            r: 0xe8,
            g: 0x79,
            b: 0xa0,
        },
        Ink::Secondary => Color::Rgb {
            r: 0x8e,
            g: 0xc4,
            b: 0xc8,
        },
        Ink::Muted => Color::Rgb {
            r: 0xc0,
            g: 0xc0,
            b: 0xc8,
        },
        Ink::Active => Color::Rgb {
            r: 0xff,
            g: 0xb0,
            b: 0x20,
        },
    }
}

fn styled_segments(line: &str) -> Vec<StyledSegment> {
    let chars: Vec<char> = line.chars().collect();
    let Some(last) = chars
        .iter()
        .rposition(|character| !character.is_whitespace())
    else {
        return Vec::new();
    };
    let leading = chars
        .iter()
        .take_while(|character| character.is_whitespace())
        .count();
    let content = line.trim_start();
    let top_border = content.starts_with("╭─ ") || content.starts_with("+- ");
    let bottom_border = content.starts_with("╰") || content.starts_with('+');
    let content_line = content.starts_with('│') || content.starts_with('|');
    let title_end = if top_border {
        chars
            .iter()
            .enumerate()
            .skip(leading + 3)
            .find(|(_, character)| matches!(character, '─' | '-'))
            .map(|(index, _)| index)
            .unwrap_or(last)
    } else {
        0
    };
    let mut segments: Vec<StyledSegment> = Vec::new();
    let title_word = ["Scroll", "lrcSlo", "rSclol", "olScrl", "cSroll", "Scorll"]
        .iter()
        .any(|word| line.contains(word));
    for (index, character) in chars.iter().copied().enumerate() {
        let ink = if top_border && index >= leading + 3 && index < title_end {
            Ink::Accent
        } else if top_border || bottom_border {
            Ink::Border
        } else if (content_line && (index == leading || index == last))
            || (index == leading && matches!(character, '╭' | '╮' | '╰' | '╯' | '+' | '│' | '|'))
        {
            Ink::Border
        } else if title_word {
            Ink::Accent
        } else if matches!(character, '✦' | '✧') {
            Ink::Accent
        } else if character == '▶' {
            Ink::Active
        } else if matches!(character, '·' | '…') {
            Ink::Secondary
        } else if line.contains("↑/↓ select") || line.contains("Enter prints") {
            Ink::Muted
        } else {
            Ink::Body
        };
        if let Some(segment) = segments.last_mut().filter(|segment| segment.ink == ink) {
            segment.text.push(character);
        } else {
            segments.push(StyledSegment {
                text: character.to_string(),
                ink,
            });
        }
    }
    segments
}

#[derive(Clone, Copy)]
struct SplashAnimation {
    started_at: Instant,
}

impl SplashAnimation {
    fn finished(self, now: Instant) -> bool {
        now.saturating_duration_since(self.started_at) >= SPLASH_DURATION
    }

    fn lines(self, now: Instant, columns: u16, rows: u16, theme: Theme) -> Vec<String> {
        const SCRAMBLES: [&str; 6] = ["lrcSlo", "rSclol", "olScrl", "cSroll", "Scorll", "Scroll"];
        const SIGILS: [&str; 6] = ["·", "✧", "✦", "✧", "✦", "✦"];
        const ASCII_SIGILS: [&str; 6] = [".", "*", "#", "*", "#", "#"];

        let elapsed = now.saturating_duration_since(self.started_at);
        let elapsed_ms = elapsed.as_millis() as usize;
        let frame = ((elapsed_ms / 90) % SCRAMBLES.len()).min(SCRAMBLES.len() - 1);
        let letters_settled = elapsed_ms.saturating_sub(450) / 150;
        let letters_settled = letters_settled.min("Scroll".len());
        let scrambled: Vec<char> = SCRAMBLES[frame].chars().collect();
        let final_title: Vec<char> = "Scroll".chars().collect();
        let title: String = final_title[..letters_settled]
            .iter()
            .chain(scrambled[letters_settled..].iter())
            .collect();
        let sigil_frame = ((elapsed_ms / 180).min(SIGILS.len() - 1)) as usize;
        let sigil = if theme.unicode {
            SIGILS[sigil_frame]
        } else {
            ASCII_SIGILS[sigil_frame]
        };
        let frame_progress = elapsed_ms.saturating_sub(900).min(700) as f32 / 700.0;
        let max_width = columns.saturating_sub(4).max(20).min(72) as usize;
        let start_width = 26.min(max_width);
        let frame_width = (start_width as f32
            + (max_width.saturating_sub(start_width) as f32 * frame_progress))
            .round()
            .max(20.0) as usize;
        let mut contents = vec![
            String::new(),
            format!("          {sigil}"),
            format!("        {title}"),
            String::new(),
            "     A living guide to Grove".to_owned(),
            "  Meet the house. Learn its tools.".to_owned(),
            String::new(),
            " Tab / Shift-Tab turn the lessons".to_owned(),
            "       Any key to begin".to_owned(),
        ];
        let frame_title = if theme.unicode {
            "✦ THE GROVE FIELD GUIDE ✦"
        } else {
            "* THE GROVE FIELD GUIDE *"
        };
        if elapsed_ms < 1450 {
            contents[4].clear();
            contents[5].clear();
            contents[7].clear();
            contents[8].clear();
        } else if elapsed_ms < 1900 {
            contents[7].clear();
            contents[8].clear();
        }
        let frame = panel(frame_title, &contents, frame_width, 11, theme.unicode);
        let x = (columns as usize).saturating_sub(frame_width) / 2;
        let y = rows.saturating_sub(frame.len() as u16) / 2;
        let mut lines = vec![String::new(); rows as usize];
        for (index, line) in frame.iter().enumerate() {
            let row = y as usize + index;
            if row < lines.len() {
                lines[row] = format!("{}{}", " ".repeat(x), line);
            }
        }
        lines
    }
}

impl PageTransition {
    fn progress(&self, now: Instant) -> f32 {
        (now.saturating_duration_since(self.started_at).as_secs_f32()
            / PAGE_TRANSITION_DURATION.as_secs_f32())
        .clamp(0.0, 1.0)
    }

    fn finished(&self, now: Instant) -> bool {
        self.progress(now) >= 1.0
    }

    fn eased_shift(&self, now: Instant, rows: usize) -> usize {
        let progress = self.progress(now);
        let eased = 1.0 - (1.0 - progress).powi(3);
        (eased * rows as f32).round() as usize
    }

    fn compose(&self, incoming: &[String], now: Instant, rows: usize, width: usize) -> Vec<String> {
        let shift = self.eased_shift(now, rows).min(rows);
        let incoming_start = rows.saturating_sub(shift);
        (0..rows)
            .map(|row| {
                let source_row = match self.direction {
                    PageTurnDirection::Next if row < incoming_start => self.from.get(row + shift),
                    PageTurnDirection::Next => incoming.get(row - incoming_start),
                    PageTurnDirection::Previous if row < shift => {
                        incoming.get(row + incoming_start)
                    }
                    PageTurnDirection::Previous => self.from.get(row - shift),
                };
                source_row.map(|line| fit(line, width)).unwrap_or_default()
            })
            .collect()
    }
}

fn reduced_motion_requested(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

struct Screen {
    lines: Vec<String>,
    hits: Vec<HitBox>,
    page_content_offset: usize,
    page_content_size: usize,
    path_selection_size: usize,
    index_selection_size: usize,
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
    let mut index_mode: Option<IndexMode> = None;
    let mut last_path_click: Option<(usize, Instant)> = None;
    let motion_enabled =
        !reduced_motion_requested(std::env::var("SCROLL_REDUCED_MOTION").ok().as_deref());
    let mut splash = motion_enabled.then(|| SplashAnimation {
        started_at: Instant::now(),
    });
    let mut page_transition: Option<PageTransition> = None;
    let theme = Theme::from_env();
    if start_path {
        page_index = book_pages(&data, egg_state()).len().saturating_sub(1);
    }
    ambience.start();

    let outcome = 'ui: loop {
        let state = egg_state();
        let leaves = book_pages(&data, state);
        page_index = page_index.min(leaves.len().saturating_sub(1));
        let now = Instant::now();
        if splash.is_some_and(|animation| animation.finished(now)) {
            splash = None;
        }
        if page_transition
            .as_ref()
            .is_some_and(|animation| animation.finished(now))
        {
            page_transition = None;
        }
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
            &theme,
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
        let visible_lines = page_transition
            .as_ref()
            .map(|animation| {
                animation.compose(
                    &screen.lines,
                    now,
                    rows as usize,
                    columns.saturating_sub(2).max(20) as usize,
                )
            })
            .unwrap_or_else(|| screen.lines.clone());
        if let Some(animation) = splash {
            draw(
                &mut guard.output,
                &animation.lines(now, columns, rows, theme),
                theme,
            );
        } else {
            draw(&mut guard.output, &visible_lines, theme);
        }

        let animating = splash.is_some() || page_transition.is_some();
        let poll_interval = if animating {
            Duration::from_millis(16)
        } else {
            Duration::from_millis(150)
        };
        if !event::poll(poll_interval).unwrap_or(false) {
            continue;
        }
        let Ok(event) = event::read() else {
            continue;
        };
        match event {
            Event::Mouse(mouse) => {
                if splash.take().is_some() {
                    page_transition = None;
                    continue;
                }
                if page_transition.is_some() {
                    page_transition = None;
                    if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                        continue;
                    }
                }
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
                if splash.take().is_some() {
                    page_transition = None;
                    if key.code == KeyCode::Char('q')
                        || key.code == KeyCode::Char('Q')
                        || (key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL))
                    {
                        break 'ui UiResult::Quit;
                    }
                    continue;
                }
                if !crate::grove_keys::is_cycle_next(key.code, key.modifiers)
                    && !crate::grove_keys::is_cycle_previous(key.code, key.modifiers)
                {
                    page_transition = None;
                }
                if let Some(mode) = index_mode.as_mut() {
                    if let Some(selected) =
                        navigation_target(mode.selected, key.code, screen.index_selection_size)
                    {
                        mode.selected = selected;
                        continue;
                    }
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
                if crate::grove_keys::is_cycle_next(key.code, key.modifiers) {
                    if motion_enabled {
                        page_transition = Some(PageTransition {
                            from: visible_lines.clone(),
                            started_at: Instant::now(),
                            direction: PageTurnDirection::Next,
                        });
                    } else {
                        page_transition = None;
                    }
                    page_index = (page_index + 1) % leaves.len();
                    page_offset = 0;
                    path_filter.clear();
                    path_selected = 0;
                    continue;
                }
                if crate::grove_keys::is_cycle_previous(key.code, key.modifiers) {
                    if motion_enabled {
                        page_transition = Some(PageTransition {
                            from: visible_lines.clone(),
                            started_at: Instant::now(),
                            direction: PageTurnDirection::Previous,
                        });
                    } else {
                        page_transition = None;
                    }
                    page_index = page_index.checked_sub(1).unwrap_or(leaves.len() - 1);
                    page_offset = 0;
                    path_filter.clear();
                    path_selected = 0;
                    continue;
                }
                if leaves[page_index].is_path() {
                    if let Some(selected) =
                        navigation_target(path_selected, key.code, screen.path_selection_size)
                    {
                        path_selected = selected;
                        continue;
                    }
                } else if let Some(offset) =
                    navigation_target(page_offset, key.code, screen.page_content_size)
                {
                    page_offset = offset;
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') => break 'ui UiResult::Quit,
                    KeyCode::Char('m') if !leaves[page_index].is_path() => ambience.toggle(),
                    KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        ambience.toggle()
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

fn navigation_target(current: usize, code: KeyCode, page_size: usize) -> Option<usize> {
    match code {
        KeyCode::Up => Some(current.saturating_sub(1)),
        KeyCode::Down => Some(current.saturating_add(1)),
        KeyCode::PageUp => Some(current.saturating_sub(page_size.max(1))),
        KeyCode::PageDown => Some(current.saturating_add(page_size.max(1))),
        _ => None,
    }
}

fn draw(output: &mut TerminalOutput, lines: &[String], theme: Theme) {
    let _ = queue!(output, MoveTo(0, 0), Clear(ClearType::All));
    for (index, line) in lines.iter().enumerate() {
        let _ = queue!(output, MoveTo(0, index as u16));
        if theme.color {
            for segment in styled_segments(line) {
                let _ = queue!(
                    output,
                    SetForegroundColor(ink_color(segment.ink)),
                    Print(segment.text)
                );
            }
            let _ = queue!(output, ResetColor);
        } else {
            let _ = queue!(output, Print(line));
        }
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
    theme: &Theme,
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
        page_content_size: 1,
        path_selection_size: 1,
        index_selection_size: 1,
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
        let header_panel = panel("Scroll app index", &header, width, 5, theme.unicode);
        lines.extend(header_panel);
        lines.push(String::new());
        let footer_height = 4;
        let list_height = row_budget
            .saturating_sub(lines.len() + footer_height)
            .max(4);
        let list_slots = list_height.saturating_sub(2).max(1);
        screen.index_selection_size = list_slots;
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
        let panel_lines = panel("app index", &body, width, list_height, theme.unicode);
        lines.extend(panel_lines);
        lines.push(String::new());
        lines.extend(panel(
            "quick jump",
            &["↑/↓ select · PgUp/PgDn move · Enter opens · Esc closes".to_owned()],
            width,
            footer_height,
            theme.unicode,
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
            theme.unicode,
        ));
        lines.push(String::new());
        let footer_height = 4;
        let list_height = row_budget
            .saturating_sub(lines.len() + footer_height)
            .max(4);
        let list_slots = list_height.saturating_sub(2).max(1);
        screen.path_selection_size = list_slots;
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
        lines.extend(panel(
            "path commands",
            &body,
            width,
            list_height,
            theme.unicode,
        ));
        lines.push(String::new());
        lines.extend(panel(
            "runes",
            &["Enter prints · ↑/↓ select · PgUp/PgDn move · type filters · q quit".to_owned()],
            width,
            footer_height,
            theme.unicode,
        ));
    } else if let Leaf::App(page) = leaf {
        let body_height = row_budget.saturating_sub(5).max(5);
        let footer_height = 4;
        let body_slot_count = body_height.saturating_sub(2).max(1);
        screen.page_content_size = body_slot_count;
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
        lines.extend(panel(
            &title,
            &body_lines,
            width,
            body_height,
            theme.unicode,
        ));
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
            "1–9 cast · ↑/↓ scroll · PgUp/PgDn page · Ctrl+F search · Tab page · q quit".to_owned(),
        ];
        let footer_start = lines.len();
        lines.extend(panel("runes", &footer, width, footer_height, theme.unicode));
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
        out.push(format!("… ↑ {start} above · ↑/↓ or wheel …"));
    }
    out.extend(lines.iter().skip(start).take(room).cloned());
    let end = (start + room).min(lines.len());
    has_below = end < lines.len();
    if has_below {
        out.push(format!("… ↓ {} more · ↑/↓ or wheel …", lines.len() - end));
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
    unicode: bool,
) -> Vec<String> {
    let width = width.max(20);
    let inner = width.saturating_sub(2);
    let title_slot = inner.saturating_sub(2);
    let title = fit(title, title_slot).trim_end().to_owned();
    let title_width = UnicodeWidthStr::width(title.as_str());
    let remaining = inner.saturating_sub(title_width + 3);
    let (top_left, top_right, bottom_left, bottom_right, horizontal, vertical) = if unicode {
        ('╭', '╮', '╰', '╯', '─', '│')
    } else {
        ('+', '+', '+', '+', '-', '|')
    };
    let mut lines = vec![format!(
        "{top_left}{horizontal} {title} {}{top_right}",
        horizontal.to_string().repeat(remaining)
    )];
    let interior = height.saturating_sub(2);
    for index in 0..interior {
        let text = content.get(index).map(String::as_str).unwrap_or("");
        lines.push(format!(
            "{vertical} {} {vertical}",
            fit(text, inner.saturating_sub(2))
        ));
    }
    lines.push(format!(
        "{bottom_left}{}{bottom_right}",
        horizontal.to_string().repeat(inner)
    ));
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

    fn trim_lines(lines: Vec<String>) -> Vec<String> {
        lines
            .into_iter()
            .map(|line| line.trim_end().to_owned())
            .collect()
    }

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

    #[test]
    fn splash_storyboard_scrambles_resolves_and_finishes() {
        let started_at = Instant::now();
        let splash = SplashAnimation { started_at };
        let theme = Theme {
            color: true,
            unicode: true,
        };
        let opening = splash.lines(started_at, 100, 30, theme).join("\n");
        let middle = splash
            .lines(started_at + Duration::from_millis(1200), 100, 30, theme)
            .join("\n");
        let settled = splash
            .lines(started_at + SPLASH_DURATION, 100, 30, theme)
            .join("\n");

        assert!(opening.contains("Scroll") || opening.contains("lrcSlo"));
        assert!(middle.contains("Scroll"));
        assert!(settled.contains("A living guide to Grove"));
        assert!(settled.contains("Tab / Shift-Tab turn the lessons"));
        assert!(settled.contains("Any key to begin"));
        assert!(!splash.finished(started_at + SPLASH_DURATION - Duration::from_millis(1)));
        assert!(splash.finished(started_at + SPLASH_DURATION));
    }

    #[test]
    fn splash_has_unicode_and_ascii_sigils_and_adapts_to_terminal_size() {
        let now = Instant::now();
        let splash = SplashAnimation { started_at: now };
        let unicode = splash.lines(
            now + Duration::from_millis(800),
            80,
            24,
            Theme {
                color: true,
                unicode: true,
            },
        );
        let ascii = splash.lines(
            now + Duration::from_millis(800),
            60,
            18,
            Theme {
                color: false,
                unicode: false,
            },
        );
        assert!(unicode.iter().any(|line| line.contains('✦')));
        assert!(ascii.iter().any(|line| line.contains('#')));
        assert_eq!(ascii.len(), 18);
        assert!(ascii.iter().all(|line| line.chars().count() <= 60));
    }

    #[test]
    fn vertical_transition_pushes_in_opposite_directions_and_completes() {
        let started_at = Instant::now();
        let from: Vec<String> = (0..10).map(|row| format!("old-{row}")).collect();
        let to: Vec<String> = (0..10).map(|row| format!("new-{row}")).collect();
        let next = PageTransition {
            from: from.clone(),
            started_at,
            direction: PageTurnDirection::Next,
        };
        let previous = PageTransition {
            from: from.clone(),
            started_at,
            direction: PageTurnDirection::Previous,
        };
        let halfway = started_at + Duration::from_millis(120);
        let next_frame = next.compose(&to, halfway, 10, 10);
        let previous_frame = previous.compose(&to, halfway, 10, 10);

        assert_eq!(trim_lines(next.compose(&to, started_at, 10, 10)), from);
        assert_eq!(
            trim_lines(next.compose(&to, started_at + PAGE_TRANSITION_DURATION, 10, 10)),
            to
        );
        assert!(next_frame[0].starts_with("old-"));
        assert!(next_frame[9].starts_with("new-"));
        assert!(previous_frame[0].starts_with("new-"));
        assert!(previous_frame[9].starts_with("old-"));
        assert!(next.finished(started_at + PAGE_TRANSITION_DURATION));
    }

    #[test]
    fn rapid_page_cycle_retargets_without_a_visible_jump() {
        let started_at = Instant::now();
        let original: Vec<String> = (0..10).map(|row| format!("page-a-{row}")).collect();
        let middle: Vec<String> = (0..10).map(|row| format!("page-b-{row}")).collect();
        let destination: Vec<String> = (0..10).map(|row| format!("page-c-{row}")).collect();
        let first = PageTransition {
            from: original,
            started_at,
            direction: PageTurnDirection::Next,
        };
        let retarget_at = started_at + Duration::from_millis(70);
        let visible = first.compose(&middle, retarget_at, 10, 10);
        let second = PageTransition {
            from: visible.clone(),
            started_at: retarget_at,
            direction: PageTurnDirection::Next,
        };
        assert_eq!(second.compose(&destination, retarget_at, 10, 10), visible);
        assert_eq!(
            trim_lines(second.compose(
                &destination,
                retarget_at + PAGE_TRANSITION_DURATION,
                10,
                10
            )),
            destination
        );
    }

    #[test]
    fn reduced_motion_setting_accepts_common_true_values() {
        for value in ["1", "true", "YES", " on "] {
            assert!(reduced_motion_requested(Some(value)));
        }
        for value in [None, Some("0"), Some("false"), Some("no")] {
            assert!(!reduced_motion_requested(value));
        }
    }

    #[test]
    fn panels_use_rounded_unicode_or_ascii_corners() {
        let rounded = panel("lesson", &["hello".to_owned()], 24, 3, true);
        assert!(rounded[0].starts_with("╭─ lesson"));
        assert!(rounded[1].starts_with("│ "));
        assert!(rounded[2].starts_with('╰'));

        let ascii = panel("lesson", &["hello".to_owned()], 24, 3, false);
        assert!(ascii[0].starts_with("+- lesson"));
        assert!(ascii[1].starts_with("| "));
        assert!(ascii[2].starts_with('+'));
    }

    #[test]
    fn unicode_panels_are_default_and_ascii_can_be_requested() {
        assert!(unicode_enabled(None));
        assert!(unicode_enabled(Some("1")));
        assert!(!unicode_enabled(Some("ascii")));
        assert!(!unicode_enabled(Some("0")));
    }

    #[test]
    fn semantic_styling_keeps_text_separate_from_terminal_colors() {
        let segments = styled_segments("╭─ Scroll ✦ lesson ─╮");
        assert!(segments.iter().any(|segment| segment.ink == Ink::Border));
        assert!(segments.iter().any(|segment| segment.ink == Ink::Accent));
        assert!(segments
            .iter()
            .all(|segment| !segment.text.contains("\u{1b}")));

        let selected = styled_segments("│ ▶ chosen app │");
        assert!(selected.iter().any(|segment| segment.ink == Ink::Active));
    }

    #[test]
    fn conventional_navigation_moves_by_line_or_visible_page_size() {
        assert_eq!(navigation_target(4, KeyCode::Up, 8), Some(3));
        assert_eq!(navigation_target(4, KeyCode::Down, 8), Some(5));
        assert_eq!(navigation_target(12, KeyCode::PageUp, 5), Some(7));
        assert_eq!(navigation_target(12, KeyCode::PageDown, 5), Some(17));
        assert_eq!(navigation_target(0, KeyCode::PageUp, 0), Some(0));
        assert_eq!(navigation_target(4, KeyCode::Char('j'), 8), None);
        assert_eq!(navigation_target(4, KeyCode::Char('g'), 8), None);
    }

    #[test]
    fn path_and_search_lists_expose_their_visible_page_size() {
        let data = crate::model::load_data();
        let leaves = crate::model::book_pages(&data, crate::model::egg_state());
        let path_entries: Vec<_> = (0..40)
            .map(|index| PathEntry {
                name: format!("command-{index}"),
                directory: "/bin".to_owned(),
            })
            .collect();
        let theme = Theme {
            color: false,
            unicode: true,
        };
        let path_screen = render(
            &leaves,
            leaves.len() - 1,
            0,
            "",
            0,
            None,
            &path_entries,
            "",
            &theme,
            100,
            30,
        );
        assert!(path_screen.path_selection_size > 1);
        assert!(
            navigation_target(0, KeyCode::PageDown, path_screen.path_selection_size).unwrap() > 1
        );

        let index_mode = IndexMode {
            query: String::new(),
            selected: 0,
        };
        let index_screen = render(
            &leaves,
            0,
            0,
            "",
            0,
            Some(&index_mode),
            &path_entries,
            "",
            &theme,
            100,
            30,
        );
        assert!(index_screen.index_result_count > 0);
        assert!(index_screen.index_selection_size > 1);
        assert!(
            navigation_target(0, KeyCode::PageDown, index_screen.index_selection_size).unwrap() > 1
        );
    }
}

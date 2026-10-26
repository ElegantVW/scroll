mod model;
mod path_index;
mod ui;

use std::process::ExitCode;

const HELP: &str = "\
scroll — faeOS living book and PATH leaf

Usage:
  scroll-rs              open the book on page one
  scroll-rs --path       open the PATH leaf
  scroll-rs list         print every page as text
  scroll-rs --refresh    rescan the PATH cache
  scroll-rs -x <query>   run the first matching PATH command

Keys:
  Tab / Shift-Tab        next / previous page
  1–9                    cast footer runes
  j/k or wheel           scroll / select
  g/G then digit         jump to pages 11–20 / 21–30
  Ctrl-F                 search and quick-jump to an app
  m / Ctrl-P              toggle Scroll ambience
  q                      close the book
";

fn main() -> ExitCode {
    let invoked_as = std::env::args_os()
        .next()
        .and_then(|arg| {
            std::path::PathBuf::from(arg)
                .file_name()
                .map(|name| name.to_owned())
        })
        .unwrap_or_default();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let summon_mode = std::env::var("FAE_SCROLL_TAB")
        .unwrap_or_default()
        .to_lowercase();
    let mut start_path =
        invoked_as == "summon" || matches!(summon_mode.as_str(), "path" | "summon" | "1");

    if args
        .first()
        .is_some_and(|arg| arg == "-h" || arg == "--help")
    {
        print!("{HELP}");
        return ExitCode::SUCCESS;
    }
    if args.first().is_some_and(|arg| arg == "--refresh") {
        let found = path_index::scan_path();
        let count = found.len();
        return match path_index::write_cache(&path_index::cache_path(), &found) {
            Ok(()) => {
                eprintln!("scroll: PATH cache refreshed ({count} commands)");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("scroll: could not refresh PATH cache: {error}");
                ExitCode::FAILURE
            }
        };
    }
    if args
        .first()
        .is_some_and(|arg| matches!(arg.as_str(), "-l" | "--list" | "list"))
    {
        if start_path || args.iter().any(|arg| arg == "--path") {
            for entry in path_index::entries() {
                println!("{}", entry.name);
            }
        } else {
            ui::print_list();
        }
        return ExitCode::SUCCESS;
    }

    let mut run_now = false;
    let mut remaining = Vec::new();
    for arg in args {
        match arg.as_str() {
            "-x" | "--exec" => run_now = true,
            "--path" | "-p" | "path" => start_path = true,
            _ => remaining.push(arg),
        }
    }
    if run_now && !remaining.is_empty() {
        let query = &remaining[0];
        let entries = path_index::entries();
        let Some(hit) = path_index::best_match(&entries, query) else {
            eprintln!("summon: no match for {query:?}");
            return ExitCode::from(1);
        };
        let mut command = hit.name.clone();
        for arg in remaining.iter().skip(1) {
            command.push(' ');
            command.push_str(arg);
        }
        return ExitCode::from(ui::run_command(&command).clamp(0, 255) as u8);
    }

    if !ui::is_terminal() {
        if start_path {
            eprintln!("summon: an interactive terminal is required");
            return ExitCode::FAILURE;
        }
        ui::print_list();
        return ExitCode::SUCCESS;
    }

    match ui::run(start_path, String::new(), ui::Ambience::new()) {
        ui::UiResult::Quit => ExitCode::from(1),
        ui::UiResult::Fallback => ExitCode::SUCCESS,
        ui::UiResult::PrintName(name) => {
            println!("{name}");
            ExitCode::SUCCESS
        }
        ui::UiResult::RunCommand(command) => {
            ExitCode::from(ui::run_command(&command).clamp(0, 255) as u8)
        }
    }
}

mod config;
mod diagram;
mod export;
mod image;
mod json;
mod markdown;
#[cfg(feature = "pos")]
mod pos;
mod style;
mod theme;
mod viewer;

use std::io::{self, IsTerminal, Read};
use std::{fs, process};

use clap::Parser;

#[derive(Parser)]
#[command(
    name = "mdterm",
    version,
    about = "Terminal Markdown viewer with style"
)]
struct Cli {
    /// Markdown file(s) to view
    files: Vec<String>,

    /// Theme: dark or light
    #[arg(long, short = 'T')]
    theme: Option<String>,

    /// Display width override (0 = auto)
    #[arg(long, short = 'w', default_value = "0")]
    width: usize,

    /// Slide mode (horizontal rules become slide separators)
    #[arg(long, short = 's')]
    slides: bool,

    /// Deprecated: file watching is now always active
    #[arg(long, short = 'f', hide = true)]
    follow: bool,

    /// Show line numbers in code blocks
    #[arg(long, short = 'l')]
    line_numbers: bool,

    /// Export format instead of interactive view (html)
    #[arg(long)]
    export: Option<String>,

    /// Disable colors
    #[arg(long)]
    no_color: bool,

    /// Part-of-speech highlighting (requires `pos` feature)
    #[arg(long, num_args = 0..=1, value_name = "CATEGORIES")]
    pos: Option<String>,
}

mod pos_cli {
    /// Parsed `--pos` value: `None` (flag absent), `All` (`--pos` / `--pos all`),
    /// or `Some(names)` for an explicit list.
    #[derive(Debug)]
    pub enum PosArg {
        All,
        Some(Vec<String>),
    }

    impl PosArg {
        pub fn parse(raw: Option<&str>) -> Result<Self, String> {
            match raw {
                None => Ok(Self::All),
                Some(v) => {
                    let v = v.trim();
                    if v.eq_ignore_ascii_case("all") || v.is_empty() {
                        Ok(Self::All)
                    } else {
                        let names: Vec<String> = v
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        if names.is_empty() {
                            Ok(Self::All)
                        } else {
                            Ok(Self::Some(names))
                        }
                    }
                }
            }
        }
    }

    #[allow(dead_code)]
    pub const VALID_CATEGORIES: [&str; 9] = [
        "noun",
        "verb",
        "adjective",
        "adverb",
        "preposition",
        "conjunction",
        "determiner",
        "pronoun",
        "value",
    ];

    #[allow(dead_code)]
    pub const INSTALL_HINT: &str = "POS highlighting requires: cargo install mdterm --features pos";
}

fn main() {
    let cli = Cli::parse();
    let config = config::Config::load();

    // Determine theme
    let theme_name = cli.theme.as_deref().unwrap_or(&config.theme);
    let initial_theme = match theme_name {
        "light" => theme::Theme::light(),
        _ => theme::Theme::dark(),
    };

    let line_numbers = cli.line_numbers || config.line_numbers;
    let width = if cli.width > 0 {
        cli.width
    } else if config.width > 0 {
        config.width
    } else {
        0
    };

    // Resolve --pos: parse the CLI value (if any).
    let pos_arg = match &cli.pos {
        None => None,
        Some(v) => pos_cli::PosArg::parse(if v.is_empty() { None } else { Some(v.as_str()) })
            .map(Some)
            .unwrap_or_else(|e| {
                eprintln!("{e}");
                process::exit(2);
            }),
    };

    #[cfg(not(feature = "pos"))]
    if pos_arg.is_some() {
        eprintln!("{}", pos_cli::INSTALL_HINT);
        process::exit(0);
    }

    // Resolve enabled + raw categories from CLI (overrides config).
    let (pos_enabled, pos_categories): (bool, Vec<String>) = match pos_arg {
        Some(pos_cli::PosArg::All) => (true, Vec::new()),
        Some(pos_cli::PosArg::Some(names)) => (true, names),
        None => (
            config.pos.enabled,
            config.pos.categories.clone().unwrap_or_default(),
        ),
    };

    #[cfg(feature = "pos")]
    let pos_set = if pos_categories.is_empty() {
        pos::PosCategorySet::all()
    } else {
        match pos::PosCategorySet::from_names(&pos_categories) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{e}");
                process::exit(2);
            }
        }
    };

    // Read content: stdin or file(s)
    let (content, filename) = if cli.files.is_empty() {
        if io::stdin().is_terminal() {
            eprintln!("Usage: mdterm [OPTIONS] <FILE>...");
            eprintln!("       command | mdterm");
            eprintln!();
            eprintln!("Try 'mdterm --help' for more information.");
            process::exit(1);
        }
        const MAX_STDIN_BYTES: u64 = 100 * 1024 * 1024; // 100 MB
        let mut buf = String::new();
        let n = io::stdin()
            .take(MAX_STDIN_BYTES + 1)
            .read_to_string(&mut buf)
            .unwrap_or_else(|e| {
                eprintln!("Error reading stdin: {}", e);
                process::exit(1);
            });
        if n as u64 > MAX_STDIN_BYTES {
            eprintln!("Error: stdin input exceeds 100 MB limit");
            process::exit(1);
        }
        (buf, "<stdin>".to_string())
    } else {
        let path = &cli.files[0];
        let c = fs::read_to_string(path).unwrap_or_else(|e| {
            eprintln!("Error reading '{}': {}", path, e);
            process::exit(1);
        });
        (c, path.clone())
    };

    let is_json = filename.ends_with(".json");

    // Export mode
    if let Some(ref fmt) = cli.export {
        match fmt.as_str() {
            "html" => {
                let w = if width > 0 { width } else { 80 };
                export::to_html(&content, w, &initial_theme, &filename);
            }
            _ => {
                eprintln!("Unknown export format '{}'. Supported: html", fmt);
                process::exit(1);
            }
        }
        return;
    }

    // Interactive or piped
    if io::stdout().is_terminal() && !cli.no_color {
        let opts = viewer::ViewerOptions {
            files: cli.files,
            initial_content: content,
            filename,
            theme: initial_theme,
            slide_mode: cli.slides,
            line_numbers,
            width_override: if width > 0 { Some(width) } else { None },
            pos_enabled,
            pos_categories,
        };
        if let Err(e) = viewer::run(opts) {
            eprintln!("Viewer error: {}", e);
            process::exit(1);
        }
    } else {
        let w = if width > 0 {
            width
        } else {
            crossterm::terminal::size()
                .map(|(c, _)| c as usize)
                .unwrap_or(80)
        };
        let (mut lines, doc_info) = if is_json {
            match json::render(&content, w, &initial_theme) {
                Ok(result) => result,
                Err(e) => {
                    eprintln!("JSON parse error: {}", e);
                    process::exit(1);
                }
            }
        } else {
            markdown::render(&content, w, &initial_theme, line_numbers)
        };

        #[cfg(feature = "pos")]
        {
            if pos_enabled && !is_json {
                match pos::PosTagger::load() {
                    Ok(tagger) => pos::apply(
                        &mut lines,
                        &initial_theme,
                        &tagger,
                        pos_set,
                        doc_info.frontmatter_lines,
                    ),
                    Err(e) => eprintln!("POS disabled: {e}"),
                }
            }
        }
        #[cfg(not(feature = "pos"))]
        {
            let _ = &mut lines;
            let _ = &doc_info;
        }
        let wrapped = style::wrap_lines(&lines, w);
        if cli.no_color {
            viewer::print_lines_plain(&wrapped);
        } else {
            viewer::print_lines(&wrapped);
        }
    }
}

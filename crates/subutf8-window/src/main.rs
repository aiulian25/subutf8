//! The desktop window for SubUTF8. `subutf8` starts it with the interface's address in
//! `SUBUTF8_WINDOW_URL` and keeps its standard input open: a `raise` line brings the window to
//! the front, and the end of input means `subutf8` has stopped, so the window closes too.
//! Closing the window exits with success, which tells `subutf8` to stop.
//!
//! The page asks for the system's own file picker by posting `files`, `folders` or
//! `output-folder`; the chosen paths go back to the page's `subutf8Picked` function. Files
//! dropped onto the window go there too, as `dropped`, because WebKitGTK does not tell the page
//! where dropped files are.

use std::env;
use std::io::{self, BufRead};
use std::path::PathBuf;
use std::process::ExitCode;
use std::thread;

use gtk::prelude::{FileChooserExt, NativeDialogExt};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::unix::WindowExtUnix;
use tao::window::{Window, WindowBuilder};
use wry::{DragDropEvent, NewWindowResponse, WebViewBuilder, WebViewBuilderExtUnix};

/// Matches `subutf8.desktop`, so desktops show the app's own name and icon for this window.
const PROGRAM_NAME: &str = "subutf8";
const WINDOW_TITLE: &str = "SubUTF8";
const URL_VARIABLE: &str = "SUBUTF8_WINDOW_URL";
const RAISE_COMMAND: &str = "raise";
const SCHEME_SEPARATOR: &str = "://";
const PATH_SEPARATOR: char = '/';
const WINDOW_WIDTH: f64 = 1180.0;
const WINDOW_HEIGHT: f64 = 760.0;
const MINIMUM_WIDTH: f64 = 720.0;
const MINIMUM_HEIGHT: f64 = 480.0;
const MISSING_URL_MESSAGE: &str = "subutf8-window is started by subutf8; run subutf8 instead.";
const START_FAILED_MESSAGE: &str = "The SubUTF8 window could not open:";
const NO_CONTENT_AREA_MESSAGE: &str = "the window has no content area";

/// `ui/constants.js` sends and receives the same names.
const PICK_FILES: &str = "files";
const PICK_FOLDERS: &str = "folders";
const PICK_OUTPUT_FOLDER: &str = "output-folder";
const DROPPED: &str = "dropped";
const PICKED_CALLBACK: &str = "subutf8Picked";
const FILES_TITLE: &str = "Add subtitle files";
const FOLDERS_TITLE: &str = "Add folders";
const OUTPUT_FOLDER_TITLE: &str = "Choose the output folder";
const ADD_LABEL: &str = "_Add";
const SELECT_LABEL: &str = "_Select";
const CANCEL_LABEL: &str = "_Cancel";
const SUBTITLES_FILTER_NAME: &str = "Subtitles (.srt)";
const SUBTITLES_PATTERN: &str = "*.[sS][rR][tT]";

#[derive(Debug, Clone, Copy)]
enum Pick {
    Files,
    Folders,
    OutputFolder,
}

impl Pick {
    fn from_message(message: &str) -> Option<Self> {
        match message {
            PICK_FILES => Some(Self::Files),
            PICK_FOLDERS => Some(Self::Folders),
            PICK_OUTPUT_FOLDER => Some(Self::OutputFolder),
            _ => None,
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::Files => PICK_FILES,
            Self::Folders => PICK_FOLDERS,
            Self::OutputFolder => PICK_OUTPUT_FOLDER,
        }
    }
}

#[derive(Debug, Clone)]
enum Command {
    Raise,
    Close,
    Pick(Pick),
    Dropped(Vec<PathBuf>),
}

fn main() -> ExitCode {
    let Ok(url) = env::var(URL_VARIABLE) else {
        eprintln!("{MISSING_URL_MESSAGE}");
        return ExitCode::FAILURE;
    };
    gtk::glib::set_prgname(Some(PROGRAM_NAME));
    gtk::glib::set_application_name(WINDOW_TITLE);
    if let Err(problem) = open(&url) {
        eprintln!("{START_FAILED_MESSAGE} {problem}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// Returns only when the window cannot open; otherwise the event loop ends the process.
fn open(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoopBuilder::<Command>::with_user_event().build();
    gtk::Window::set_default_icon_name(PROGRAM_NAME);
    let window = WindowBuilder::new()
        .with_title(WINDOW_TITLE)
        .with_inner_size(LogicalSize::new(WINDOW_WIDTH, WINDOW_HEIGHT))
        .with_min_inner_size(LogicalSize::new(MINIMUM_WIDTH, MINIMUM_HEIGHT))
        .build(&event_loop)?;
    let container = window.default_vbox().ok_or(NO_CONTENT_AREA_MESSAGE)?;
    // The page may only ever show the app's own address; anything else is refused.
    let origin = origin_of(url).to_owned();
    let picker = event_loop.create_proxy();
    let dropper = event_loop.create_proxy();
    let webview = WebViewBuilder::new()
        .with_url(url)
        .with_incognito(true)
        .with_navigation_handler(move |target| target.starts_with(&origin))
        .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
        .with_download_started_handler(|_, _| false)
        .with_ipc_handler(move |request| {
            if let Some(pick) = Pick::from_message(request.body()) {
                let _ = picker.send_event(Command::Pick(pick));
            }
        })
        // The page still sees the drag, to highlight its drop zone; the drop itself comes here.
        .with_drag_drop_handler(move |event| {
            let DragDropEvent::Drop { paths, .. } = event else {
                return false;
            };
            let _ = dropper.send_event(Command::Dropped(paths));
            true
        })
        .build_gtk(container)?;
    listen_to_subutf8(event_loop.create_proxy());
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            }
            | Event::UserEvent(Command::Close) => *control_flow = ControlFlow::Exit,
            Event::UserEvent(Command::Raise) => window.set_focus(),
            Event::UserEvent(Command::Pick(pick)) => {
                let paths = choose(&window, pick);
                let _ = webview.evaluate_script(&picked_script(pick.message(), &paths));
            }
            Event::UserEvent(Command::Dropped(paths)) => {
                let _ = webview.evaluate_script(&picked_script(DROPPED, &paths));
            }
            _ => {}
        }
    })
}

/// The system's file picker: the desktop's own dialog through its portal when there is one,
/// with every shortcut the file manager has.
fn choose(window: &Window, pick: Pick) -> Vec<PathBuf> {
    let (title, action, accept, multiple) = match pick {
        Pick::Files => (FILES_TITLE, gtk::FileChooserAction::Open, ADD_LABEL, true),
        Pick::Folders => (
            FOLDERS_TITLE,
            gtk::FileChooserAction::SelectFolder,
            ADD_LABEL,
            true,
        ),
        Pick::OutputFolder => (
            OUTPUT_FOLDER_TITLE,
            gtk::FileChooserAction::SelectFolder,
            SELECT_LABEL,
            false,
        ),
    };
    let dialog = gtk::FileChooserNative::new(
        Some(title),
        Some(window.gtk_window()),
        action,
        Some(accept),
        Some(CANCEL_LABEL),
    );
    dialog.set_select_multiple(multiple);
    if let Pick::Files = pick {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(SUBTITLES_FILTER_NAME));
        filter.add_pattern(SUBTITLES_PATTERN);
        dialog.add_filter(filter);
    }
    if dialog.run() != gtk::ResponseType::Accept {
        return Vec::new();
    }
    dialog.filenames()
}

fn picked_script(kind: &str, paths: &[PathBuf]) -> String {
    let texts: Vec<&str> = paths.iter().filter_map(|path| path.to_str()).collect();
    let paths = serde_json::to_string(&texts).unwrap_or_default();
    let kind = serde_json::to_string(kind).unwrap_or_default();
    format!("window.{PICKED_CALLBACK}?.({kind}, {paths});")
}

/// `http://127.0.0.1:40123/#token=…` becomes `http://127.0.0.1:40123/`.
fn origin_of(url: &str) -> &str {
    let after_scheme = url
        .find(SCHEME_SEPARATOR)
        .map_or(0, |index| index + SCHEME_SEPARATOR.len());
    let path_start = url[after_scheme..]
        .find(PATH_SEPARATOR)
        .map_or(url.len(), |index| {
            after_scheme + index + PATH_SEPARATOR.len_utf8()
        });
    &url[..path_start]
}

fn listen_to_subutf8(proxy: EventLoopProxy<Command>) {
    thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else {
                break;
            };
            if line.trim() == RAISE_COMMAND {
                let _ = proxy.send_event(Command::Raise);
            }
        }
        let _ = proxy.send_event(Command::Close);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_keeps_scheme_host_and_port_only() {
        assert_eq!(
            origin_of("http://127.0.0.1:40123/#token=abc"),
            "http://127.0.0.1:40123/"
        );
        assert_eq!(origin_of("http://localhost:1"), "http://localhost:1");
    }

    /// Paths reach the page as JSON, so quotes and other characters cannot break the script.
    #[test]
    fn picked_paths_are_passed_as_json() {
        let paths = [
            PathBuf::from("/tmp/a \"b\".srt"),
            PathBuf::from("/tmp/c\\d"),
        ];
        assert_eq!(
            picked_script(Pick::Files.message(), &paths),
            r#"window.subutf8Picked?.("files", ["/tmp/a \"b\".srt","/tmp/c\\d"]);"#
        );
    }
}

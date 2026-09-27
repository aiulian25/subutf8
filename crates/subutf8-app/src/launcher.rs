use std::env;
use std::process::{Command as BlockingCommand, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, watch};

use crate::constants::{
    BROWSER_OPENER, PORTAL_SETTING, PORTAL_VARIABLE, RAISE_WINDOW_LINE, RUNNING_MESSAGE,
    WEBKIT_RENDERER_SETTING, WEBKIT_RENDERER_VARIABLE, WINDOW_PROGRAM, WINDOW_URL_VARIABLE,
};

/// How the desktop builds show the interface: in the app's own window when `subutf8-window`
/// sits next to this program, and otherwise, or if the window cannot start, in the browser.
pub struct Interface {
    url: String,
    window_open: AtomicBool,
    /// The window's standard input: `raise` lines bring it to the front, and when this
    /// program stops, the input closes and the window closes with it.
    window_input: Mutex<Option<ChildStdin>>,
    shutdown: Arc<watch::Sender<bool>>,
}

impl Interface {
    pub fn new(url: String, shutdown: Arc<watch::Sender<bool>>) -> Arc<Self> {
        Arc::new(Self {
            url,
            window_open: AtomicBool::new(false),
            window_input: Mutex::new(None),
            shutdown,
        })
    }

    pub fn is_window_open(&self) -> bool {
        self.window_open.load(Ordering::Relaxed)
    }

    /// Opens the window, or brings it to the front when it is already open; falls back to
    /// the browser.
    pub fn show(self: &Arc<Self>) {
        if self.is_window_open() {
            tokio::spawn(Arc::clone(self).raise_window());
            return;
        }
        let program = env::current_exe()
            .ok()
            .map(|executable| executable.with_file_name(WINDOW_PROGRAM))
            .filter(|program| program.is_file());
        let Some(program) = program else {
            self.open_browser();
            return;
        };
        // The address carries the access token, and other users can read a process's
        // arguments but not its environment. The DMA-BUF renderer shows a blank window
        // with some graphics drivers, and the plain renderer is fast enough for this page.
        let spawned = Command::new(program)
            .env(WINDOW_URL_VARIABLE, &self.url)
            .env(WEBKIT_RENDERER_VARIABLE, WEBKIT_RENDERER_SETTING)
            .env(PORTAL_VARIABLE, PORTAL_SETTING)
            .stdin(Stdio::piped())
            .kill_on_drop(true)
            .spawn();
        match spawned {
            Ok(child) => {
                self.window_open.store(true, Ordering::Relaxed);
                tokio::spawn(Arc::clone(self).watch_window(child));
            }
            Err(_) => self.open_browser(),
        }
    }

    async fn raise_window(self: Arc<Self>) {
        if let Some(input) = self.window_input.lock().await.as_mut() {
            let _ = input.write_all(RAISE_WINDOW_LINE.as_bytes()).await;
        }
    }

    /// Closing the window quits the app (ACCESS-09). A window that fails, for example
    /// because WebKitGTK is missing, falls back to the browser.
    async fn watch_window(self: Arc<Self>, mut child: Child) {
        *self.window_input.lock().await = child.stdin.take();
        let closed_normally = child.wait().await.is_ok_and(|status| status.success());
        self.window_open.store(false, Ordering::Relaxed);
        *self.window_input.lock().await = None;
        if closed_normally {
            let _ = self.shutdown.send(true);
            return;
        }
        self.open_browser();
    }

    /// ACCESS-07: the address is printed only when no browser can be opened.
    fn open_browser(&self) {
        let url = self.url.clone();
        tokio::task::spawn_blocking(move || {
            let opened = BlockingCommand::new(BROWSER_OPENER)
                .arg(&url)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success());
            if !opened {
                println!("{RUNNING_MESSAGE} {url}");
            }
        });
    }
}

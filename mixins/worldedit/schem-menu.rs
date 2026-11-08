// The schematics menu: `//schem` with no argument, over a world this game
// owns. It lists `<world>/schematics`, loads the picked file into the editor's
// clipboard and pastes it, and opens the folder so files can be dragged in.
//
// The screen is the pause menu's (`Screen::Paused` with `pause_menu` set): the
// game already carries that screen's mouse and keys, so `draw_pause_menu`,
// `pause_click` and `key_pressed` are hooked in `horizon-mixin.toml` and hand
// over to this module while a menu is open, and to the pause menu otherwise.
//
// The open menu lives in a thread-local, not in `Game`'s editor state: the
// editor's state is leased for the length of a command (`with_state`), and a
// click here runs commands, which lease it in turn.
use super::{Game, Screen};
use horizon_client::chat::worldedit_paths::{self, Entry};
use horizon_client::hud::HudBatch;
use horizon_client::text::{self, Text};
use horizon_client::ui;
use std::cell::RefCell;
use std::path::PathBuf;
use winit::keyboard::KeyCode;

/// The list's width and a row's height.
const WIDTH: f32 = 340.0;
const ROW: f32 = 12.0;
/// Where the list starts, and the room the buttons and their hint take below.
const TOP: f32 = 42.0;
const FOOTER: f32 = 52.0;
/// How far the first row sits inside the list's frame.
const ROW_INSET: f32 = 3.0;

/// What the open menu holds.
#[derive(Default)]
struct Menu {
    dir: PathBuf,
    entries: Vec<Entry>,
    selected: Option<usize>,
    /// The first row the list shows.
    scroll: usize,
}

impl Menu {
    /// Re-reads the folder, keeping the pick when that file is still there.
    fn refresh(&mut self) {
        let kept = self
            .selected
            .and_then(|i| self.entries.get(i))
            .map(|entry| entry.file.clone());
        self.entries = worldedit_paths::entries(&self.dir);
        self.selected = kept
            .and_then(|file| self.entries.iter().position(|entry| entry.file == file))
            .or_else(|| (!self.entries.is_empty()).then_some(0));
        self.scroll = 0;
    }

    /// Keeps the pick on screen, and the scroll within the list.
    fn settle(&mut self, rows: usize) {
        let last = self.entries.len().saturating_sub(rows.max(1));
        self.scroll = self.scroll.min(last);
        if let Some(selected) = self.selected {
            if selected < self.scroll {
                self.scroll = selected;
            }
            if selected >= self.scroll + rows.max(1) {
                self.scroll = selected + 1 - rows.max(1);
            }
        }
    }

    fn picked(&self) -> Option<Entry> {
        self.selected.and_then(|i| self.entries.get(i)).cloned()
    }
}

thread_local! {
    static MENU: RefCell<Option<Menu>> = const { RefCell::new(None) };
}

/// Where the menu draws, from the screen's size.
struct Layout {
    x: f32,
    top: f32,
    bottom: f32,
    rows: usize,
    buttons_y: f32,
}

impl Layout {
    fn of(width: f32, height: f32) -> Self {
        let bottom = height - FOOTER;
        Self {
            x: (width / 2.0).floor() - WIDTH / 2.0,
            top: TOP,
            bottom,
            rows: (((bottom - TOP) / ROW).floor().max(1.0)) as usize,
            buttons_y: height - 27.0,
        }
    }

    /// `Load & Paste`, `Open Folder`, `Done`.
    fn buttons(&self) -> [(f32, f32, &'static str); 3] {
        [
            (self.x, 100.0, "Load & Paste"),
            (self.x + 110.0, 130.0, "Open Folder"),
            (self.x + 250.0, 90.0, "Done"),
        ]
    }
}

/// What a click on the menu landed on.
enum Hit {
    Row(usize),
    Paste,
    Folder,
    Refresh,
    Done,
    /// The menu's own background: the pause menu must not act under it.
    Elsewhere,
}

/// `text` cut from the front with `…` until it fits `max` pixels.
fn short_path(atlas: &horizon_client::hud::HudAtlas, line: &str, max: f32) -> String {
    if atlas.font.width(line) <= max {
        return line.to_string();
    }
    let chars: Vec<char> = line.chars().collect();
    let mut keep = chars.len();
    while keep > 1 {
        keep -= 1;
        let short: String = std::iter::once('…')
            .chain(chars[chars.len() - keep..].iter().copied())
            .collect();
        if atlas.font.width(&short) <= max {
            return short;
        }
    }
    "…".to_string()
}

fn refresh_button(width: f32) -> (f32, f32, f32, f32) {
    (width - 74.0, 6.0, 68.0, 16.0)
}

fn hit(menu: &Menu, layout: &Layout, width: f32, cursor: (f32, f32)) -> Hit {
    for (x, w, what) in layout.buttons() {
        if ui::hovered(cursor, x, layout.buttons_y, w, 20.0) {
            return match what {
                "Load & Paste" => Hit::Paste,
                "Open Folder" => Hit::Folder,
                _ => Hit::Done,
            };
        }
    }
    let (rx, ry, rw, rh) = refresh_button(width);
    if ui::hovered(cursor, rx, ry, rw, rh) {
        return Hit::Refresh;
    }
    if ui::hovered(
        cursor,
        layout.x,
        layout.top,
        WIDTH,
        layout.bottom - layout.top,
    ) {
        let row = (cursor.1 - layout.top - ROW_INSET) / ROW;
        if row >= 0.0 {
            let index = menu.scroll + row as usize;
            if index < menu.entries.len() {
                return Hit::Row(index);
            }
        }
    }
    Hit::Elsewhere
}

/// Opens the menu over the world this game owns. False when it has none: a
/// guest of someone else's world leaves `//schem` to that world's server, and
/// a game without a window (a dedicated server's console) to the chat list.
pub(super) fn open(game: &mut Game<'_>) -> bool {
    if game.renderer.is_none() || !(game.integrated.is_some() || game.remote.is_none()) {
        return false;
    }
    let dir = if game.world_root.as_os_str().is_empty() {
        worldedit_paths::schematics()
    } else {
        Some(worldedit_paths::ensure_world(&game.world_root))
    };
    let Some(dir) = dir else {
        game.command_failure(Text::literal(
            "No world folder yet: //schem lists the schematics of the world you are in",
        ));
        return false;
    };
    let mut menu = Menu {
        dir,
        ..Menu::default()
    };
    menu.refresh();
    MENU.with(|slot| *slot.borrow_mut() = Some(menu));
    game.screen = Screen::Paused;
    game.pause_menu = true;
    game.capture(false);
    true
}

/// Whether a menu is open.
fn is_open() -> bool {
    MENU.with(|slot| slot.borrow().is_some())
}

/// Closes the menu and gives the mouse back to the world.
pub(super) fn close(game: &mut Game<'_>) {
    MENU.with(|slot| *slot.borrow_mut() = None);
    if game.screen == Screen::Paused {
        game.screen = Screen::Playing;
        game.capture(true);
    }
}

/// The open menu, or the pause menu.
pub(super) fn draw(game: &mut Game<'_>, batch: &mut HudBatch) -> bool {
    if !is_open() {
        return false;
    }
    let (width, height) = (batch.width, batch.height);
    let cursor = game.cursor;
    let layout = Layout::of(width, height);
    let atlas = &game.gui.atlas;
    let white = u32::MAX;
    let gray = 0xFFA0_A0A0;
    MENU.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(menu) = slot.as_mut() else {
            return false;
        };
        menu.settle(layout.rows);
        ui::menu_background(batch, atlas, 0.0, 0.0, width, height, true);
        // The title, and the folder it lists.
        let title = Text::literal("Schematics");
        let tw = atlas.font.width_text(&title);
        batch.text_styled(
            atlas,
            &title,
            ((width - tw) / 2.0).floor(),
            10.0,
            white,
            true,
        );
        // The path, shortened from the front so a deep world folder still
        // fits and keeps its own name visible.
        ui::centered_text(
            batch,
            atlas,
            &Text::colored(
                short_path(atlas, &menu.dir.display().to_string(), width - 24.0),
                text::GRAY,
            ),
            0.0,
            24.0,
            width,
            white,
        );
        let (rx, ry, rw, rh) = refresh_button(width);
        ui::button(
            batch,
            atlas,
            rx,
            ry,
            rw,
            rh,
            &Text::literal("Refresh"),
            true,
            ui::hovered(cursor, rx, ry, rw, rh),
        );
        // The list's frame.
        batch.fill(
            atlas,
            layout.x,
            layout.top,
            WIDTH,
            layout.bottom - layout.top,
            0x8000_0000,
        );
        for (x, y, w, h) in [
            (layout.x, layout.top, WIDTH, 1.0),
            (layout.x, layout.bottom - 1.0, WIDTH, 1.0),
            (layout.x, layout.top, 1.0, layout.bottom - layout.top),
            (
                layout.x + WIDTH - 1.0,
                layout.top,
                1.0,
                layout.bottom - layout.top,
            ),
        ] {
            batch.fill(atlas, x, y, w, h, 0xFF50_5050);
        }
        if menu.entries.is_empty() {
            for (i, line) in [
                "This folder has no schematics yet.",
                "Drop a .schem, .litematic or .schematic",
                "file into it, then press Refresh.",
            ]
            .iter()
            .enumerate()
            {
                ui::centered_text(
                    batch,
                    atlas,
                    &Text::colored(*line, text::GRAY),
                    layout.x + 4.0,
                    layout.top + 14.0 + i as f32 * ROW,
                    WIDTH - 8.0,
                    white,
                );
            }
        }
        let last = (menu.scroll + layout.rows).min(menu.entries.len());
        for (visible, index) in (menu.scroll..last).enumerate() {
            let y = layout.top + ROW_INSET + visible as f32 * ROW;
            let entry = &menu.entries[index];
            if menu.selected == Some(index) {
                batch.fill(atlas, layout.x + 2.0, y, WIDTH - 4.0, ROW, 0x40FF_FFFF);
            } else if ui::hovered(cursor, layout.x, y, WIDTH, ROW) {
                batch.fill(atlas, layout.x + 2.0, y, WIDTH - 4.0, ROW, 0x20FF_FFFF);
            }
            // The whole file name, extension and all: two files can share a
            // stem (`harbor.litematic`, `harbor.schematic`).
            batch.text(atlas, &entry.file, layout.x + 6.0, y + 2.0, white, true);
            let meta = format!(
                "{} · {}",
                entry.format,
                worldedit_paths::human_size(entry.bytes)
            );
            let mw = atlas.font.width(&meta);
            batch.text(
                atlas,
                &meta,
                layout.x + WIDTH - 8.0 - mw,
                y + 2.0,
                gray,
                true,
            );
        }
        if menu.entries.len() > layout.rows {
            ui::scrollbar(
                batch,
                atlas,
                layout.x + WIDTH - 5.0,
                layout.top + 2.0,
                layout.bottom - 2.0,
                menu.entries.len() as f32 * ROW,
                menu.scroll as f32 * ROW,
            );
        }
        let hint = Text::colored("Arrows pick · Enter loads · Esc closes", text::GRAY);
        ui::centered_text(
            batch,
            atlas,
            &hint,
            layout.x,
            layout.buttons_y - 13.0,
            WIDTH,
            white,
        );
        for (x, w, label) in layout.buttons() {
            let active = !(label == "Load & Paste" && menu.selected.is_none());
            ui::button(
                batch,
                atlas,
                x,
                layout.buttons_y,
                w,
                20.0,
                &Text::literal(label),
                active,
                ui::hovered(cursor, x, layout.buttons_y, w, 20.0),
            );
        }
        true
    })
}

/// A click on the menu; false when none is open (the pause menu's own click).
pub(super) fn click(game: &mut Game<'_>) -> bool {
    if !is_open() {
        return false;
    }
    let (width, height) = game.gui_size();
    let cursor = game.cursor;
    let layout = Layout::of(width, height);
    let hit = MENU.with(|slot| {
        slot.borrow()
            .as_ref()
            .map_or(Hit::Elsewhere, |menu| hit(menu, &layout, width, cursor))
    });
    match hit {
        Hit::Paste => paste_picked(game),
        Hit::Folder => {
            let dir = MENU.with(|slot| slot.borrow().as_ref().map(|menu| menu.dir.clone()));
            if let Some(dir) = dir {
                game.audio.play_button_click();
                super::packs::open_folder(&dir);
            }
        }
        Hit::Refresh => {
            game.audio.play_button_click();
            MENU.with(|slot| {
                if let Some(menu) = slot.borrow_mut().as_mut() {
                    menu.refresh();
                }
            });
        }
        Hit::Done => {
            game.audio.play_button_click();
            close(game);
        }
        Hit::Row(index) => {
            game.audio.play_button_click();
            MENU.with(|slot| {
                if let Some(menu) = slot.borrow_mut().as_mut() {
                    menu.selected = Some(index);
                }
            });
        }
        Hit::Elsewhere => {}
    }
    true
}

/// A key while the menu is up; false when none is open.
pub(super) fn key(game: &mut Game<'_>, key: KeyCode) -> bool {
    if !is_open() {
        return false;
    }
    let (width, height) = game.gui_size();
    let rows = Layout::of(width, height).rows;
    match key {
        KeyCode::Escape => close(game),
        KeyCode::Enter | KeyCode::NumpadEnter => paste_picked(game),
        KeyCode::ArrowDown => step(game, 1),
        KeyCode::ArrowUp => step(game, -1),
        KeyCode::PageDown => step(game, rows as i64),
        KeyCode::PageUp => step(game, -(rows as i64)),
        KeyCode::Home | KeyCode::End => {
            let last =
                MENU.with(|slot| slot.borrow().as_ref().map_or(0, |menu| menu.entries.len()));
            set_pick(
                game,
                if key == KeyCode::Home {
                    0
                } else {
                    last.saturating_sub(1)
                },
            );
        }
        KeyCode::F5 => {
            MENU.with(|slot| {
                if let Some(menu) = slot.borrow_mut().as_mut() {
                    menu.refresh();
                }
            });
        }
        _ => {}
    }
    true
}

/// Moves the pick by `by` rows, keeping it inside the list.
fn step(game: &mut Game<'_>, by: i64) {
    let next = MENU.with(|slot| {
        slot.borrow().as_ref().and_then(|menu| {
            let last = menu.entries.len().checked_sub(1)?;
            let from = menu.selected.unwrap_or(0) as i64;
            Some((from + by).clamp(0, last as i64) as usize)
        })
    });
    if let Some(next) = next {
        set_pick(game, next);
    }
}

fn set_pick(game: &mut Game<'_>, index: usize) {
    let moved = MENU.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(menu) = slot.as_mut() else {
            return false;
        };
        if index >= menu.entries.len() || menu.selected == Some(index) {
            return false;
        }
        menu.selected = Some(index);
        true
    });
    if moved {
        game.audio.play_button_click();
    }
}

/// `Load & Paste`: the picked file into the clipboard, then where you stand.
fn paste_picked(game: &mut Game<'_>) {
    let entry = MENU.with(|slot| slot.borrow().as_ref().and_then(Menu::picked));
    let Some(entry) = entry else {
        return;
    };
    game.audio.play_button_click();
    close(game);
    // The server of this world owns the clipboard: send both commands to it,
    // in order, the way a typed line would go.
    send(game, &format!("//schem load {}", entry.file));
    send(game, "//paste");
}

/// A command line, to the world's server (this window's own, or another's).
fn send(game: &mut Game<'_>, line: &str) {
    if game.remote.is_some() {
        game.send_chat(line);
    } else if let Some(command) = line.strip_prefix('/') {
        game.run_command(command);
    }
}

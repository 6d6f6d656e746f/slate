use std::fs::{self, OpenOptions};
use std::path::{Component, Path, PathBuf};

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Sidebar, SidebarEvent};
use crate::file_tree::FileTree;

const DIALOG_WIDTH: f32 = 380.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CreateKind {
    File,
    Folder,
}

impl CreateKind {
    fn title(self) -> &'static str {
        match self {
            Self::File => "New File",
            Self::Folder => "New Folder",
        }
    }

    fn placeholder(self) -> &'static str {
        match self {
            Self::File => "filename.ext",
            Self::Folder => "folder-name",
        }
    }
}

#[derive(Debug, Clone)]
enum NameDialogMode {
    Create { parent_dir: PathBuf, kind: CreateKind },
    Rename { old_path: PathBuf, is_dir: bool },
}

/// Validates a user-entered file or folder name relative to its target parent.
/// Allows relative subpaths like `sub/file.rs`, while rejecting empty names,
/// absolute paths, and `.` / `..` segments.
pub(crate) fn validate_entry_name(raw: &str) -> Result<PathBuf, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Enter a name.");
    }
    if trimmed.contains('\0') {
        return Err("Invalid character in name.");
    }
    let path = Path::new(trimmed);
    if path.is_absolute() {
        return Err("Enter a relative name.");
    }
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => clean.push(part),
            _ => return Err("Name cannot contain '.' or '..'."),
        }
    }
    if clean.as_os_str().is_empty() {
        return Err("Enter a name.");
    }
    Ok(clean)
}

fn set_dialog_error(error: &Entity<Option<SharedString>>, message: &'static str, window: &mut Window, cx: &mut App) {
    error.update(cx, |err, cx| {
        *err = Some(message.into());
        cx.notify();
    });
    window.refresh();
}

impl Sidebar {
    pub(super) fn prompt_create(
        &mut self,
        parent_dir: PathBuf,
        kind: CreateKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let location = self.relative_dir_label(&parent_dir);
        self.open_name_dialog(
            kind.title(),
            location,
            kind.placeholder(),
            "Create",
            None,
            NameDialogMode::Create { parent_dir, kind },
            window,
            cx,
        );
    }

    pub(super) fn prompt_rename(
        &mut self,
        old_path: PathBuf,
        is_dir: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(name) = old_path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            return;
        };
        let parent_dir = old_path
            .parent()
            .map(Path::to_path_buf)
            .or_else(|| self.root().map(Path::to_path_buf))
            .unwrap_or_default();
        let location = self.relative_dir_label(&parent_dir);
        let title = if is_dir { "Rename Folder" } else { "Rename File" };
        self.open_name_dialog(
            title,
            location,
            name.clone(),
            "Rename",
            Some((name, !is_dir)),
            NameDialogMode::Rename { old_path, is_dir },
            window,
            cx,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn open_name_dialog(
        &mut self,
        title: &'static str,
        location: String,
        placeholder: impl Into<SharedString>,
        confirm_label: &'static str,
        initial: Option<(String, bool)>,
        mode: NameDialogMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) {
            return;
        }
        let placeholder = placeholder.into();
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        if let Some((value, select_stem)) = initial {
            input.update(cx, |state, cx| {
                state.set_value(value.clone(), window, cx);
                if select_stem && let Some(dot) = value.rfind('.').filter(|&i| i > 0) {
                    state.set_selected_range(0..dot, cx);
                } else {
                    state.select_all(window, cx);
                }
            });
        }
        let error: Entity<Option<SharedString>> = cx.new(|_| None);

        let clear_error = error.clone();
        let mode_for_enter = mode.clone();
        let input_for_enter = input.clone();
        let error_for_enter = error.clone();
        let subscription = cx.subscribe_in(&input, window, move |sidebar, _, event: &InputEvent, window, cx| {
            match event {
                InputEvent::Change => {
                    if clear_error.read(cx).is_some() {
                        clear_error.update(cx, |err, cx| {
                            *err = None;
                            cx.notify();
                        });
                    }
                }
                InputEvent::PressEnter { .. } => {
                    if sidebar.confirm_name_dialog(&mode_for_enter, &input_for_enter, &error_for_enter, window, cx) {
                        window.close_dialog(cx);
                    }
                }
                _ => {}
            }
        });

        let this = cx.entity().downgrade();
        let input_for_dialog = input.clone();
        let input_for_ok = input.clone();
        let error_for_ok = error.clone();
        let mode_for_ok = mode;

        window.open_dialog(cx, move |dialog, _, _| {
            let _keep_sub = &subscription;
            let input = input_for_dialog.clone();
            let error = error.clone();
            let this_ok = this.clone();
            let this_btn = this.clone();
            let input_ok = input_for_ok.clone();
            let error_ok = error_for_ok.clone();
            let mode_ok = mode_for_ok.clone();
            let input_btn = input_for_ok.clone();
            let error_btn = error_for_ok.clone();
            let mode_btn = mode_for_ok.clone();
            let location = location.clone();

            dialog
                .w(px(DIALOG_WIDTH))
                .margin_top(px(72.))
                .title(title)
                .on_ok(move |_, window, cx| {
                    this_ok
                        .update(cx, |sidebar, cx| {
                            sidebar.confirm_name_dialog(&mode_ok, &input_ok, &error_ok, window, cx)
                        })
                        .unwrap_or(true)
                })
                .content(move |content, _, cx| {
                    let message = error.read(cx).clone();
                    content.child(
                        v_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .truncate()
                                    .child(location.clone()),
                            )
                            .child(Input::new(&input).aria_label(title))
                            .when_some(message, |col, msg| {
                                col.child(div().text_xs().text_color(cx.theme().danger).child(msg))
                            }),
                    )
                })
                .footer(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new("entry-dialog-cancel")
                                .outline()
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("entry-dialog-confirm")
                                .primary()
                                .label(confirm_label)
                                .on_click(move |_, window, cx| {
                                    let close = this_btn
                                        .update(cx, |sidebar, cx| {
                                            sidebar.confirm_name_dialog(&mode_btn, &input_btn, &error_btn, window, cx)
                                        })
                                        .unwrap_or(true);
                                    if close {
                                        window.close_dialog(cx);
                                    }
                                }),
                        ),
                )
        });
        let input_for_focus = input.clone();
        window.defer(cx, move |window, cx| {
            input_for_focus.update(cx, |state, cx| state.focus(window, cx));
        });
    }

    fn relative_dir_label(&self, dir: &Path) -> String {
        let Some(root) = self.root() else {
            return format!("In {}", dir.display());
        };
        match dir.strip_prefix(root) {
            Ok(rel) if rel.as_os_str().is_empty() => {
                let root_name = root
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| root.display().to_string());
                format!("In {root_name}/")
            }
            Ok(rel) => format!("In {}/", rel.display()),
            Err(_) => format!("In {}", dir.display()),
        }
    }

    fn confirm_name_dialog(
        &mut self,
        mode: &NameDialogMode,
        input: &Entity<InputState>,
        error: &Entity<Option<SharedString>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let raw = input.read(cx).value();
        let rel = match validate_entry_name(&raw) {
            Ok(rel) => rel,
            Err(msg) => {
                set_dialog_error(error, msg, window, cx);
                return false;
            }
        };

        match mode {
            NameDialogMode::Create { parent_dir, kind } => {
                self.confirm_create(parent_dir, *kind, &rel, error, window, cx)
            }
            NameDialogMode::Rename { old_path, is_dir } => {
                self.confirm_rename(old_path, *is_dir, &rel, error, window, cx)
            }
        }
    }

    fn confirm_create(
        &mut self,
        parent_dir: &Path,
        kind: CreateKind,
        rel: &Path,
        error: &Entity<Option<SharedString>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let target = parent_dir.join(rel);
        if target.exists() {
            set_dialog_error(error, "An item with that name already exists.", window, cx);
            return false;
        }

        let parent = parent_dir.to_path_buf();
        let target_bg = target.clone();
        let create = cx.background_spawn(async move {
            match kind {
                CreateKind::File => {
                    if let Some(dir) = target_bg.parent() {
                        fs::create_dir_all(dir)?;
                    }
                    OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&target_bg)?;
                    Ok(())
                }
                CreateKind::Folder => fs::create_dir_all(&target_bg),
            }
        });

        cx.spawn_in(window, async move |this, cx| {
            let result = create.await;
            _ = this.update_in(cx, |sidebar, window, cx| match result {
                Ok(()) => sidebar.finish_create(parent, target, kind, cx),
                Err(err) => {
                    window.push_notification(
                        Notification::error(format!("Couldn't create {}: {err}", target.display())),
                        cx,
                    );
                }
            });
        })
        .detach();

        true
    }

    fn confirm_rename(
        &mut self,
        old_path: &Path,
        is_dir: bool,
        rel: &Path,
        error: &Entity<Option<SharedString>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(old_parent) = old_path.parent().map(Path::to_path_buf) else {
            return true;
        };
        let target = old_parent.join(rel);
        if target == old_path {
            return true;
        }
        if is_dir && target.starts_with(old_path) {
            set_dialog_error(error, "Cannot move a folder into itself.", window, cx);
            return false;
        }
        if target.exists() {
            set_dialog_error(error, "An item with that name already exists.", window, cx);
            return false;
        }

        let old_bg = old_path.to_path_buf();
        let new_bg = target.clone();
        let rename = cx.background_spawn(async move {
            if let Some(dir) = new_bg.parent() {
                fs::create_dir_all(dir)?;
            }
            fs::rename(&old_bg, &new_bg)
        });

        let old_owned = old_path.to_path_buf();
        cx.spawn_in(window, async move |this, cx| {
            let result = rename.await;
            _ = this.update_in(cx, |sidebar, window, cx| match result {
                Ok(()) => sidebar.finish_rename(old_owned, target, is_dir, cx),
                Err(err) => {
                    window.push_notification(
                        Notification::error(format!("Couldn't rename {}: {err}", old_owned.display())),
                        cx,
                    );
                }
            });
        })
        .detach();

        true
    }

    fn finish_create(
        &mut self,
        parent_dir: PathBuf,
        target: PathBuf,
        kind: CreateKind,
        cx: &mut Context<Self>,
    ) {
        self.pending_select = Some(FileTree::row_id(&target));
        let stop_dir = match kind {
            CreateKind::File => target.parent().unwrap_or(&parent_dir),
            CreateKind::Folder => parent_dir.as_path(),
        };
        self.expand_and_load_ancestors(stop_dir, &parent_dir, cx);

        if kind == CreateKind::File {
            cx.emit(SidebarEvent::OpenFile(target));
        }
    }

    fn finish_rename(
        &mut self,
        old_path: PathBuf,
        new_path: PathBuf,
        is_dir: bool,
        cx: &mut Context<Self>,
    ) {
        self.pending_select = Some(FileTree::row_id(&new_path));

        if is_dir {
            if let Some(watcher) = self.watcher.as_mut() {
                watcher.unwatch_prefix(&old_path);
            }
            if let Some(tree) = self.tree.as_mut() {
                let migrated = tree.rename(&old_path, &new_path);
                if let Some(watcher) = self.watcher.as_mut() {
                    for dir in migrated {
                        watcher.watch(&dir);
                    }
                }
            }
        }

        if let Some(old_parent) = old_path.parent() {
            self.load_dir(old_parent.to_path_buf(), cx);
        }
        if let Some(new_parent) = new_path.parent()
            && Some(new_parent) != old_path.parent()
        {
            self.expand_and_load_ancestors(new_parent, new_parent, cx);
        }

        cx.emit(SidebarEvent::Renamed {
            old_path,
            new_path,
            is_dir,
        });
    }

    /// Expands folders from `stop_dir` up to `base_dir` (within tree root) and reloads them.
    fn expand_and_load_ancestors(&mut self, stop_dir: &Path, base_dir: &Path, cx: &mut Context<Self>) {
        let mut dirs_to_load = Vec::new();
        if let Some(tree) = self.tree.as_mut() {
            let root = tree.root().to_path_buf();
            let mut cursor = Some(stop_dir);
            while let Some(dir) = cursor {
                if !dir.starts_with(&root) {
                    break;
                }
                if dir != root {
                    tree.set_expanded(dir, true);
                }
                dirs_to_load.push(dir.to_path_buf());
                if dir == base_dir || dir == root {
                    break;
                }
                cursor = dir.parent();
            }
        }
        if !dirs_to_load.iter().any(|dir| dir == base_dir) {
            dirs_to_load.push(base_dir.to_path_buf());
        }
        for dir in dirs_to_load {
            self.load_dir(dir, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::validate_entry_name;

    #[test]
    fn validates_relative_entry_names() {
        assert_eq!(validate_entry_name("notes.md"), Ok(PathBuf::from("notes.md")));
        assert_eq!(validate_entry_name("  src/mod.rs  "), Ok(PathBuf::from("src/mod.rs")));
        assert!(validate_entry_name("").is_err());
        assert!(validate_entry_name("   ").is_err());
        assert!(validate_entry_name("/etc/passwd").is_err());
        assert!(validate_entry_name("../escape").is_err());
        assert!(validate_entry_name("a/../b").is_err());
        assert!(validate_entry_name(".").is_err());
        assert!(validate_entry_name("bad\0name").is_err());
    }
}

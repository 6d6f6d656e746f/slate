use std::time::Duration;

use gpui_kit::component::WindowExt as _;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Entity, InputEvent as _, Modifiers, ModifiersChangedEvent,
    Point, TestAppContext, px,
};

use super::Workspace;
use super::tests::{open_file, open_workspace, press, test_dir, type_text};
use crate::file_tree::FileTree;

fn tab_names(cx: &mut TestAppContext, workspace: &Entity<Workspace>) -> Vec<String> {
    cx.update(|cx| workspace.read(cx).buffers.iter().map(|buffer| buffer.document.display_name()).collect())
}

fn active_name(cx: &mut TestAppContext, workspace: &Entity<Workspace>) -> String {
    cx.update(|cx| workspace.read(cx).active_buffer().document.display_name())
}

fn dialog_open(cx: &mut TestAppContext, handle: AnyWindowHandle) -> bool {
    cx.update_window(handle, |_, window, cx| window.has_active_dialog(cx))
        .unwrap()
}

#[gpui_kit::test]
fn ctrl_shift_t_reopens_closed_files_newest_first(cx: &mut TestAppContext) {
    let dir = test_dir("reopen");
    let (a, b, c) = (dir.join("a.txt"), dir.join("b.txt"), dir.join("c.txt"));
    for file in [&a, &b, &c] {
        std::fs::write(file, "text").unwrap();
    }
    let (handle, workspace) = open_workspace(cx, &dir);
    for file in [&a, &b, &c] {
        open_file(cx, handle, &workspace, file);
    }
    press(cx, handle, "ctrl-w"); // c
    press(cx, handle, "ctrl-w"); // b
    std::fs::remove_file(&b).unwrap();
    press(cx, handle, "ctrl-w"); // a, which leaves a fresh untitled tab

    press(cx, handle, "ctrl-shift-t");
    assert_eq!(tab_names(cx, &workspace), ["a.txt"], "reuses the empty tab");
    press(cx, handle, "ctrl-shift-t");
    assert_eq!(tab_names(cx, &workspace), ["a.txt", "c.txt"], "b.txt is gone from disk, so it's skipped");
    press(cx, handle, "ctrl-shift-t");
    assert_eq!(tab_names(cx, &workspace), ["a.txt", "c.txt"], "nothing left to reopen");
}

#[gpui_kit::test]
fn find_in_files_opens_the_match_at_its_line(cx: &mut TestAppContext) {
    let dir = test_dir("find-in-files");
    let project = dir.join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("notes.md"), "nothing to see").unwrap();
    std::fs::write(project.join("src/lib.rs"), "fn one() {}\n\nfn two() { Needle(); }\n").unwrap();

    let (handle, workspace) = open_workspace(cx, &dir);
    cx.update(|cx| workspace.update(cx, |workspace, cx| workspace.show_folder(project.clone(), cx)));
    cx.run_until_parked();

    press(cx, handle, "ctrl-shift-f");
    assert!(dialog_open(cx, handle));
    type_text(cx, handle, "needle");
    cx.executor().advance_clock(Duration::from_millis(300));
    cx.run_until_parked();
    let found = cx.update(|cx| workspace.read(cx).find_in_files.matches.clone());
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].file.as_str(), found[0].line, found[0].column), ("src/lib.rs", 2, 11));

    press(cx, handle, "enter");
    assert!(!dialog_open(cx, handle));
    assert_eq!(active_name(cx, &workspace), "lib.rs");
    let cursor = cx.update(|cx| workspace.read(cx).active_buffer().editor.read(cx).cursor_position());
    assert_eq!((cursor.line, cursor.character), (2, 11));
}

#[gpui_kit::test]
fn find_in_files_needs_somewhere_to_look(cx: &mut TestAppContext) {
    let (handle, _) = open_workspace(cx, &test_dir("find-in-files-none"));
    press(cx, handle, "ctrl-shift-f");
    assert!(!dialog_open(cx, handle));
}

#[gpui_kit::test]
fn the_word_count_follows_edits(cx: &mut TestAppContext) {
    let (handle, workspace) = open_workspace(cx, &test_dir("word-count"));
    type_text(cx, handle, "one two\nthree");
    assert_eq!(cx.update(|cx| workspace.read(cx).active_buffer().words(cx)), 3);
    type_text(cx, handle, " four");
    assert_eq!(cx.update(|cx| workspace.read(cx).active_buffer().words(cx)), 4);
}

fn set_modifiers(cx: &mut TestAppContext, handle: AnyWindowHandle, modifiers: Modifiers) {
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_event(
            ModifiersChangedEvent {
                modifiers,
                ..Default::default()
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
fn ctrl_tab_cycles_tabs_in_mru_order_and_commits_on_ctrl_release(cx: &mut TestAppContext) {
    let dir = test_dir("mru-tabs");
    let (a, b, c) = (dir.join("a.txt"), dir.join("b.txt"), dir.join("c.txt"));
    for file in [&a, &b, &c] {
        std::fs::write(file, "text").unwrap();
    }
    let (handle, workspace) = open_workspace(cx, &dir);
    for file in [&a, &b, &c] {
        open_file(cx, handle, &workspace, file);
    }
    // Opening order leaves c active; MRU order is [c, b, a].
    assert_eq!(active_name(cx, &workspace), "c.txt");

    // Hold Ctrl and press Ctrl+Tab once: switcher opens on b.txt without activating yet.
    set_modifiers(cx, handle, Modifiers::control());
    press(cx, handle, "ctrl-tab");
    cx.update(|cx| {
        let ws = workspace.read(cx);
        let switcher = ws.tab_switcher.as_ref().expect("switcher should be open");
        assert_eq!(switcher.selected_id(), Some(ws.buffers[1].id));
        assert_eq!(ws.active_buffer().document.display_name(), "c.txt");
    });

    // Second Ctrl+Tab steps to a.txt; Ctrl+Shift+Tab steps back to b.txt.
    press(cx, handle, "ctrl-tab");
    cx.update(|cx| {
        let ws = workspace.read(cx);
        assert_eq!(ws.tab_switcher.as_ref().unwrap().selected_id(), Some(ws.buffers[0].id));
    });
    press(cx, handle, "ctrl-shift-tab");
    cx.update(|cx| {
        let ws = workspace.read(cx);
        assert_eq!(ws.tab_switcher.as_ref().unwrap().selected_id(), Some(ws.buffers[1].id));
    });

    // Release Ctrl: commits b.txt and closes the switcher; MRU becomes [b, c, a].
    set_modifiers(cx, handle, Modifiers::none());
    cx.update(|cx| assert!(workspace.read(cx).tab_switcher.is_none()));
    assert_eq!(active_name(cx, &workspace), "b.txt");

    // Next Ctrl+Tab toggles back to c.txt (most recently used before b.txt).
    set_modifiers(cx, handle, Modifiers::control());
    press(cx, handle, "ctrl-tab");
    set_modifiers(cx, handle, Modifiers::none());
    assert_eq!(active_name(cx, &workspace), "c.txt");

    // Escape cancels the switcher without changing the active tab.
    set_modifiers(cx, handle, Modifiers::control());
    press(cx, handle, "ctrl-tab");
    press(cx, handle, "escape");
    set_modifiers(cx, handle, Modifiers::none());
    cx.update(|cx| assert!(workspace.read(cx).tab_switcher.is_none()));
    assert_eq!(active_name(cx, &workspace), "c.txt");

    // Ctrl+PageDown still cycles linearly by tab strip order.
    press(cx, handle, "ctrl-pagedown");
    assert_eq!(active_name(cx, &workspace), "a.txt");
}

fn right_click_menu_and_pick(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    target: gpui_kit::ElementId,
    offset: Option<Point<gpui_kit::Pixels>>,
    menu_ix: usize,
) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        match offset {
            Some(off) => {
                let bounds = window.find(target.clone()).bounds();
                let pos = bounds.origin + off;
                window.dispatch_event(
                    gpui_kit::MouseMoveEvent {
                        position: pos,
                        pressed_button: None,
                        modifiers: Default::default(),
                    }
                    .to_platform_input(),
                    cx,
                );
                window.render_frame(cx);
                window.dispatch_event(
                    gpui_kit::MouseDownEvent {
                        button: gpui_kit::MouseButton::Right,
                        position: pos,
                        modifiers: Default::default(),
                        click_count: 1,
                        first_mouse: false,
                    }
                    .to_platform_input(),
                    cx,
                );
                window.render_frame(cx);
                window.dispatch_event(
                    gpui_kit::MouseUpEvent {
                        button: gpui_kit::MouseButton::Right,
                        position: pos,
                        modifiers: Default::default(),
                        click_count: 1,
                    }
                    .to_platform_input(),
                    cx,
                );
                window.render_frame(cx);
            }
            None => window.right_click(target, cx),
        }
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.within("popup-menu").click(menu_ix, cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
fn sidebar_right_click_creates_files_and_folders(cx: &mut TestAppContext) {
    let dir = test_dir("sidebar-create");
    let project = dir.join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("README.md"), "# Hi\n").unwrap();

    let (handle, workspace) = open_workspace(cx, &dir);
    cx.update(|cx| workspace.update(cx, |workspace, cx| workspace.show_folder(project.clone(), cx)));
    cx.run_until_parked();

    // 1. Right-click folder `src` -> New File... (menu item 0)
    let src_id = FileTree::row_id(&project.join("src"));
    right_click_menu_and_pick(cx, handle, gpui_kit::ElementId::Name(src_id), None, 0);
    assert!(dialog_open(cx, handle));

    // Empty input keeps dialog open.
    press(cx, handle, "enter");
    assert!(dialog_open(cx, handle));

    type_text(cx, handle, "lib.rs");
    press(cx, handle, "enter");
    assert!(!dialog_open(cx, handle));
    assert!(project.join("src/lib.rs").is_file());
    assert_eq!(active_name(cx, &workspace), "lib.rs");

    // 2. Right-click file `README.md` -> New Folder... (menu item 1) creates sibling in project root
    let readme_id = FileTree::row_id(&project.join("README.md"));
    right_click_menu_and_pick(cx, handle, gpui_kit::ElementId::Name(readme_id.clone()), None, 1);
    assert!(dialog_open(cx, handle));
    type_text(cx, handle, "docs");
    press(cx, handle, "enter");
    assert!(!dialog_open(cx, handle));
    assert!(project.join("docs").is_dir());

    // 3. Duplicate name keeps dialog open, Escape cancels.
    right_click_menu_and_pick(cx, handle, gpui_kit::ElementId::Name(readme_id.clone()), None, 1);
    assert!(dialog_open(cx, handle));
    type_text(cx, handle, "docs");
    press(cx, handle, "enter");
    assert!(dialog_open(cx, handle));
    press(cx, handle, "escape");
    assert!(!dialog_open(cx, handle));

    // 4. Right-click empty area of sidebar -> New File... (menu item 0) creates in root
    right_click_menu_and_pick(
        cx,
        handle,
        gpui_kit::ElementId::Name("sidebar-tree-wrap".into()),
        Some(Point::new(px(40.), px(300.))),
        0,
    );
    assert!(dialog_open(cx, handle));
    type_text(cx, handle, "root.txt");
    press(cx, handle, "enter");
    assert!(!dialog_open(cx, handle));
    assert!(project.join("root.txt").is_file());
    assert_eq!(active_name(cx, &workspace), "root.txt");
}

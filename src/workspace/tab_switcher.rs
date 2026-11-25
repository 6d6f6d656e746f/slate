use std::path::Path;

use gpui_kit::component::{ActiveTheme as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::KEY_CONTEXT;
use super::Workspace;
use super::buffer::BufferId;
use super::chrome::display_dir;

actions!(
    slate,
    [
        NextTabMru,
        PreviousTabMru,
        SwitcherUp,
        SwitcherDown,
        SwitcherConfirm,
        SwitcherCancel,
    ]
);

const SWITCHER_CONTEXT: &str = "TabSwitcher";
const SWITCHER_WIDTH: f32 = 440.;

pub(super) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-tab", NextTabMru, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-shift-tab", PreviousTabMru, Some(KEY_CONTEXT)),
        KeyBinding::new("up", SwitcherUp, Some(SWITCHER_CONTEXT)),
        KeyBinding::new("down", SwitcherDown, Some(SWITCHER_CONTEXT)),
        KeyBinding::new("enter", SwitcherConfirm, Some(SWITCHER_CONTEXT)),
        KeyBinding::new("escape", SwitcherCancel, Some(SWITCHER_CONTEXT)),
    ]);
}

/// Deferred MRU tab switcher shown while Ctrl is held during Ctrl+Tab cycling.
pub(super) struct TabSwitcher {
    order: Vec<BufferId>,
    selected: usize,
    focus_handle: FocusHandle,
}

impl TabSwitcher {
    #[cfg(test)]
    pub(super) fn selected_id(&self) -> Option<BufferId> {
        self.order.get(self.selected).copied()
    }
}

impl Workspace {
    /// Ensures `mru` holds every open buffer (most recent first) and drops closed ones.
    pub(super) fn sync_mru(&mut self) {
        let buffers = &self.buffers;
        self.mru.retain(|id| buffers.iter().any(|buffer| buffer.id == *id));
        for buffer in buffers {
            if !self.mru.contains(&buffer.id) {
                self.mru.push(buffer.id);
            }
        }
    }

    /// Moves `id` to the front of the MRU list.
    pub(super) fn touch_mru(&mut self, id: BufferId) {
        self.mru.retain(|existing| *existing != id);
        self.mru.insert(0, id);
    }

    pub(super) fn next_tab_mru(&mut self, _: &NextTabMru, window: &mut Window, cx: &mut Context<Self>) {
        self.step_mru(true, window, cx);
    }

    pub(super) fn previous_tab_mru(&mut self, _: &PreviousTabMru, window: &mut Window, cx: &mut Context<Self>) {
        self.step_mru(false, window, cx);
    }

    fn step_mru(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return;
        }
        self.sync_mru();
        if self.mru.len() < 2 {
            return;
        }

        // If Ctrl is not held (e.g. triggered from Command Palette or a single-press
        // key simulation), jump directly to the next/previous MRU tab.
        if self.tab_switcher.is_none() && !window.modifiers().control {
            let target = if forward { self.mru[1] } else { self.mru[self.mru.len() - 1] };
            if let Some(index) = self.index_of(target) {
                self.activate(index, window, cx);
            }
            return;
        }

        if let Some(switcher) = self.tab_switcher.as_mut() {
            let len = switcher.order.len();
            if len > 0 {
                switcher.selected = if forward {
                    (switcher.selected + 1) % len
                } else {
                    (switcher.selected + len - 1) % len
                };
            }
        } else {
            let order = self.mru.clone();
            let selected = if forward { 1 } else { order.len() - 1 };
            let focus_handle = cx.focus_handle();
            focus_handle.focus(window, cx);
            self.tab_switcher = Some(TabSwitcher {
                order,
                selected,
                focus_handle,
            });
        }
        cx.notify();
    }

    pub(super) fn on_modifiers_changed(
        &mut self,
        event: &ModifiersChangedEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tab_switcher.is_some() && !event.modifiers.control {
            self.commit_tab_switcher(window, cx);
        }
    }

    fn switcher_up(&mut self, _: &SwitcherUp, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(switcher) = self.tab_switcher.as_mut()
            && !switcher.order.is_empty()
        {
            let len = switcher.order.len();
            switcher.selected = (switcher.selected + len - 1) % len;
            cx.notify();
        }
    }

    fn switcher_down(&mut self, _: &SwitcherDown, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(switcher) = self.tab_switcher.as_mut()
            && !switcher.order.is_empty()
        {
            switcher.selected = (switcher.selected + 1) % switcher.order.len();
            cx.notify();
        }
    }

    fn switcher_confirm(&mut self, _: &SwitcherConfirm, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_tab_switcher(window, cx);
    }

    fn switcher_cancel(&mut self, _: &SwitcherCancel, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_tab_switcher(window, cx);
    }

    fn commit_tab_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(switcher) = self.tab_switcher.take() else {
            return;
        };
        if let Some(index) = switcher
            .order
            .get(switcher.selected)
            .and_then(|id| self.index_of(*id))
        {
            self.activate(index, window, cx);
        } else {
            let editor = self.active_buffer().editor.clone();
            editor.update(cx, |state, cx| state.focus(window, cx));
            cx.notify();
        }
    }

    pub(super) fn cancel_tab_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab_switcher.take().is_some() {
            let editor = self.active_buffer().editor.clone();
            editor.update(cx, |state, cx| state.focus(window, cx));
            cx.notify();
        }
    }

    pub(super) fn render_tab_switcher(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let switcher = self.tab_switcher.as_ref()?;
        let theme = cx.theme();

        let rows: Vec<AnyElement> = switcher
            .order
            .iter()
            .enumerate()
            .filter_map(|(row_ix, &id)| {
                let buf_ix = self.index_of(id)?;
                let buffer = &self.buffers[buf_ix];
                let selected = row_ix == switcher.selected;
                let name = buffer.document.display_name();
                let dirty = buffer.document.is_dirty();
                let accent = self.tab_hsla(buf_ix, buffer, cx);
                let dir = buffer
                    .document
                    .path()
                    .and_then(Path::parent)
                    .map(display_dir);

                Some(
                    h_flex()
                        .id(("tab-switcher-row", id.0))
                        .w_full()
                        .h(px(32.))
                        .px_3()
                        .gap_2()
                        .items_center()
                        .rounded(theme.radius)
                        .cursor_pointer()
                        .when(selected, |row| {
                            row.bg(theme.list_active).text_color(theme.accent_foreground)
                        })
                        .when(!selected, |row| {
                            row.text_color(theme.popover_foreground)
                                .hover(|style| style.bg(theme.list_hover))
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.tab_switcher = None;
                                if let Some(index) = this.index_of(id) {
                                    this.activate(index, window, cx);
                                }
                            }),
                        )
                        .child(
                            div()
                                .w(px(3.))
                                .h(px(14.))
                                .rounded_full()
                                .flex_shrink_0()
                                .when_some(accent, |dot, color| dot.bg(color)),
                        )
                        .child(div().flex_none().text_sm().child(name))
                        .when(dirty, |row| {
                            row.child(div().size_1p5().rounded_full().flex_shrink_0().bg(theme.primary))
                        })
                        .when_some(dir, |row, dir| {
                            row.child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_xs()
                                    .text_right()
                                    .text_color(theme.muted_foreground)
                                    .child(dir),
                            )
                        })
                        .into_any_element(),
                )
            })
            .collect();

        let panel = v_flex()
            .id("tab-switcher")
            .key_context(SWITCHER_CONTEXT)
            .track_focus(&switcher.focus_handle)
            .on_action(cx.listener(Self::switcher_up))
            .on_action(cx.listener(Self::switcher_down))
            .on_action(cx.listener(Self::switcher_confirm))
            .on_action(cx.listener(Self::switcher_cancel))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .w(px(SWITCHER_WIDTH))
            .max_h(px(360.))
            .p_1p5()
            .gap_0p5()
            .overflow_y_scroll()
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius_lg)
            .shadow_lg()
            .children(rows);

        Some(
            div()
                .id("tab-switcher-backdrop")
                .absolute()
                .inset_0()
                .flex()
                .justify_center()
                .items_start()
                .pt(px(72.))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.cancel_tab_switcher(window, cx)),
                )
                .child(panel)
                .into_any_element(),
        )
    }
}

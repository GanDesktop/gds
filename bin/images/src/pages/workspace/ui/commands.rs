//! Shared dispatch for the toolbar, inspector, application menu and keyboard.
use super::*;
use crate::{
    entities::document::SpatialTransformOp as Op,
    features::{
        edit_canvas::{CancelOutcome, ReorderOp, ToolMode},
        navigation::{self, ZoomPreset},
    },
    shared::geometry::FitMode,
};
use gds::ui::IconName;
use zgui::{
    view::events,
    vocab::{Key, NamedKey},
};

macro_rules! commands {
    ($($name:ident => ($key:literal, $icon:ident)),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(super) enum Command {
            $($name),*
        }

        impl Command {
            pub(super) fn label(self) -> String {
                text(match self {
                    $(Self::$name => $key),*
                })
            }

            pub(super) fn icon(self) -> IconName {
                match self {
                    $(Self::$name => IconName::$icon),*
                }
            }
        }
    };
}

commands! {
    Open => ("action-open", FolderOpen),
    OpenFolder => ("action-open-folder", Folder),
    Paste => ("action-open-clipboard", Clipboard),
    Save => ("action-save", Save),
    SaveAs => ("action-save-as", Download),
    Copy => ("action-copy", Copy),
    Prev => ("action-prev", ChevronUp),
    Next => ("action-next", ChevronDown),
    Filmstrip => ("action-toggle-filmstrip", PanelLeft),
    Inspector => ("action-toggle-inspector", PanelRight),
    ZoomIn => ("action-zoom-in", ZoomIn),
    ZoomOut => ("action-zoom-out", ZoomOut),
    Fit => ("action-zoom-fit", Maximize),
    Actual => ("action-zoom-actual", ScanSearch),
    Width => ("action-zoom-fit-width", UnfoldHorizontal),
    Height => ("action-zoom-fit-height", UnfoldVertical),
    Cw => ("action-rotate-cw", RotateCw),
    Ccw => ("action-rotate-ccw", RotateCcw),
    FlipH => ("action-flip-h", FlipHorizontal2),
    FlipV => ("action-flip-v", FlipVertical2),
    Undo => ("action-undo", Undo2),
    Redo => ("action-redo", Redo2),
    Crop => ("tool-apply-crop", Crop),
    Duplicate => ("action-duplicate", CopyPlus),
    Delete => ("action-delete-object", Trash),
    Forward => ("action-bring-forward", LayersArrowUp),
    Backward => ("action-send-backward", LayersArrowDown),
    Front => ("action-bring-to-front", LayersArrowUp),
    Back => ("action-send-to-back", LayersArrowDown),
    Rename => ("action-edit-location", Pencil),
    Trash => ("action-trash", Trash),
    Reveal => ("action-reveal", Folder),
    OpenWith => ("action-open-with", ExternalLink),
    Settings => ("action-settings", Settings),
    Logs => ("action-logs", List),
    About => ("action-about", Info),
    Strip => ("inspector-strip-privacy", Shield),
}

impl Command {
    pub(super) fn shortcut(self) -> Option<gds::ui::Shortcut> {
        use Command::*;
        gds::ui::Shortcut::parse(match self {
            Open => "Ctrl+O",
            OpenFolder => "Ctrl+Shift+O",
            Paste => "Ctrl+Shift+V",
            Save => "Ctrl+S",
            SaveAs => "Ctrl+Shift+S",
            Copy => "Ctrl+C",
            Prev => "Left",
            Next => "Right",
            Filmstrip => "Ctrl+B",
            Inspector => "Ctrl+I",
            ZoomIn => "+",
            ZoomOut => "-",
            Fit => "F",
            Actual => "Ctrl+1",
            Cw => "R",
            Ccw => "Shift+R",
            FlipH => "H",
            FlipV => "V",
            Undo => "Ctrl+Z",
            Redo => "Ctrl+Shift+Z",
            Crop => "Enter",
            Duplicate => "Ctrl+D",
            Delete => "Delete",
            Forward => "]",
            Backward => "[",
            Front => "Ctrl+]",
            Back => "Ctrl+[",
            Rename => "Ctrl+L",
            Trash => "Ctrl+Delete",
            Settings => "Ctrl+,",
            Logs => "Ctrl+Shift+L",
            Width | Height | Reveal | OpenWith | About | Strip => return None,
        })
    }
}

fn is_node_focused(node: NodeRef) -> bool {
    zgui::view::focused_node()
        .get_untracked()
        .is_some_and(|focused| node.get_untracked() == Some(focused) || node.contains(focused))
}

impl Page {
    pub(super) fn enabled(self, c: Command) -> bool {
        use Command::*;
        let w = self.window();
        if w.busy.get_untracked() {
            return false;
        }
        self.read(|s| match c {
            Open | OpenFolder | Paste | Filmstrip | Inspector | Settings | About => true,
            Logs => gds::ui::logs::is_enabled(),
            Prev => s.catalog.can_step_prev(),
            Next => s.catalog.can_step_next(),
            _ if w.loading.get_untracked() || s.doc.is_none() => false,
            Undo => s.history.can_undo(),
            Redo => s.history.can_redo(),
            Crop => s.tool.staged_crop.is_some(),
            Delete => !s.tool.selection.is_empty() || s.tool.staged_crop.is_some(),
            Duplicate | Forward | Backward | Front | Back => !s.tool.selection.is_empty(),
            Reveal | Rename | Trash | OpenWith => s
                .doc
                .as_ref()
                .is_some_and(|d| d.decoded.origin.path().is_some()),
            Save | SaveAs | Copy | ZoomIn | ZoomOut | Fit | Actual | Width | Height | Cw | Ccw
            | FlipH | FlipV | Strip => true,
        })
    }

    fn edit_document(self, f: impl FnOnce(&mut DocumentState, &mut ToolFsm, &mut EditHistory)) {
        self.flush_text();
        self.cancel_gesture();
        let edited = self.change(|s| {
            let Some(doc) = s.doc.as_mut() else {
                return false;
            };
            f(doc, &mut s.tool, &mut s.history);
            true
        });
        if edited {
            self.fit();
            self.changed();
            self.render();
        }
    }

    fn apply_zoom_fit(self, mode: FitMode) {
        let vp = self.window().size.get();
        let changed = self.change(|s| {
            let Some(doc) = s.doc.as_ref() else {
                return false;
            };
            let (width, height) = doc.output_dimensions();
            s.transform =
                ViewportTransform::compute_fit(mode, vp, Size2::new(width as f32, height as f32));
            true
        });
        if changed {
            self.zoom_changed();
        }
    }

    fn apply_zoom_preset(self, preset: ZoomPreset) {
        let vp = self.window().size.get();
        let changed = self.change(|s| {
            let Some(doc) = s.doc.as_ref() else {
                return false;
            };
            let (width, height) = doc.output_dimensions();
            navigation::apply_zoom_preset(
                preset,
                &mut s.transform,
                vp,
                Size2::new(width as f32, height as f32),
            );
            true
        });
        if changed {
            self.zoom_changed();
        }
    }

    pub(super) fn action(self, c: Command) {
        use Command::*;
        if !self.enabled(c) {
            return;
        }
        let w = self.window();
        match c {
            Open | OpenFolder | SaveAs => self.pick(c),
            Paste => self.open_clipboard(),
            Save => self.save(),
            Copy => self.copy(),
            Settings => {
                self.dismiss_popovers();
                self.flush_text();
                self.close_rename();
                w.settings.update(|v| *v = !*v);
            }
            Logs => {
                self.dismiss_popovers();
                gds::ui::logs::open();
            }
            About => {
                self.dismiss_popovers();
                w.about.set(true);
            }
            Filmstrip => {
                if let Err(e) = w.prefs.edit(|p| p.filmstrip_open = !p.filmstrip_open) {
                    self.error(e);
                }
            }
            Inspector => {
                if let Err(e) = w.prefs.edit(|p| p.inspector_open = !p.inspector_open) {
                    self.error(e);
                }
            }
            Prev => {
                if let Some(path) = self.read(|s| s.catalog.peek_prev()) {
                    self.open_path(path);
                }
            }
            Next => {
                if let Some(path) = self.read(|s| s.catalog.peek_next()) {
                    self.open_path(path);
                }
            }
            Rename => {
                self.flush_text();
                let initial = self.read(|s| {
                    s.doc
                        .as_ref()
                        .map(|d| d.decoded.origin.display_name())
                        .unwrap_or_default()
                });
                self.open_rename(initial);
            }
            Trash => {
                if let Some(path) = self.path() {
                    self.request(Pending::Trash(path));
                }
            }
            Reveal => self.reveal_in_files(),
            OpenWith => self.open_with_chooser(),
            Strip => self.strip(),
            Width => self.apply_zoom_fit(FitMode::FillWidth),
            Height => self.apply_zoom_fit(FitMode::FillHeight),
            ZoomIn => self.apply_zoom_preset(ZoomPreset::ZoomIn),
            ZoomOut => self.apply_zoom_preset(ZoomPreset::ZoomOut),
            Fit => self.apply_zoom_preset(ZoomPreset::Fit),
            Actual => self.apply_zoom_preset(ZoomPreset::Actual),
            Undo => self.edit_document(|doc, tool, hist| {
                hist.undo(doc);
                tool.selection.reconcile(&doc.annotations);
            }),
            Redo => self.edit_document(|doc, tool, hist| {
                hist.redo(doc);
                tool.selection.reconcile(&doc.annotations);
            }),
            Crop => self.edit_document(|doc, tool, hist| {
                tool.apply_staged_crop(doc, hist);
            }),
            Delete => self.edit_document(|doc, tool, hist| {
                tool.delete_selected_or_crop(doc, hist);
            }),
            Duplicate => self.edit_document(|doc, tool, hist| {
                tool.duplicate_selected(doc, hist);
            }),
            Cw => self.edit_document(|doc, tool, hist| {
                tool.apply_spatial_transform(Op::RotateCw, doc, hist);
            }),
            Ccw => self.edit_document(|doc, tool, hist| {
                tool.apply_spatial_transform(Op::RotateCcw, doc, hist);
            }),
            FlipH => self.edit_document(|doc, tool, hist| {
                tool.apply_spatial_transform(Op::FlipHorizontal, doc, hist);
            }),
            FlipV => self.edit_document(|doc, tool, hist| {
                tool.apply_spatial_transform(Op::FlipVertical, doc, hist);
            }),
            Forward => self.edit_document(|doc, tool, hist| {
                tool.reorder_selection(ReorderOp::BringForward, doc, hist);
            }),
            Backward => self.edit_document(|doc, tool, hist| {
                tool.reorder_selection(ReorderOp::SendBackward, doc, hist);
            }),
            Front => self.edit_document(|doc, tool, hist| {
                tool.reorder_selection(ReorderOp::BringToFront, doc, hist);
            }),
            Back => self.edit_document(|doc, tool, hist| {
                tool.reorder_selection(ReorderOp::SendToBack, doc, hist);
            }),
        }
    }

    pub(super) fn zoom_changed(self) {
        self.repaint();
        if self.read(|s| {
            s.doc
                .as_ref()
                .is_some_and(|d| d.format() == gds::service::image::source::Format::Svg)
        }) {
            self.render();
        }
    }

    pub(super) fn select_tool(self, t: ToolMode) {
        if self.window().busy.get_untracked() {
            return;
        }
        self.flush_text();
        self.cancel_gesture();
        self.change(|s| {
            s.tool.set_tool(t, &mut s.history);
        });
        self.changed();
        self.repaint();
        self.focus();
    }

    pub(super) fn cancel_gesture(self) {
        let active = self.change(|s| {
            s.pan = None;
            if !s.tool.is_pointer_gesture() {
                return false;
            }
            s.tool.cancel_pointer_gesture(&mut s.history);
            s.gesture += 1;
            true
        });
        if active {
            self.render();
            self.repaint();
        }
    }

    pub(super) fn escape(self) {
        let w = self.window();
        if w.active_popover.get_untracked() != ActivePopover::None {
            self.dismiss_popovers();
        } else if self.is_renaming() {
            self.close_rename();
        } else if w.settings.get_untracked() {
            w.settings.set(false);
        } else {
            self.change(|s| {
                if s.tool.cancel_active(&mut s.history) == CancelOutcome::None {
                    if s.tool.selection.clear() {
                        s.history.break_coalesce();
                    } else {
                        s.tool.set_tool(ToolMode::Pointer, &mut s.history);
                    }
                }
                s.gesture += 1;
                s.pan = None;
            });
            self.close_text_editor();
            self.changed();
            self.render();
            self.repaint();
        }
        self.focus();
    }

    pub(super) fn key(self, ev: &mut EventCx<'_, events::KeyDown>) {
        use Command::*;
        use NamedKey as K;
        let w = self.window();
        if w.confirm_open.get_untracked()
            || w.about.get_untracked()
            || w.busy.get_untracked()
            || gds::ui::keytips::has_active_modal_scope()
        {
            return;
        }
        let ctrl = ev.modifiers.primary();
        let shift = ev.modifiers.shift();
        let alt = ev.modifiers.alt();
        let entry = zgui::view::focus_is_text_entry();
        let letter = if let Key::Character(c) = &ev.key {
            c.to_lowercase()
        } else {
            String::new()
        };
        if ev.key == Key::Named(K::Escape) && !ctrl && !alt {
            self.escape();
        } else if ctrl && shift && !alt && letter == "l" && !entry && gds::ui::logs::is_enabled() {
            self.action(Logs);
        } else if ctrl && !shift && !alt && letter == "q" {
            self.request(Pending::Quit);
        } else if ctrl && !shift && !alt && letter == "," {
            self.action(Settings);
        } else if w.settings.get_untracked() {
            if ctrl && !shift && !alt && letter == "b" {
                self.action(Filmstrip);
            } else if alt && !ctrl && !shift && ev.key == Key::Named(K::ArrowLeft) {
                self.escape();
            } else {
                return;
            }
        } else if !ctrl
            && !alt
            && ev.key == Key::Named(K::Enter)
            && let Some((value, node)) = self.rename_editor()
            && is_node_focused(node)
        {
            self.rename(value.get_untracked());
        } else if !ctrl
            && !alt
            && !shift
            && ev.key == Key::Named(K::Enter)
            && let Some(node) = self.text_editor_node()
            && is_node_focused(node)
        {
            self.flush_text();
            self.focus();
        } else if alt || (entry && !(ctrl && !shift && letter == "s")) {
            return;
        } else if let Some(cmd) = match (ctrl, shift, letter.as_str()) {
            (true, true, "v") => Some(Paste),
            (true, true, "s") => Some(SaveAs),
            (true, true, "z") => Some(Redo),
            (true, false, "s") => Some(Save),
            (true, false, "c") => Some(Copy),
            (true, false, "o") => Some(Open),
            (true, false, "z") => Some(Undo),
            (true, false, "y") => Some(Redo),
            (true, false, "b") => Some(Filmstrip),
            (true, false, "i") => Some(Inspector),
            (true, false, "l") => Some(Rename),
            (true, false, "d") => Some(Duplicate),
            (true, _, "]") | (_, true, "}") => Some(Front),
            (true, _, "[") | (_, true, "{") => Some(Back),
            (false, false, "]") => Some(Forward),
            (false, false, "[") => Some(Backward),
            (_, _, "+" | "=") => Some(ZoomIn),
            (_, _, "-") => Some(ZoomOut),
            (true, false, "0") | (false, false, "f") => Some(Fit),
            (true, false, "1") => Some(Actual),
            (false, true, "r") => Some(Ccw),
            (false, false, "r") => Some(Cw),
            (false, false, "h") => Some(FlipH),
            (false, false, "v") => Some(FlipV),
            (false, false, "i") => Some(Inspector),
            (true, true, "o") => Some(OpenFolder),
            _ => None,
        } {
            self.action(cmd);
        } else if ctrl && !shift && letter == "a" {
            self.cancel_gesture();
            self.change(|s| {
                if let Some(doc) = &s.doc {
                    s.tool.select_all(doc, &mut s.history);
                }
            });
            self.changed();
            self.repaint();
        } else if !ctrl
            && !shift
            && let Some(t) = match letter.as_str() {
                "1" => Some(ToolMode::Pointer),
                "2" => Some(ToolMode::Pan),
                "3" => Some(ToolMode::Crop),
                "4" => Some(ToolMode::Rect),
                "5" => Some(ToolMode::Ellipse),
                "6" => Some(ToolMode::Arrow),
                "7" => Some(ToolMode::Line),
                "b" => Some(ToolMode::Brush),
                "8" => Some(ToolMode::Text),
                "9" => Some(ToolMode::Mosaic),
                "0" => Some(ToolMode::Blur),
                "n" => Some(ToolMode::Number),
                _ => None,
            }
        {
            self.select_tool(t);
        } else {
            match ev.key {
                Key::Named(K::Delete) if ctrl && !shift => self.action(Trash),
                Key::Named(K::Delete | K::Backspace) if !ctrl && !shift => self.action(Delete),
                Key::Named(K::Enter) if !ctrl && !shift => self.action(Crop),
                Key::Named(K::Home | K::End) if !ctrl && !shift => {
                    if let Some(path) = self.read(|s| {
                        if ev.key == Key::Named(K::Home) {
                            s.catalog.first()
                        } else {
                            s.catalog.last()
                        }
                    }) {
                        self.open_path(path);
                    }
                }
                Key::Named(k @ (K::ArrowUp | K::ArrowLeft | K::ArrowRight | K::ArrowDown))
                    if !ctrl =>
                {
                    if self.read(|s| s.tool.selection.is_empty()) {
                        if shift {
                            return;
                        }
                        match k {
                            K::ArrowLeft => self.action(Prev),
                            K::ArrowRight => self.action(Next),
                            _ => return,
                        }
                    } else {
                        let step = if shift { 10. } else { 1. };
                        let (dx, dy) = match k {
                            K::ArrowUp => (0., -step),
                            K::ArrowLeft => (-step, 0.),
                            K::ArrowRight => (step, 0.),
                            _ => (0., step),
                        };
                        self.change(|s| {
                            if let Some(doc) = &mut s.doc {
                                s.tool.nudge_selection(dx, dy, doc, &mut s.history);
                            }
                        });
                        self.changed();
                        self.render();
                    }
                }
                _ => return,
            }
        }
        ev.prevent_default();
        ev.stop_propagation();
    }
}

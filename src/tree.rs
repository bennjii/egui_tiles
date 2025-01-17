use egui::{NumExt as _, Rect, Ui};

use crate::{ContainerKind, UiResponse};

use super::{
    is_possible_drag, Behavior, Container, DropContext, InsertionPoint, SimplificationOptions,
    SimplifyAction, Tile, TileId, Tiles,
};

/// The top level type. Contains all persistent state, including layouts and sizes.
///
/// You'll usually construct this once and then store it, calling [`Tree::ui`] each frame.
///
/// See [the crate-level documentation](crate) for a complete example.
///
/// ## How to constriuct a [`Tree`]
/// ```
/// use egui_tiles::{Tiles, TileId, Tree};
///
/// struct Pane { } // put some state here
///
/// let mut tiles = Tiles::default();
/// let tabs: Vec<TileId> = vec![tiles.insert_pane(Pane { }), tiles.insert_pane(Pane { })];
/// let root: TileId = tiles.insert_tab_tile(tabs);
///
/// let tree = Tree::new(root, tiles);
/// ```
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Tree<Pane> {
    /// None = empty tree
    pub root: Option<TileId>,

    /// All the tiles in the tree.
    pub tiles: Tiles<Pane>,
}

impl<Pane> Default for Tree<Pane> {
    // An empty tree
    fn default() -> Self {
        Self {
            root: None,
            tiles: Default::default(),
        }
    }
}

impl<Pane: std::fmt::Debug> std::fmt::Debug for Tree<Pane> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Print a hierarchical view of the tree:
        fn format_tile<Pane: std::fmt::Debug>(
            f: &mut std::fmt::Formatter<'_>,
            tiles: &Tiles<Pane>,
            indent: usize,
            tile_id: TileId,
        ) -> std::fmt::Result {
            write!(f, "{} {tile_id:?} ", "  ".repeat(indent))?;
            if let Some(tile) = tiles.get(tile_id) {
                match tile {
                    Tile::Pane(pane) => writeln!(f, "Pane {pane:?}"),
                    Tile::Container(container) => {
                        writeln!(
                            f,
                            "{}",
                            match container {
                                Container::Tabs(_) => "Tabs",
                                Container::Linear(_) => "Linear",
                                Container::Grid(_) => "Grid",
                            }
                        )?;
                        for &child in container.children() {
                            format_tile(f, tiles, indent + 1, child)?;
                        }
                        Ok(())
                    }
                }
            } else {
                write!(f, "DANGLING {tile_id:?}")
            }
        }

        if let Some(root) = self.root {
            writeln!(f, "Tree {{")?;
            format_tile(f, &self.tiles, 1, root)?;
            write!(f, "}}")
        } else {
            writeln!(f, "Tree {{ }}")
        }
    }
}

// ----------------------------------------------------------------------------

impl<Pane> Tree<Pane> {
    pub fn empty() -> Self {
        Self::default()
    }

    /// The most flexible constructor, allowing you to set up the tiles
    /// however you want.
    pub fn new(root: TileId, tiles: Tiles<Pane>) -> Self {
        Self {
            root: Some(root),
            tiles,
        }
    }

    /// Create a top-level [`crate::Tabs`] container with the given panes.
    pub fn new_tabs(panes: Vec<Pane>) -> Self {
        Self::new_container(ContainerKind::Tabs, panes)
    }

    /// Create a top-level horizontal [`crate::Linear`] container with the given panes.
    pub fn new_horizontal(panes: Vec<Pane>) -> Self {
        Self::new_container(ContainerKind::Horizontal, panes)
    }

    /// Create a top-level vertical [`crate::Linear`] container with the given panes.
    pub fn new_vertical(panes: Vec<Pane>) -> Self {
        Self::new_container(ContainerKind::Vertical, panes)
    }

    /// Create a top-level [`crate::Grid`] container with the given panes.
    pub fn new_grid(panes: Vec<Pane>) -> Self {
        Self::new_container(ContainerKind::Grid, panes)
    }

    /// Create a top-level container with the given panes.
    pub fn new_container(kind: ContainerKind, panes: Vec<Pane>) -> Self {
        let mut tiles = Tiles::default();
        let tile_ids = panes
            .into_iter()
            .map(|pane| tiles.insert_pane(pane))
            .collect();
        let root = tiles.insert_tile(Tile::Container(Container::new(kind, tile_ids)));
        Self::new(root, tiles)
    }

    pub fn root(&self) -> Option<TileId> {
        self.root
    }

    pub fn is_root(&self, tile: TileId) -> bool {
        self.root == Some(tile)
    }

    /// Show the tree in the given [`Ui`].
    ///
    /// The tree will use upp all the available space - nothing more, nothing less.
    pub fn ui(&mut self, behavior: &mut dyn Behavior<Pane>, ui: &mut Ui) {
        let options = behavior.simplification_options();
        self.simplify(&options);
        if options.all_panes_must_have_tabs {
            if let Some(root) = self.root {
                self.tiles.make_all_panes_children_of_tabs(false, root);
            }
        }

        self.tiles.gc_root(behavior, self.root);

        self.tiles.rects.clear();

        // Check if anything is being dragged:
        let mut drop_context = DropContext {
            enabled: true,
            dragged_tile_id: self.dragged_id(ui.ctx()),
            mouse_pos: ui.input(|i| i.pointer.hover_pos()),
            best_dist_sq: f32::INFINITY,
            best_insertion: None,
            preview_rect: None,
        };

        if let Some(root) = self.root {
            self.tiles
                .layout_tile(ui.style(), behavior, ui.available_rect_before_wrap(), root);

            self.tile_ui(behavior, &mut drop_context, ui, root);
        }

        self.preview_dragged_tile(behavior, &drop_context, ui);
    }

    pub(super) fn tile_ui(
        &mut self,
        behavior: &mut dyn Behavior<Pane>,
        drop_context: &mut DropContext,
        ui: &mut Ui,
        tile_id: TileId,
    ) {
        // NOTE: important that we get the rect and tile in two steps,
        // otherwise we could loose the tile when there is no rect.
        let Some(rect) = self.tiles.try_rect(tile_id) else {
            log::warn!("Failed to find rect for tile {tile_id:?} during ui");
            return
        };
        let Some(mut tile) = self.tiles.tiles.remove(&tile_id) else {
            log::warn!("Failed to find tile {tile_id:?} during ui");
            return
        };

        let drop_context_was_enabled = drop_context.enabled;
        if Some(tile_id) == drop_context.dragged_tile_id {
            // Can't drag a tile onto self or any children
            drop_context.enabled = false;
        }
        drop_context.on_tile(behavior, ui.style(), tile_id, rect, &tile);

        // Each tile gets its own `Ui`, nested inside each other, with proper clip rectangles.
        let mut ui = egui::Ui::new(
            ui.ctx().clone(),
            ui.layer_id(),
            ui.id().with(tile_id),
            rect,
            rect,
        );
        match &mut tile {
            Tile::Pane(pane) => {
                if behavior.pane_ui(&mut ui, tile_id, pane) == UiResponse::DragStarted {
                    ui.memory_mut(|mem| mem.set_dragged_id(tile_id.id()));
                }
            }
            Tile::Container(container) => {
                container.ui(self, behavior, drop_context, &mut ui, rect, tile_id);
            }
        };

        self.tiles.tiles.insert(tile_id, tile);
        drop_context.enabled = drop_context_was_enabled;
    }

    /// Recursively "activate" the ancestors of the tiles that matches the given predicate.
    ///
    /// This means making the matching tiles and its ancestors the active tab in any tab layout.
    pub fn make_active(&mut self, should_activate: impl Fn(&Tile<Pane>) -> bool) {
        if let Some(root) = self.root {
            self.tiles.make_active(root, &should_activate);
        }
    }

    fn preview_dragged_tile(
        &mut self,
        behavior: &mut dyn Behavior<Pane>,
        drop_context: &DropContext,
        ui: &mut Ui,
    ) {
        let (Some(mouse_pos), Some(dragged_tile_id)) =
            (drop_context.mouse_pos, drop_context.dragged_tile_id) else { return; };

        ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::Grabbing);

        // Preview what is being dragged:
        egui::Area::new(egui::Id::new((dragged_tile_id, "preview")))
            .pivot(egui::Align2::CENTER_CENTER)
            .current_pos(mouse_pos)
            .interactable(false)
            .show(ui.ctx(), |ui| {
                let mut frame = egui::Frame::popup(ui.style());
                frame.fill = frame.fill.gamma_multiply(0.5); // Make see-through
                frame.show(ui, |ui| {
                    // TODO(emilk): preview contents?
                    let text = behavior.tab_title_for_tile(&self.tiles, dragged_tile_id);
                    ui.label(text);
                });
            });

        if let Some(preview_rect) = drop_context.preview_rect {
            let preview_rect = smooth_preview_rect(ui.ctx(), dragged_tile_id, preview_rect);

            let parent_rect = drop_context
                .best_insertion
                .and_then(|insertion_point| self.tiles.try_rect(insertion_point.parent_id));

            behavior.paint_drag_preview(ui.visuals(), ui.painter(), parent_rect, preview_rect);

            if behavior.preview_dragged_panes() {
                // TODO(emilk): add support for previewing containers too.
                if preview_rect.width() > 32.0 && preview_rect.height() > 32.0 {
                    if let Some(Tile::Pane(pane)) = self.tiles.get_mut(dragged_tile_id) {
                        let _ = behavior.pane_ui(
                            &mut ui.child_ui(preview_rect, *ui.layout()),
                            dragged_tile_id,
                            pane,
                        );
                    }
                }
            }
        }

        if ui.input(|i| i.pointer.any_released()) {
            ui.memory_mut(|mem| mem.stop_dragging());
            if let Some(insertion_point) = drop_context.best_insertion {
                self.move_tile(dragged_tile_id, insertion_point);
            }
            clear_smooth_preview_rect(ui.ctx(), dragged_tile_id);
        }
    }

    fn simplify(&mut self, options: &SimplificationOptions) {
        if let Some(root) = self.root {
            match self.tiles.simplify(options, root, None) {
                SimplifyAction::Keep => {}
                SimplifyAction::Remove => {
                    self.root = None;
                }
                SimplifyAction::Replace(new_root) => {
                    self.root = Some(new_root);
                }
            }
        }
    }

    /// Move the given tile to the given insertion point.
    pub(super) fn move_tile(&mut self, moved_tile_id: TileId, insertion_point: InsertionPoint) {
        log::debug!(
            "Moving {moved_tile_id:?} into {:?}",
            insertion_point.insertion
        );
        self.remove_tile_id_from_parent(moved_tile_id);
        self.tiles.insert(insertion_point, moved_tile_id);
    }

    /// Find the currently dragged tile, if any.
    pub fn dragged_id(&self, ctx: &egui::Context) -> Option<TileId> {
        if !is_possible_drag(ctx) {
            // We're not sure we're dragging _at all_ yet.
            return None;
        }

        for &tile_id in self.tiles.tiles.keys() {
            if self.is_root(tile_id) {
                continue; // not allowed to drag root
            }

            let id = tile_id.id();
            let is_tile_being_dragged = ctx.memory(|mem| mem.is_being_dragged(id));
            if is_tile_being_dragged {
                // Abort drags on escape:
                if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                    ctx.memory_mut(|mem| mem.stop_dragging());
                    return None;
                }

                return Some(tile_id);
            }
        }
        None
    }

    /// This removes the given tile from the parents list of children.
    ///
    /// The [`Tile`] itself is not removed from [`Self::tiles`].
    ///
    /// Performs no simplifcations.
    pub(super) fn remove_tile_id_from_parent(&mut self, remove_me: TileId) {
        for parent in self.tiles.tiles.values_mut() {
            if let Tile::Container(container) = parent {
                container.retain(|child| child != remove_me);
            }
        }
    }
}

// ----------------------------------------------------------------------------

/// We store the preview rect in egui temp storage so that it is not serialized,
/// and so that a user could re-create the [`Tree`] each frame and still get smooth previews.
fn smooth_preview_rect_id(dragged_tile_id: TileId) -> egui::Id {
    egui::Id::new((dragged_tile_id, "smoothed_preview_rect"))
}

fn clear_smooth_preview_rect(ctx: &egui::Context, dragged_tile_id: TileId) {
    let data_id = smooth_preview_rect_id(dragged_tile_id);
    ctx.data_mut(|data| data.remove::<Rect>(data_id));
}

/// Take the preview rectangle and smooth it over time.
fn smooth_preview_rect(ctx: &egui::Context, dragged_tile_id: TileId, new_rect: Rect) -> Rect {
    let data_id = smooth_preview_rect_id(dragged_tile_id);

    let dt = ctx.input(|input| input.stable_dt).at_most(0.1);

    let mut requires_repaint = false;

    let smoothed = ctx.data_mut(|data| {
        let smoothed: &mut Rect = data.get_temp_mut_or(data_id, new_rect);

        let t = egui::emath::exponential_smooth_factor(0.9, 0.05, dt);

        *smoothed = smoothed.lerp_towards(&new_rect, t);

        let diff = smoothed.min.distance(new_rect.min) + smoothed.max.distance(new_rect.max);
        if diff < 0.5 {
            *smoothed = new_rect;
        } else {
            requires_repaint = true;
        }
        *smoothed
    });

    if requires_repaint {
        ctx.request_repaint();
    }

    smoothed
}

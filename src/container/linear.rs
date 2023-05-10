use egui::{pos2, vec2, NumExt, Rect};
use itertools::Itertools as _;

use crate::{
    is_being_dragged, Behavior, ContainerInsertion, DropContext, InsertionPoint, ResizeState,
    SimplifyAction, TileId, Tiles, Tree,
};

// ----------------------------------------------------------------------------

/// How large of a share of space each child has, on a 1D axis.
///
/// Used for [`Linear`] containers (horizontal and vertical).
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Shares {
    /// How large of a share each child has.
    ///
    /// For instance, the shares `[1, 2, 3]` means that the first child gets 1/6 of the space,
    /// the second gets 2/6 and the third gets 3/6.
    shares: nohash_hasher::IntMap<TileId, f32>,
}

impl Shares {
    pub fn replace_with(&mut self, remove: TileId, new: TileId) {
        if let Some(share) = self.shares.remove(&remove) {
            self.shares.insert(new, share);
        }
    }

    /// Split the given width based on the share of the children.
    pub fn split(&self, children: &[TileId], available_width: f32) -> Vec<f32> {
        let mut num_shares = 0.0;
        for &child in children {
            num_shares += self[child];
        }
        if num_shares == 0.0 {
            num_shares = 1.0;
        }
        children
            .iter()
            .map(|&child| available_width * self[child] / num_shares)
            .collect()
    }

    pub fn retain(&mut self, keep: impl Fn(TileId) -> bool) {
        self.shares.retain(|&child, _| keep(child));
    }

    pub fn sum(&self) -> f32 {
        self.shares.values().sum()
    }
}

impl<'a> IntoIterator for &'a Shares {
    type Item = (&'a TileId, &'a f32);
    type IntoIter = std::collections::hash_map::Iter<'a, TileId, f32>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.shares.iter()
    }
}

impl std::ops::Index<TileId> for Shares {
    type Output = f32;

    #[inline]
    fn index(&self, id: TileId) -> &Self::Output {
        self.shares.get(&id).unwrap_or(&1.0)
    }
}

impl std::ops::IndexMut<TileId> for Shares {
    #[inline]
    fn index_mut(&mut self, id: TileId) -> &mut Self::Output {
        self.shares.entry(id).or_insert(1.0)
    }
}

// ----------------------------------------------------------------------------

/// The direction of a [`Linear`] container. Either horizontal or vertical.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum LinearDir {
    #[default]
    Horizontal,
    Vertical,
}

/// Horizontal or vertical container.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Linear {
    pub children: Vec<TileId>,
    pub dir: LinearDir,
    pub shares: Shares,
}

impl Linear {
    pub fn new(dir: LinearDir, children: Vec<TileId>) -> Self {
        Self {
            children,
            dir,
            ..Default::default()
        }
    }

    /// Create a binary split with the given split ratio in the 0.0 - 1.0 range.
    ///
    /// The `fraction` is the fraction of the total width that the first child should get.
    pub fn new_binary(dir: LinearDir, children: [TileId; 2], fraction: f32) -> Self {
        debug_assert!(0.0 <= fraction && fraction <= 1.0);
        let mut slf = Self {
            children: children.into(),
            dir,
            ..Default::default()
        };
        // We multiply the shares with 2.0 because the default share size is 1.0,
        // and so we want the total share to be the same as the number of children.
        slf.shares[children[0]] = 2.0 * (fraction);
        slf.shares[children[1]] = 2.0 * (1.0 - fraction);
        slf
    }

    pub fn add_child(&mut self, child: TileId) {
        self.children.push(child);
    }

    pub fn layout<Pane>(
        &mut self,
        tiles: &mut Tiles<Pane>,
        style: &egui::Style,
        behavior: &mut dyn Behavior<Pane>,
        rect: Rect,
    ) {
        // GC:
        let child_set: nohash_hasher::IntSet<TileId> = self.children.iter().copied().collect();
        self.shares.retain(|id| child_set.contains(&id));

        match self.dir {
            LinearDir::Horizontal => {
                self.layout_horizontal(tiles, style, behavior, rect);
            }
            LinearDir::Vertical => self.layout_vertical(tiles, style, behavior, rect),
        }
    }

    fn layout_horizontal<Pane>(
        &mut self,
        tiles: &mut Tiles<Pane>,
        style: &egui::Style,
        behavior: &mut dyn Behavior<Pane>,
        rect: Rect,
    ) {
        let num_gaps = self.children.len().saturating_sub(1);
        let gap_width = behavior.gap_width(style);
        let total_gap_width = gap_width * num_gaps as f32;
        let available_width = (rect.width() - total_gap_width).at_least(0.0);

        let widths = self.shares.split(&self.children, available_width);

        let mut x = rect.min.x;
        for (child, width) in self.children.iter().zip(widths) {
            let child_rect = Rect::from_min_size(pos2(x, rect.min.y), vec2(width, rect.height()));
            tiles.layout_tile(style, behavior, child_rect, *child);
            x += width + gap_width;
        }
    }

    fn layout_vertical<Pane>(
        &mut self,
        tiles: &mut Tiles<Pane>,
        style: &egui::Style,
        behavior: &mut dyn Behavior<Pane>,
        rect: Rect,
    ) {
        let num_gaps = self.children.len().saturating_sub(1);
        let gap_height = behavior.gap_width(style);
        let total_gap_height = gap_height * num_gaps as f32;
        let available_height = (rect.height() - total_gap_height).at_least(0.0);

        let heights = self.shares.split(&self.children, available_height);

        let mut y = rect.min.y;
        for (child, height) in self.children.iter().zip(heights) {
            let child_rect = Rect::from_min_size(pos2(rect.min.x, y), vec2(rect.width(), height));
            tiles.layout_tile(style, behavior, child_rect, *child);
            y += height + gap_height;
        }
    }

    pub(super) fn ui<Pane>(
        &mut self,
        tree: &mut Tree<Pane>,
        behavior: &mut dyn Behavior<Pane>,
        drop_context: &mut DropContext,
        ui: &mut egui::Ui,
        tile_id: TileId,
    ) {
        match self.dir {
            LinearDir::Horizontal => self.horizontal_ui(tree, behavior, drop_context, ui, tile_id),
            LinearDir::Vertical => self.vertical_ui(tree, behavior, drop_context, ui, tile_id),
        }
    }

    fn horizontal_ui<Pane>(
        &mut self,
        tree: &mut Tree<Pane>,
        behavior: &mut dyn Behavior<Pane>,
        drop_context: &mut DropContext,
        ui: &mut egui::Ui,
        parent_id: TileId,
    ) {
        for &child in &self.children {
            if !is_being_dragged(ui.ctx(), child) {
                tree.tile_ui(behavior, drop_context, ui, child);
            }
        }

        linear_drop_zones(ui.ctx(), tree, &self.children, self.dir, |rect, i| {
            drop_context.suggest_rect(
                InsertionPoint::new(parent_id, ContainerInsertion::Horizontal(i)),
                rect,
            );
        });

        // ------------------------
        // resizing:

        let parent_rect = tree.tiles.rect(parent_id);
        for (i, (left, right)) in self.children.iter().copied().tuple_windows().enumerate() {
            let resize_id = egui::Id::new((parent_id, "resize", i));

            let left_rect = tree.tiles.rect(left);
            let right_rect = tree.tiles.rect(right);
            let x = egui::lerp(left_rect.right()..=right_rect.left(), 0.5);

            let mut resize_state = ResizeState::Idle;
            if let Some(pointer) = ui.ctx().pointer_latest_pos() {
                let line_rect = Rect::from_center_size(
                    pos2(x, parent_rect.center().y),
                    vec2(
                        2.0 * ui.style().interaction.resize_grab_radius_side,
                        parent_rect.height(),
                    ),
                );
                let response = ui.interact(line_rect, resize_id, egui::Sense::click_and_drag());
                resize_state = resize_interaction(
                    behavior,
                    &mut self.shares,
                    &self.children,
                    &response,
                    [left, right],
                    ui.painter().round_to_pixel(pointer.x) - x,
                    i,
                    |tile_id: TileId| tree.tiles.rect(tile_id).width(),
                );

                if resize_state != ResizeState::Idle {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
            }

            let stroke = behavior.resize_stroke(ui.style(), resize_state);
            ui.painter().vline(x, parent_rect.y_range(), stroke);
        }
    }

    fn vertical_ui<Pane>(
        &mut self,
        tree: &mut Tree<Pane>,
        behavior: &mut dyn Behavior<Pane>,
        drop_context: &mut DropContext,
        ui: &mut egui::Ui,
        parent_id: TileId,
    ) {
        for &child in &self.children {
            if !is_being_dragged(ui.ctx(), child) {
                tree.tile_ui(behavior, drop_context, ui, child);
            }
        }

        linear_drop_zones(ui.ctx(), tree, &self.children, self.dir, |rect, i| {
            drop_context.suggest_rect(
                InsertionPoint::new(parent_id, ContainerInsertion::Vertical(i)),
                rect,
            );
        });

        // ------------------------
        // resizing:

        let parent_rect = tree.tiles.rect(parent_id);
        for (i, (top, bottom)) in self.children.iter().copied().tuple_windows().enumerate() {
            let resize_id = egui::Id::new((parent_id, "resize", i));

            let top_rect = tree.tiles.rect(top);
            let bottom_rect = tree.tiles.rect(bottom);
            let y = egui::lerp(top_rect.bottom()..=bottom_rect.top(), 0.5);

            let mut resize_state = ResizeState::Idle;
            if let Some(pointer) = ui.ctx().pointer_latest_pos() {
                let line_rect = Rect::from_center_size(
                    pos2(parent_rect.center().x, y),
                    vec2(
                        parent_rect.width(),
                        2.0 * ui.style().interaction.resize_grab_radius_side,
                    ),
                );
                let response = ui.interact(line_rect, resize_id, egui::Sense::click_and_drag());
                resize_state = resize_interaction(
                    behavior,
                    &mut self.shares,
                    &self.children,
                    &response,
                    [top, bottom],
                    ui.painter().round_to_pixel(pointer.y) - y,
                    i,
                    |tile_id: TileId| tree.tiles.rect(tile_id).height(),
                );

                if resize_state != ResizeState::Idle {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                }
            }

            let stroke = behavior.resize_stroke(ui.style(), resize_state);
            ui.painter().hline(parent_rect.x_range(), y, stroke);
        }
    }

    pub(super) fn simplify_children(&mut self, mut simplify: impl FnMut(TileId) -> SimplifyAction) {
        self.children.retain_mut(|child| match simplify(*child) {
            SimplifyAction::Remove => false,
            SimplifyAction::Keep => true,
            SimplifyAction::Replace(new) => {
                self.shares.replace_with(*child, new);
                *child = new;
                true
            }
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn resize_interaction<Pane>(
    behavior: &mut dyn Behavior<Pane>,
    shares: &mut Shares,
    children: &[TileId],
    splitter_response: &egui::Response,
    [left, right]: [TileId; 2],
    dx: f32,
    i: usize,
    tile_width: impl Fn(TileId) -> f32,
) -> ResizeState {
    if splitter_response.double_clicked() {
        // double-click to center the split between left and right:
        let mean = 0.5 * (shares[left] + shares[right]);
        shares[left] = mean;
        shares[right] = mean;
        ResizeState::Hovering
    } else if splitter_response.dragged() {
        if dx < 0.0 {
            // Expand right, shrink stuff to the left:
            shares[right] += shrink_shares(
                behavior,
                shares,
                &children[0..=i].iter().copied().rev().collect_vec(),
                dx.abs(),
                tile_width,
            );
        } else {
            // Expand the left, shrink stuff to the right:
            shares[left] +=
                shrink_shares(behavior, shares, &children[i + 1..], dx.abs(), tile_width);
        }
        ResizeState::Dragging
    } else if splitter_response.hovered() {
        ResizeState::Hovering
    } else {
        ResizeState::Idle
    }
}

/// Try shrink the children by a total of `target_in_points`,
/// making sure no child gets smaller than its minimum size.
fn shrink_shares<Pane>(
    behavior: &dyn Behavior<Pane>,
    shares: &mut Shares,
    children: &[TileId],
    target_in_points: f32,
    size_in_point: impl Fn(TileId) -> f32,
) -> f32 {
    if children.is_empty() {
        return 0.0;
    }

    let mut total_shares = 0.0;
    let mut total_points = 0.0;
    for &child in children {
        total_shares += shares[child];
        total_points += size_in_point(child);
    }

    let shares_per_point = total_shares / total_points;

    let min_size_in_shares = shares_per_point * behavior.min_size();

    let target_in_shares = shares_per_point * target_in_points;
    let mut total_shares_lost = 0.0;

    for &child in children {
        let share = &mut shares[child];
        let spare_share = (*share - min_size_in_shares).at_least(0.0);
        let shares_needed = (target_in_shares - total_shares_lost).at_least(0.0);
        let shrink_by = f32::min(spare_share, shares_needed);

        *share -= shrink_by;
        total_shares_lost += shrink_by;
    }

    total_shares_lost
}

fn linear_drop_zones<Pane>(
    egui_ctx: &egui::Context,
    tree: &Tree<Pane>,
    children: &[TileId],
    dir: LinearDir,
    add_drop_drect: impl FnMut(Rect, usize),
) {
    let preview_thickness = 12.0;
    let dragged_index = children
        .iter()
        .position(|&child| is_being_dragged(egui_ctx, child));

    let after_rect = |rect: Rect| match dir {
        LinearDir::Horizontal => Rect::from_min_max(
            rect.right_top() - vec2(preview_thickness, 0.0),
            rect.right_bottom(),
        ),
        LinearDir::Vertical => Rect::from_min_max(
            rect.left_bottom() - vec2(0.0, preview_thickness),
            rect.right_bottom(),
        ),
    };

    drop_zones(
        preview_thickness,
        children,
        dragged_index,
        dir,
        |tile_id| tree.tiles.rect(tile_id),
        add_drop_drect,
        after_rect,
    );
}

/// Register drop-zones for a linear container.
pub(super) fn drop_zones(
    preview_thickness: f32,
    children: &[TileId],
    dragged_index: Option<usize>,
    dir: LinearDir,
    get_rect: impl Fn(TileId) -> Rect,
    mut add_drop_drect: impl FnMut(Rect, usize),
    after_rect: impl Fn(Rect) -> Rect,
) {
    let before_rect = |rect: Rect| match dir {
        LinearDir::Horizontal => Rect::from_min_max(
            rect.left_top(),
            rect.left_bottom() + vec2(preview_thickness, 0.0),
        ),
        LinearDir::Vertical => Rect::from_min_max(
            rect.left_top(),
            rect.right_top() + vec2(0.0, preview_thickness),
        ),
    };
    let between_rects = |a: Rect, b: Rect| match dir {
        LinearDir::Horizontal => Rect::from_center_size(
            a.right_center().lerp(b.left_center(), 0.5),
            vec2(preview_thickness, a.height()),
        ),
        LinearDir::Vertical => Rect::from_center_size(
            a.center_bottom().lerp(b.center_top(), 0.5),
            vec2(a.width(), preview_thickness),
        ),
    };

    let mut prev_rect: Option<Rect> = None;
    let mut insertion_index = 0; // skips over drag-source, if any, because it will be removed before its re-inserted

    for (i, &child) in children.iter().enumerate() {
        let rect = get_rect(child);

        if Some(i) == dragged_index {
            // Suggest hole as a drop-target:
            add_drop_drect(rect, i);
        } else {
            if let Some(prev_rect) = prev_rect {
                if Some(i - 1) != dragged_index {
                    // Suggest dropping between the rects:
                    add_drop_drect(between_rects(prev_rect, rect), insertion_index);
                }
            } else {
                // Suggest dropping before the first child:
                add_drop_drect(before_rect(rect), 0);
            }

            insertion_index += 1;
        }

        prev_rect = Some(rect);
    }

    if let Some(last_rect) = prev_rect {
        // Suggest dropping after the last child (unless that's the one being dragged):
        if dragged_index != Some(children.len() - 1) {
            add_drop_drect(after_rect(last_rect), insertion_index + 1);
        }
    }
}

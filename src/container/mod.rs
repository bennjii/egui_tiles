use egui::Rect;

use crate::Tree;

use super::{Behavior, DropContext, SimplifyAction, TileId, Tiles};

mod grid;
mod linear;
mod tabs;

pub use grid::{Grid, GridLayout, GridLoc};
pub use linear::{Linear, LinearDir, Shares};
pub use tabs::Tabs;

// ----------------------------------------------------------------------------

/// The layout type of a [`Container`].
///
/// This is used to describe a [`Container`], and to change it to a different layout type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ContainerKind {
    /// Each child in an individual tab.
    #[default]
    Tabs,

    /// Left-to-right
    Horizontal,

    /// Top-down
    Vertical,

    /// In a grid, laied out row-wise, left-to-right, top-down.
    Grid,
}

impl ContainerKind {
    pub const ALL: [Self; 4] = [Self::Tabs, Self::Horizontal, Self::Vertical, Self::Grid];
}

// ----------------------------------------------------------------------------

/// A container of several [`super::Tile`]s.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Container {
    Tabs(Tabs),
    Linear(Linear),
    Grid(Grid),
}

impl From<Tabs> for Container {
    #[inline]
    fn from(tabs: Tabs) -> Self {
        Self::Tabs(tabs)
    }
}

impl From<Linear> for Container {
    #[inline]
    fn from(linear: Linear) -> Self {
        Self::Linear(linear)
    }
}

impl From<Grid> for Container {
    #[inline]
    fn from(grid: Grid) -> Self {
        Self::Grid(grid)
    }
}

impl Container {
    pub fn new(typ: ContainerKind, children: Vec<TileId>) -> Self {
        match typ {
            ContainerKind::Tabs => Self::new_tabs(children),
            ContainerKind::Horizontal => Self::new_horizontal(children),
            ContainerKind::Vertical => Self::new_vertical(children),
            ContainerKind::Grid => Self::new_grid(children),
        }
    }

    pub fn new_linear(dir: LinearDir, children: Vec<TileId>) -> Self {
        Self::Linear(Linear::new(dir, children))
    }

    pub fn new_horizontal(children: Vec<TileId>) -> Self {
        Self::new_linear(LinearDir::Horizontal, children)
    }

    pub fn new_vertical(children: Vec<TileId>) -> Self {
        Self::new_linear(LinearDir::Vertical, children)
    }

    pub fn new_tabs(children: Vec<TileId>) -> Self {
        Self::Tabs(Tabs::new(children))
    }

    pub fn new_grid(children: Vec<TileId>) -> Self {
        Self::Grid(Grid::new(children))
    }

    pub fn is_empty(&self) -> bool {
        self.children().is_empty()
    }

    pub fn children(&self) -> &[TileId] {
        match self {
            Self::Tabs(tabs) => &tabs.children,
            Self::Linear(linear) => &linear.children,
            Self::Grid(grid) => &grid.children,
        }
    }

    pub fn add_child(&mut self, child: TileId) {
        match self {
            Self::Tabs(tabs) => tabs.add_child(child),
            Self::Linear(linear) => linear.add_child(child),
            Self::Grid(grid) => grid.add_child(child),
        }
    }

    pub fn kind(&self) -> ContainerKind {
        match self {
            Self::Tabs(_) => ContainerKind::Tabs,
            Self::Linear(linear) => match linear.dir {
                LinearDir::Horizontal => ContainerKind::Horizontal,
                LinearDir::Vertical => ContainerKind::Vertical,
            },
            Self::Grid(_) => ContainerKind::Grid,
        }
    }

    pub fn set_kind(&mut self, kind: ContainerKind) {
        if kind == self.kind() {
            return;
        }

        *self = match kind {
            ContainerKind::Tabs => Self::Tabs(Tabs::new(self.children().to_vec())),
            ContainerKind::Horizontal => {
                Self::Linear(Linear::new(LinearDir::Horizontal, self.children().to_vec()))
            }
            ContainerKind::Vertical => {
                Self::Linear(Linear::new(LinearDir::Vertical, self.children().to_vec()))
            }
            ContainerKind::Grid => Self::Grid(Grid::new(self.children().to_vec())),
        };
    }

    pub(super) fn retain(&mut self, mut retain: impl FnMut(TileId) -> bool) {
        let retain = |tile_id: &TileId| retain(*tile_id);
        match self {
            Self::Tabs(tabs) => tabs.children.retain(retain),
            Self::Linear(linear) => linear.children.retain(retain),
            Self::Grid(grid) => grid.children.retain(retain),
        }
    }

    pub(super) fn simplify_children(&mut self, simplify: impl FnMut(TileId) -> SimplifyAction) {
        match self {
            Self::Tabs(tabs) => tabs.simplify_children(simplify),
            Self::Linear(linear) => linear.simplify_children(simplify),
            Self::Grid(grid) => grid.simplify_children(simplify),
        }
    }

    pub(super) fn layout<Pane>(
        &mut self,
        tiles: &mut Tiles<Pane>,
        style: &egui::Style,
        behavior: &mut dyn Behavior<Pane>,
        rect: Rect,
    ) {
        if self.is_empty() {
            return;
        }

        match self {
            Container::Tabs(tabs) => tabs.layout(tiles, style, behavior, rect),
            Container::Linear(linear) => {
                linear.layout(tiles, style, behavior, rect);
            }
            Container::Grid(grid) => grid.layout(tiles, style, behavior, rect),
        }
    }

    pub(super) fn ui<Pane>(
        &mut self,
        tree: &mut Tree<Pane>,
        behavior: &mut dyn Behavior<Pane>,
        drop_context: &mut DropContext,
        ui: &mut egui::Ui,
        rect: Rect,
        tile_id: TileId,
    ) {
        match self {
            Container::Tabs(tabs) => {
                tabs.ui(tree, behavior, drop_context, ui, rect, tile_id);
            }
            Container::Linear(linear) => {
                linear.ui(tree, behavior, drop_context, ui, tile_id);
            }
            Container::Grid(grid) => {
                grid.ui(tree, behavior, drop_context, ui, tile_id);
            }
        }
    }
}

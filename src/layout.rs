///! Basic CSS block layout.

use css::Display;
use style::StyledNode;
use paint::{DisplayList, DisplayCommand, Rect};
use std::default::Default;

pub use self::BoxType::{AnonymousBlock, InlineNode, BlockNode};

// CSS box model. All sizes are in px.

#[derive(Clone, Copy, Default, Debug)]
pub struct Dimensions {
    /// Position of the content area relative to the document origin:
    pub content: Rect,
    // Surrounding edges:
    pub padding: EdgeSizes,
    pub border: EdgeSizes,
    pub margin: EdgeSizes,
}

#[derive(Clone, Copy, Default, Debug)]
pub struct EdgeSizes {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

/// A node in the layout tree.
pub struct LayoutBox<'a> {
    pub dimensions: Dimensions,
    pub box_type: BoxType<'a>,
    pub children: Vec<LayoutBox<'a>>,
}

pub enum BoxType<'a> {
    BlockNode(&'a StyledNode<'a>),
    InlineNode(&'a StyledNode<'a>),
    AnonymousBlock, // FIXME: This should not be a separate type of box!
}

impl<'a> LayoutBox<'a> {
    fn new(box_type: BoxType) -> LayoutBox {
        LayoutBox {
            box_type: box_type,
            dimensions: Default::default(),
            children: Vec::new(),
        }
    }

    fn get_style_node(&self) -> &'a StyledNode<'a> {
        match self.box_type {
            BlockNode(node) | InlineNode(node) => node,
            AnonymousBlock => panic!("Anonymous block box has no style node")
        }
    }
}

/// Transform a style tree into a layout tree.
pub fn layout_tree<'a>(node: &'a StyledNode<'a>, mut containing_block: Dimensions) -> LayoutBox<'a> {
    // The layout algorithm expects the container height to start at 0.
    // TODO: Save the initial containing block height, for calculating percent heights.
    containing_block.content.height = 0.0;

    let mut root_box = build_layout_tree(node);
    root_box.layout(containing_block);
    root_box
}

/// Build the tree of LayoutBoxes, but don't perform any layout calculations yet.
fn build_layout_tree<'a>(style_node: &'a StyledNode<'a>) -> LayoutBox<'a> {
    // Create the root box.
    let mut root = LayoutBox::new(match style_node.specified.display {
        Display::Block => BlockNode(style_node),
        Display::Inline => InlineNode(style_node),
        Display::None => panic!("Root node has display: none.")
    });

    // Create the descendant boxes.
    for child in &style_node.children {
        match child.specified.display {
            Display::Block => root.children.push(build_layout_tree(child)),
            Display::Inline => root.get_inline_container().children.push(build_layout_tree(child)),
            Display::None => {} // Don't lay out nodes with `display: none;`
        }
    }
    root
}

/// Fold the layout tree into a display list to render.
pub fn display_list<'a>(layout_root: &LayoutBox<'a>) -> DisplayList {
    let mut list = Vec::new();
    layout_root.render(&mut list);
    list
}

impl<'a> LayoutBox<'a> {
    /// Lay out a box and its descendants.
    fn layout(&mut self, containing_block: Dimensions) {
        match self.box_type {
            BlockNode(_) => self.layout_block(containing_block),
            InlineNode(_) | AnonymousBlock => {} // TODO
        }
    }

    /// Lay out a block-level element and its descendants.
    fn layout_block(&mut self, containing_block: Dimensions) {
        // Child width can depend on parent width, so we need to calculate this box's width before
        // laying out its children.
        self.calculate_block_width(containing_block);

        // Determine where the box is located within its container.
        self.calculate_block_position(containing_block);

        // Recursively lay out the children of this box.
        self.layout_block_children();

        // Parent height can depend on child height, so `calculate_height` must be called after the
        // children are laid out.
        self.calculate_block_height();
    }

    /// Calculate the width of a block-level non-replaced element in normal flow.
    ///
    /// http://www.w3.org/TR/CSS2/visudet.html#blockwidth
    ///
    /// Sets the horizontal margin/padding/border dimensions, and the `width`.
    fn calculate_block_width(&mut self, containing_block: Dimensions) {
        let style = self.get_style_node();

        let mut width = style.specified.width;

        let mut margin_left = style.specified.margin_left;
        let mut margin_right = style.specified.margin_right;

        let border_left = style.specified.border_left;
        let border_right = style.specified.border_right;

        let padding_left = style.specified.padding_left;
        let padding_right = style.specified.padding_right;

        let total: f32 = [
            margin_left.unwrap_or_default(), margin_right.unwrap_or_default(),
            border_left, border_right,
            padding_left, padding_right,
            width.unwrap_or_default(),
        ].iter().sum();

        // If width is not auto and the total is wider than the container, treat auto margins as 0.
        if width.is_some() && total > containing_block.content.width {
            if margin_left.is_none() {
                margin_left = Some(0.0);
            }
            if margin_right.is_none() {
                margin_right = Some(0.0);
            }
        }

        // Adjust used values so that the above sum equals `containing_block.width`.
        // Each arm of the `match` should increase the total width by exactly `underflow`,
        // and afterward all values should be absolute lengths in px.
        let underflow = containing_block.content.width - total;

        match (width.is_none(), margin_left.is_none(), margin_right.is_none()) {
            // If the values are overconstrained, calculate margin_right.
            (false, false, false) => {
                margin_right = Some(margin_right.unwrap_or_default() + underflow);
            }

            // If exactly one size is auto, its used value follows from the equality.
            (false, false, true) => { margin_right = Some(underflow); }
            (false, true, false) => { margin_left  = Some(underflow); }

            // If width is set to auto, any other auto values become 0.
            (true, _, _) => {
                if margin_left.is_none() { margin_left = Some(0.0); }
                if margin_right.is_none() { margin_right = Some(0.0); }

                if underflow >= 0.0 {
                    // Expand width to fill the underflow.
                    width = Some(underflow);
                } else {
                    // Width can't be negative. Adjust the right margin instead.
                    width = Some(0.0);
                    margin_right = Some(margin_right.unwrap_or_default() + underflow);
                }
            }

            // If margin-left and margin-right are both auto, their used values are equal.
            (false, true, true) => {
                margin_left = Some(underflow / 2.0);
                margin_right = Some(underflow / 2.0);
            }
        }

        let d = &mut self.dimensions;
        d.content.width = width.unwrap_or_default();

        d.padding.left = padding_left;
        d.padding.right = padding_right;

        d.border.left = border_left;
        d.border.right = border_right;

        d.margin.left = margin_left.unwrap_or_default();
        d.margin.right = margin_right.unwrap_or_default();
    }

    /// Finish calculating the block's edge sizes, and position it within its containing block.
    ///
    /// http://www.w3.org/TR/CSS2/visudet.html#normal-block
    ///
    /// Sets the vertical margin/padding/border dimensions, and the `x`, `y` values.
    fn calculate_block_position(&mut self, containing_block: Dimensions) {
        let style = self.get_style_node();
        let d = &mut self.dimensions;

        // If margin-top or margin-bottom is `auto`, the used value is zero.
        d.margin.top = style.specified.margin_top.unwrap_or_default();
        d.margin.bottom = style.specified.margin_bottom.unwrap_or_default();

        d.border.top = style.specified.border_top;
        d.border.bottom = style.specified.border_bottom;

        d.padding.top = style.specified.padding_top;
        d.padding.bottom = style.specified.padding_bottom;

        d.content.x = containing_block.content.x +
                      d.margin.left + d.border.left + d.padding.left;

        // Position the box below all the previous boxes in the container.
        d.content.y = containing_block.content.height + containing_block.content.y +
                      d.margin.top + d.border.top + d.padding.top;
    }

    /// Lay out the block's children within its content area.
    ///
    /// Sets `self.dimensions.height` to the total content height.
    fn layout_block_children(&mut self) {
        let d = &mut self.dimensions;
        for child in &mut self.children {
            child.layout(*d);
            // Increment the height so each child is laid out below the previous one.
            d.content.height = d.content.height + child.dimensions.margin_box().height;
        }
    }

    /// Height of a block-level non-replaced element in normal flow with overflow visible.
    fn calculate_block_height(&mut self) {
        // If the height is set to an explicit length, use that exact length.
        // Otherwise, just keep the value set by `layout_block_children`.
        if let Some(h) = self.get_style_node().specified.height {
            self.dimensions.content.height = h;
        }
    }

    /// Where a new inline child should go.
    fn get_inline_container(&mut self) -> &mut LayoutBox<'a> {
        match self.box_type {
            InlineNode(_) | AnonymousBlock => self,
            BlockNode(_) => {
                // If we've just generated an anonymous block box, keep using it.
                // Otherwise, create a new one.
                match self.children.last() {
                    Some(&LayoutBox { box_type: AnonymousBlock,..}) => {}
                    _ => self.children.push(LayoutBox::new(AnonymousBlock))
                }
                self.children.last_mut().unwrap()
            }
        }
    }

    fn render(&self, list: &mut DisplayList) {
        self.render_background(list);
        self.render_borders(list);
        for child in &self.children {
            child.render(list);
        }
    }

    fn render_background(&self, list: &mut DisplayList) {
        match self.box_type {
            BlockNode(style) | InlineNode(style) => {
                let color = style.specified.background_color;
                list.push(DisplayCommand::SolidColor(color, self.dimensions.border_box()));
            },
            AnonymousBlock => {},
        }
    }

    fn render_borders(&self, list: &mut DisplayList) {
        let color = match self.box_type {
            BlockNode(style) | InlineNode(style) => style.specified.border_color,
            AnonymousBlock => return,
        };

        let d = &self.dimensions;
        let border_box = d.border_box();

        // Left border
        list.push(DisplayCommand::SolidColor(color, Rect {
            x: border_box.x,
            y: border_box.y,
            width: d.border.left,
            height: border_box.height,
        }));

        // Right border
        list.push(DisplayCommand::SolidColor(color, Rect {
            x: border_box.x + border_box.width - d.border.right,
            y: border_box.y,
            width: d.border.right,
            height: border_box.height,
        }));

        // Top border
        list.push(DisplayCommand::SolidColor(color, Rect {
            x: border_box.x,
            y: border_box.y,
            width: border_box.width,
            height: d.border.top,
        }));

        // Bottom border
        list.push(DisplayCommand::SolidColor(color, Rect {
            x: border_box.x,
            y: border_box.y + border_box.height - d.border.bottom,
            width: border_box.width,
            height: d.border.bottom,
        }));
    }
}

impl Rect {
    pub fn expanded_by(self, edge: EdgeSizes) -> Rect {
        Rect {
            x: self.x - edge.left,
            y: self.y - edge.top,
            width: self.width + edge.left + edge.right,
            height: self.height + edge.top + edge.bottom,
        }
    }
}

impl Dimensions {
    /// The area covered by the content area plus its padding.
    pub fn padding_box(self) -> Rect {
        self.content.expanded_by(self.padding)
    }
    /// The area covered by the content area plus padding and borders.
    pub fn border_box(self) -> Rect {
        self.padding_box().expanded_by(self.border)
    }
    /// The area covered by the content area plus padding, borders, and margin.
    pub fn margin_box(self) -> Rect {
        self.border_box().expanded_by(self.margin)
    }
}

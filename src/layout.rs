///! Basic CSS block layout.

use style::{StyledNode, Style, Display, Edge, Pixels};
use paint::{DisplayList, DisplayCommand};
use std::default::Default;

// CSS box model. All sizes are in px.

#[derive(Clone, Copy, Default, Debug)]
struct Rect {
    x: Pixels,
    y: Pixels,
    width: Pixels,
    height: Pixels,
}

impl Rect {
    pub fn expanded_by(self, edge: Edge<Pixels>) -> Rect {
        Rect {
            x: self.x - edge.left,
            y: self.y - edge.top,
            width: self.width + edge.left + edge.right,
            height: self.height + edge.top + edge.bottom,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BoxType {
    Block, // display: block
    Inline, // display: inline
    //Absolute, // position: absolute && display: block
    //Fixed, // position: fixed && display: block
    //Float, // display: block && float: left|right
    //Text, // literal text
}

/// A node in the layout tree.
pub struct LayoutBox<'a> {
    /// Position and size of the container box (from the containing block).
    container: Rect,
    /// Position and size ignoring any adjustments due to style constraints.
    intrinsic: Rect,
    /// Position and size of the content box relative to the document origin.
    content_box: Rect,
    /// Position and size of the padding box relative to the document origin.
    padding_box: Rect,
    /// Position and size of the border box relative to the document origin.
    border_box: Rect,
    /// Position and size of the margin box relative to the document origin.
    margin_box: Rect,
    /// Edges of the padding box.
    padding: Edge<Pixels>,
    /// Edges of the border box.
    border: Edge<Pixels>,
    /// Edges of the margin box.
    margin: Edge<Pixels>,
    /// Excess (or missing) horizontal space.
    underflow: Pixels,
    /// Specified values from styling.
    style: &'a Style,
    /// Whether this box is anonymous.
    anonymous: bool,
    /// Fundamental layout mode (e.g., block, inline, float, absolute, &c.).
    box_type: BoxType,
    /// Zero or more descendant (child) boxes.
    children: Vec<LayoutBox<'a>>,
}

impl<'a> LayoutBox<'a> {
    fn new(box_type: BoxType, style: &'a Style) -> Self {
        LayoutBox {
            container: Rect::default(),
            intrinsic: Rect::default(),
            content_box: Rect::default(),
            padding_box: Rect::default(),
            border_box: Rect::default(),
            margin_box: Rect::default(),
            padding: Edge::default(),
            border: Edge::default(),
            margin: Edge::default(),
            underflow: 0.0,
            style: style,
            anonymous: true,
            box_type: box_type,
            children: Vec::new(),
        }
    }
}

/// Transform a style tree into a layout tree.
#[allow(unused_variables)]
pub fn layout_tree<'a>(node: &'a StyledNode<'a>, width: usize, height: usize) -> LayoutBox<'a> {
    let mut root_box = build_layout_tree(node).expect("Root style node has `display: none`");
    root_box.container.width = width as Pixels;
    //root_box.container.height = height as Pixels; // this "height" is really box's top edge
    root_box.layout();
    root_box
}

/// Build the tree of LayoutBoxes, but don't perform any layout calculations yet.
fn build_layout_tree<'a>(style_node: &'a StyledNode<'a>) -> Option<LayoutBox<'a>> {
    // Create the root box.
    let box_type = match style_node.specified.display {
        Display::Block => Some(BoxType::Block),
        Display::Inline => Some(BoxType::Inline),
        Display::None => None,
    }?;
    let style = &style_node.specified;
    let mut root = LayoutBox::new(box_type, style);
    root.anonymous = false;

    // Create the descendant boxes.
    let mut wrapper = None;
    for child in style_node.children.iter().filter_map(build_layout_tree) {
        // TODO: The child sequence is really supposed to be restricted to the supremum of all
        // real child box types, taking Text < Inline < Block.
        // The hacky check below effectively just follows the original toy layout algorithm.
        if box_type != child.box_type {
            let mut anon = wrapper.get_or_insert_with(|| LayoutBox::new(box_type, style));
            anon.children.push(child);
        } else {
            if let Some(anon) = wrapper.take() {
                root.children.push(anon);
            }
            root.children.push(child);
        }
    }
    Some(root)
}

/// Fold the layout tree into a display list to render.
pub fn display_list<'a>(layout_root: &LayoutBox<'a>) -> DisplayList {
    let mut list = Vec::new();
    layout_root.render(&mut list);
    list
}

impl<'a> LayoutBox<'a> {
    /// Lay out a box and its descendants.
    fn layout(&mut self) {
        match self.box_type {
            BoxType::Block => self.layout_block(),
            BoxType::Inline => {},
        }
    }

    /// Lay out a block-level element and its descendants.
    fn layout_block(&mut self) {
        // Child width can depend on parent width, so we need to calculate this box's width before
        // laying out its children.
        self.calculate_block_width();

        // Finish calculating the block's edge sizes, and position it within its containing block.
        self.margin.top = self.style.margin.top.value(); // auto ==> 0
        self.margin.bottom = self.style.margin.bottom.value(); // auto ==> 0

        self.border.top = self.style.border.top;
        self.border.bottom = self.style.border.bottom;

        self.padding.top = self.style.padding.top;
        self.padding.bottom = self.style.padding.bottom;

        // Position the box flush left (w.r.t. margin/border/padding) to the container.
        self.intrinsic.x = self.container.x +
                           self.margin.left + self.border.left + self.padding.left;
        self.content_box.x = self.intrinsic.x;

        // Position the box below all the previous boxes in the container.
        self.intrinsic.y = self.container.y + self.container.height +
                           self.margin.top + self.border.top + self.padding.top;
        self.content_box.y = self.intrinsic.y;

        // Recursively lay out the children of this box.
        self.intrinsic.height = 0.0; // fold accumulator
        for child in &mut self.children {
            // Give the child box the boundaries of its container.
            child.container.x = self.intrinsic.x;
            child.container.y = self.intrinsic.y;
            child.container.height = self.intrinsic.height;
            child.container.width = self.content_box.width;
            // Lay out the child box.
            child.layout();
            // Increment the height so each child is laid out below the previous one.
            self.intrinsic.height += child.margin_box.height;
        }

        // Parent height can depend on child height, so `calculate_height` must be called after the
        // children are laid out.
        self.content_box.height = if self.style.height.is_auto() {
            self.intrinsic.height
        } else {
            self.style.height.value()
        };

        self.padding_box = self.content_box.expanded_by(self.padding);
        self.border_box = self.padding_box.expanded_by(self.border);
        self.margin_box = self.border_box.expanded_by(self.margin);
    }

    /// Calculate the width of a block-level non-replaced element in normal flow.
    ///
    /// http://www.w3.org/TR/CSS2/visudet.html#blockwidth
    ///
    /// Sets the horizontal margin/padding/border dimensions, and the `width`.
    fn calculate_block_width(&mut self) {
        self.intrinsic.width = [
            self.style.margin.left.value(), self.style.margin.right.value(),
            self.style.border.left, self.style.border.right,
            self.style.padding.left, self.style.padding.right,
            self.style.width.value(),
        ].iter().sum();

        // Adjust used values so that the above sum equals `containing_block.width`.
        // Each arm of the `match` should increase the total width by exactly `underflow`,
        // and afterward all values should be absolute lengths in px.
        self.underflow = self.container.width - self.intrinsic.width;

        self.padding.left = self.style.padding.left;
        self.padding.right = self.style.padding.right;

        self.border.left = self.style.border.left;
        self.border.right = self.style.border.right;

        self.content_box.width = if self.style.width.is_auto() {
            self.underflow.max(0.0)
        } else {
            self.style.width.value()
        };

        self.margin.left = if self.style.margin.left.is_auto() {
            if self.style.width.is_auto() || self.underflow < 0.0 {
                0.0
            } else if self.style.margin.right.is_auto() {
                self.underflow / 2.0
            } else {
                self.underflow
            }
        } else {
            self.style.margin.left.value()
        };

        self.margin.right = if self.style.width.is_auto() && self.underflow < 0.0 {
            self.style.margin.right.value() + self.underflow
        } else if self.style.margin.right.is_auto() {
            if self.style.width.is_auto() {
                0.0
            } else if self.style.margin.left.is_auto() {
                self.underflow / 2.0
            } else {
                self.underflow
            }
        } else if !self.style.margin.left.is_auto() || !self.style.width.is_auto() {
            self.style.margin.right.value() + self.underflow
        } else {
            self.style.margin.right.value()
        };
    }
}

impl<'a> LayoutBox<'a> {
    fn render(&self, list: &mut DisplayList) {
        self.render_background(list);
        self.render_borders(list);
        for child in &self.children {
            child.render(list);
        }
    }

    fn render_background(&self, list: &mut DisplayList) {
        list.push(DisplayCommand::SolidColor {
            color: self.style.background_color,
            x: self.border_box.x,
            y: self.border_box.y,
            width: self.border_box.width,
            height: self.border_box.height,
        });
    }

    fn render_borders(&self, list: &mut DisplayList) {
        // Left border
        list.push(DisplayCommand::SolidColor {
            color: self.style.border_color,
            x: self.border_box.x,
            y: self.border_box.y,
            width: self.border.left,
            height: self.border_box.height,
        });

        // Right border
        list.push(DisplayCommand::SolidColor {
            color: self.style.border_color,
            x: self.border_box.x + self.border_box.width - self.border.right,
            y: self.border_box.y,
            width: self.border.right,
            height: self.border_box.height,
        });

        // Top border
        list.push(DisplayCommand::SolidColor{
            color: self.style.border_color,
            x: self.border_box.x,
            y: self.border_box.y,
            width: self.border_box.width,
            height: self.border.top,
        });

        // Bottom border
        list.push(DisplayCommand::SolidColor {
            color: self.style.border_color,
            x: self.border_box.x,
            y: self.border_box.y + self.border_box.height - self.border.bottom,
            width: self.border_box.width,
            height: self.border.bottom,
        });
    }
}

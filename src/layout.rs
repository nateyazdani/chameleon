///! Basic CSS block layout.

use style::{StyledNode, Style, Display, Edge, Automatic, Pixels};
use paint::{DisplayList, DisplayCommand};
use std::default::Default;

// CSS box model. All sizes are in px.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BoxType {
    Block, // display: block
    Inline, // display: inline
}

/// A node in the layout tree.
pub struct LayoutBox<'a> {
    /// Position and size of the content box relative to the document origin.
    content: Rect,
    /// Position and size of the container box (from the containing block).
    container: Rect,
    /// Edges of the padding box.
    padding: Edge<Pixels>,
    /// Edges of the border box.
    border: Edge<Pixels>,
    /// Edges of the margin box.
    margin: Edge<Pixels>,
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
            content: Rect::default(),
            container: Rect::default(),
            padding: Edge::default(),
            border: Edge::default(),
            margin: Edge::default(),
            style: style,
            anonymous: true,
            box_type: box_type,
            children: Vec::new(),
        }
    }

    fn of_style_node(style_node: &'a StyledNode<'a>) -> Self {
        LayoutBox {
            content: Rect::default(),
            container: Rect::default(),
            padding: Edge::default(),
            border: Edge::default(),
            margin: Edge::default(),
            style: &style_node.specified,
            anonymous: false,
            box_type: match style_node.specified.display {
                Display::Block => BoxType::Block,
                Display::Inline => BoxType::Inline,
                Display::None => panic!("of_style_node: root has display of \"none\"."),
            },
            children: Vec::new(),
        }
    }

    fn is_anonymous_block(&self) -> bool {
        self.box_type == BoxType::Block && self.anonymous
    }

    #[allow(dead_code)]
    fn is_anonymous_inline(&self) -> bool {
        self.box_type == BoxType::Inline && self.anonymous
    }

    /// The area covered by the content area plus its padding.
    fn padding_box(&self) -> Rect {
        self.content.expanded_by(self.padding)
    }

    /// The area covered by the content area plus padding and borders.
    fn border_box(&self) -> Rect {
        self.padding_box().expanded_by(self.border)
    }

    /// The area covered by the content area plus padding, borders, and margin.
    fn margin_box(&self) -> Rect {
        self.border_box().expanded_by(self.margin)
    }
}

/// Transform a style tree into a layout tree.
#[allow(unused_variables)]
pub fn layout_tree<'a>(node: &'a StyledNode<'a>, width: usize, height: usize) -> LayoutBox<'a> {
    // The layout algorithm expects the container height to start at 0.
    // TODO: Save the initial containing block height, for calculating percent heights.
    let mut root_box = build_layout_tree(node);
    root_box.container.width = width as Pixels;
    //root_box.container.height = height as Pixels;
    root_box.layout();
    root_box
}

/// Build the tree of LayoutBoxes, but don't perform any layout calculations yet.
fn build_layout_tree<'a>(style_node: &'a StyledNode<'a>) -> LayoutBox<'a> {
    // Create the root box.
    let mut root = LayoutBox::of_style_node(style_node);

    // Create the descendant boxes.
    for child in &style_node.children {
        match child.specified.display {
            Display::Block => root.children.push(build_layout_tree(child)),
            Display::Inline => root.get_inline_container().children.push(build_layout_tree(child)),
            Display::None => {}, // Skip any child with `display: none;`
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
    fn layout(&mut self) {
        match self.box_type {
            BoxType::Block => self.layout_block(),
            BoxType::Inline => {} // TODO
        }
    }

    /// Lay out a block-level element and its descendants.
    fn layout_block(&mut self) {
        // Child width can depend on parent width, so we need to calculate this box's width before
        // laying out its children.
        self.calculate_block_width();

        // Determine where the box is located within its container.
        self.calculate_block_position();

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
    fn calculate_block_width(&mut self) {
        let mut width = self.style.width;

        let mut margin_left = self.style.margin.left;
        let mut margin_right = self.style.margin.right;

        let total: Pixels = [
            margin_left.value(), margin_right.value(),
            self.style.border.left, self.style.border.right,
            self.style.padding.left, self.style.padding.right,
            width.value(),
        ].iter().sum();

        // If width is not auto and the total is wider than the container, treat auto margins as 0.
        if width.is_given() && total > self.container.width {
            if margin_left.is_auto() {
                margin_left = Automatic::Given(0.0);
            }
            if margin_right.is_auto() {
                margin_right = Automatic::Given(0.0);
            }
        }

        // Adjust used values so that the above sum equals `containing_block.width`.
        // Each arm of the `match` should increase the total width by exactly `underflow`,
        // and afterward all values should be absolute lengths in px.
        let underflow = self.container.width - total;

        match (width.is_auto(), margin_left.is_auto(), margin_right.is_auto()) {
            // If the values are overconstrained, calculate margin_right.
            (false, false, false) => {
                margin_right = Automatic::Given(margin_right.value() + underflow);
            }

            // If exactly one size is auto, its used value follows from the equality.
            (false, false, true) => { margin_right = Automatic::Given(underflow); }
            (false, true, false) => { margin_left  = Automatic::Given(underflow); }

            // If width is set to auto, any other auto values become 0.
            (true, _, _) => {
                if margin_left.is_auto() { margin_left = Automatic::Given(0.0); }
                if margin_right.is_auto() { margin_right = Automatic::Given(0.0); }

                // Expand width to fill the underflow.
                width = Automatic::Given(underflow.max(0.0));
                if underflow < 0.0 {
                    // Adjust the right margin to compensate for negative underflow.
                    margin_right = Automatic::Given(margin_right.value() + underflow);
                }
            }

            // If margin-left and margin-right are both auto, their used values are equal.
            (false, true, true) => {
                margin_left = Automatic::Given(underflow / 2.0);
                margin_right = Automatic::Given(underflow / 2.0);
            }
        }

        self.content.width = width.value();

        self.padding.left = self.style.padding.left;
        self.padding.right = self.style.padding.right;

        self.border.left = self.style.border.left;
        self.border.right = self.style.border.right;

        self.margin.left = margin_left.value();
        self.margin.right = margin_right.value();
    }

    /// Finish calculating the block's edge sizes, and position it within its containing block.
    ///
    /// http://www.w3.org/TR/CSS2/visudet.html#normal-block
    ///
    /// Sets the vertical margin/padding/border dimensions, and the `x`, `y` values.
    fn calculate_block_position(&mut self) {
        // If margin-top or margin-bottom is `auto`, the used value is zero.
        self.margin.top = self.style.margin.top.value();
        self.margin.bottom = self.style.margin.bottom.value();

        self.border.top = self.style.border.top;
        self.border.bottom = self.style.border.bottom;

        self.padding.top = self.style.padding.top;
        self.padding.bottom = self.style.padding.bottom;

        self.content.x = self.container.x +
                         self.margin.left + self.border.left + self.padding.left;

        // Position the box below all the previous boxes in the container.
        self.content.y = self.container.y + self.container.height +
                         self.margin.top + self.border.top + self.padding.top;
    }

    /// Lay out the block's children within its content area.
    ///
    /// Sets `self.dimensions.height` to the total content height.
    fn layout_block_children(&mut self) {
        for child in &mut self.children {
            child.container = self.content;
            child.layout();
            // Increment the height so each child is laid out below the previous one.
            self.content.height = self.content.height + child.margin_box().height;
        }
    }

    /// Height of a block-level non-replaced element in normal flow with overflow visible.
    fn calculate_block_height(&mut self) {
        // If the height is set to an explicit length, use that exact length.
        // Otherwise, just keep the value set by `layout_block_children`.
        if let Automatic::Given(h) = self.style.height {
            self.content.height = h;
        }
    }

    /// Where a new inline child should go.
    fn get_inline_container(&mut self) -> &mut LayoutBox<'a> {
        match self.box_type {
            BoxType::Inline => self,
            BoxType::Block => {
                // If we've just generated an anonymous block box, keep using it.
                // Otherwise, create a new one.
                if !self.children.last().map(LayoutBox::is_anonymous_block).unwrap_or_default() {
                    self.children.push(LayoutBox::new(BoxType::Block, &self.style))
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
        let color = self.style.background_color;

        let border_box = self.border_box();
        list.push(DisplayCommand::SolidColor {
            color: color,
            x: border_box.x,
            y: border_box.y,
            width: border_box.width,
            height: border_box.height,
        });
    }

    fn render_borders(&self, list: &mut DisplayList) {
        let color = self.style.border_color;

        let border_box = self.border_box();

        // Left border
        list.push(DisplayCommand::SolidColor {
            color: color,
            x: border_box.x,
            y: border_box.y,
            width: self.border.left,
            height: border_box.height,
        });

        // Right border
        list.push(DisplayCommand::SolidColor {
            color: color,
            x: border_box.x + border_box.width - self.border.right,
            y: border_box.y,
            width: self.border.right,
            height: border_box.height,
        });

        // Top border
        list.push(DisplayCommand::SolidColor{
            color: color,
            x: border_box.x,
            y: border_box.y,
            width: border_box.width,
            height: self.border.top,
        });

        // Bottom border
        list.push(DisplayCommand::SolidColor {
            color: color,
            x: border_box.x,
            y: border_box.y + border_box.height - self.border.bottom,
            width: border_box.width,
            height: self.border.bottom,
        });
    }
}

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

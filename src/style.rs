//! Code for applying CSS styles to the DOM.
//!
//! This is not very interesting at the moment.  It will get much more
//! complicated if I add support for compound selectors.

use dom::{Node, NodeType, ElementData};
use css::{Stylesheet, Rule, Selector, SimpleSelector, Color, Display, Specificity};
use std::convert::TryInto;

/// A node with associated style data.
pub struct StyledNode<'a> {
    pub node: &'a Node,
    pub specified: Style,
    pub children: Vec<StyledNode<'a>>,
}

/// Computed style values
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    // layout mode
    pub display: Display,

    // box colors
    pub background_color: Color,
    pub border_color: Color,

    // content dimensions (None ~ auto)
    pub width: Option<f32>,
    pub height: Option<f32>,

    // margin lengths in pixels (None ~ auto)
    pub margin_left: Option<f32>,
    pub margin_right: Option<f32>,
    pub margin_top: Option<f32>,
    pub margin_bottom: Option<f32>,

    // padding lengths in pixels
    pub padding_left: f32,
    pub padding_right: f32,
    pub padding_top: f32,
    pub padding_bottom: f32,

    // border lengths in pixels
    pub border_left: f32,
    pub border_right: f32,
    pub border_top: f32,
    pub border_bottom: f32,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            display: Display::default(),

            background_color: Color::default(),
            border_color: Color::default(),

            width: Option::None,
            height: Option::None,

            margin_left: Option::Some(0.0),
            margin_right: Option::Some(0.0),
            margin_top: Option::Some(0.0),
            margin_bottom: Option::Some(0.0),

            padding_left: 0.0,
            padding_right: 0.0,
            padding_top: 0.0,
            padding_bottom: 0.0,

            border_left: 0.0,
            border_right: 0.0,
            border_top: 0.0,
            border_bottom: 0.0,
        }
    }
}

/// Apply a stylesheet to an entire DOM tree, returning a StyledNode tree.
///
/// This finds only the specified values at the moment. Eventually it should be extended to find the
/// computed values too, including inherited values.
pub fn style_tree<'a>(root: &'a Node, stylesheet: &'a Stylesheet) -> StyledNode<'a> {
    StyledNode {
        node: root,
        specified: match root.node_type {
            NodeType::Element(ref elem) => specified_values(elem, stylesheet),
            NodeType::Text(_) => Style::default(),
        },
        children: root.children.iter().map(|child| style_tree(child, stylesheet)).collect(),
    }
}

/// Apply styles to a single element, returning the specified styles.
///
/// To do: Allow multiple UA/author/user stylesheets, and implement the cascade.
fn specified_values(elem: &ElementData, stylesheet: &Stylesheet) -> Style {
    let mut style = Style::default();
    let mut rules = matching_rules(elem, stylesheet);

    // Go through the rules from lowest to highest specificity.
    rules.sort_by(|&(a, _), &(b, _)| a.cmp(&b));
    for (_, rule) in rules {
        for declaration in &rule.declarations {
            let property = declaration.name.as_str();
            let value = &declaration.value;
            match property {
                "display" => { style.display = value.try_into().expect(property); },

                "width" => { style.width = value.try_into().expect(property); },
                "height" => { style.height = value.try_into().expect(property); },

                "background-color" => { style.background_color = value.try_into().expect(property); },
                "border-color" => { style.border_color = value.try_into().expect(property); },

                "margin-left" => { style.margin_left = value.try_into().expect(property); },
                "margin-right" => { style.margin_right = value.try_into().expect(property); },
                "margin-top" => { style.margin_top = value.try_into().expect(property); },
                "margin-bottom" => { style.margin_bottom = value.try_into().expect(property); },
                "margin" => {
                    let specified = value.try_into().expect(property);
                    style.margin_left = specified;
                    style.margin_right = specified;
                    style.margin_top = specified;
                    style.margin_bottom = specified;
                },

                "padding-left" => { style.padding_left = value.try_into().expect(property); },
                "padding-right" => { style.padding_right = value.try_into().expect(property); },
                "padding-top" => { style.padding_top = value.try_into().expect(property); },
                "padding-bottom" => { style.padding_bottom = value.try_into().expect(property); },
                "padding" => {
                    let specified = value.try_into().expect(property);
                    style.padding_left = specified;
                    style.padding_right = specified;
                    style.padding_top = specified;
                    style.padding_bottom = specified;
                },

                "border-left-width" => { style.border_left = value.try_into().expect(property); },
                "border-right-width" => { style.border_right = value.try_into().expect(property); },
                "border-top-width" => { style.border_top = value.try_into().expect(property); },
                "border-bottom-width" => { style.border_bottom = value.try_into().expect(property); },
                "border-width" => {
                    let specified = value.try_into().expect(property);
                    style.border_left = specified;
                    style.border_right = specified;
                    style.border_top = specified;
                    style.border_bottom = specified;
                },

                _ => { /* XXX: Ignore any unsupported styling property! */ }
            }
        }
    }
    style
}

/// A single CSS rule and the specificity of its most specific matching selector.
type MatchedRule<'a> = (Specificity, &'a Rule);

/// Find all CSS rules that match the given element.
fn matching_rules<'a>(elem: &ElementData, stylesheet: &'a Stylesheet) -> Vec<MatchedRule<'a>> {
    // For now, we just do a linear scan of all the rules.  For large
    // documents, it would be more efficient to store the rules in hash tables
    // based on tag name, id, class, etc.
    stylesheet.rules.iter().filter_map(|rule| match_rule(elem, rule)).collect()
}

/// If `rule` matches `elem`, return a `MatchedRule`. Otherwise return `None`.
fn match_rule<'a>(elem: &ElementData, rule: &'a Rule) -> Option<MatchedRule<'a>> {
    // Find the first (most specific) matching selector.
    rule.selectors.iter().find(|selector| matches(elem, *selector))
        .map(|selector| (selector.specificity(), rule))
}

/// Selector matching:
fn matches(elem: &ElementData, selector: &Selector) -> bool {
    match *selector {
        Selector::Simple(ref simple_selector) => matches_simple_selector(elem, simple_selector)
    }
}

fn matches_simple_selector(elem: &ElementData, selector: &SimpleSelector) -> bool {
    // Check type selector
    if selector.tag.iter().any(|name| elem.tag != *name) {
        return false
    }

    // Check ID selector
    if selector.id.iter().any(|id| elem.id() != Some(id)) {
        return false;
    }

    // Check class selectors
    let elem_classes = elem.classes();
    if selector.class.iter().any(|class| !elem_classes.contains(&**class)) {
        return false;
    }

    // We didn't find any non-matching selector components.
    true
}

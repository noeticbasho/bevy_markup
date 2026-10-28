//! Selector matching on lightningcss's parsed selectors with its selector
//! engine, parcel_selectors (ADR 0034). lightningcss parses each rule's
//! selectors; [`Selector::from_parsed`] keeps the ones the cascade
//! supports (the allowlist) and records what they read; matching runs
//! parcel's `matches_selector` over [`Node`], an `Element` implementation
//! on the build walk's own structures: the element, its ancestor chain
//! (nearest first) and the shared sibling lists.

use std::collections::BTreeMap;
use std::fmt;

use lightningcss::selector::{Combinator as CssCombinator, Component, PseudoClass};
use parcel_selectors::attr::{AttrSelectorOperation, CaseSensitivity, NamespaceConstraint};
use parcel_selectors::context::{MatchingContext, MatchingMode, QuirksMode};
use parcel_selectors::matching::{
    CompoundSelectorMatchingResult, ElementSelectorFlags, matches_compound_selector_from,
    matches_selector,
};
use parcel_selectors::parser::{NthType, SelectorImpl};
use parcel_selectors::{Element, OpaqueElement};

use super::{Ancestor, ElementState, Position, Pseudo, SelectorReads, SiblingList, Siblings};
use crate::html::HtmlElement;

/// A parsed selector as lightningcss hands it over.
type Parsed = lightningcss::selector::Selector<'static>;

/// lightningcss's `SelectorImpl` lives in a private module; its public
/// `Selector` alias names it.
pub trait ImplOf {
    type Impl;
}

impl<'i, I: SelectorImpl<'i>> ImplOf for parcel_selectors::parser::Selector<'i, I> {
    type Impl = I;
}

type Impl = <Parsed as ImplOf>::Impl;

/// A selector the cascade supports, with what it reads.
#[derive(Clone, Debug)]
pub(super) struct Selector {
    parsed: Parsed,
    specificity: (u32, u32, u32),
    /// An interaction pseudo-class anywhere (ADR 0029's pseudo rules).
    pseudo: bool,
    reads: SelectorReads,
    /// `X ~ subject` with an `X` that reads only the sibling itself: the
    /// match-order offset of `X`. Its sibling test is answered from a scan
    /// cached on the sibling list ([`SiblingList::any_before`]): parcel
    /// walks every preceding sibling per element, O(n²) over a list that
    /// has no match (review_0005 I03's test, failing in CI).
    later_sibling: Option<usize>,
    /// Identifies the selector in sibling-list caches.
    id: u64,
}

/// Selector ids for [`Selector::id`].
static NEXT_SELECTOR_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl Selector {
    /// `None` for a selector outside the supported grammar: its rule is
    /// skipped (logged by the caller). The allowlist is also what keeps
    /// parts parcel's matcher panics on (`NthOf`, `Has`, `Nesting`,
    /// `:nth-col()`) away from it.
    pub(super) fn from_parsed(parsed: &Parsed) -> Option<Self> {
        let mut reads = SelectorReads::default();
        let mut pseudo = false;
        let mut subject = true;
        let mut sibling_seen_away = false;
        // The `X ~ subject` shape: where `X` starts, whether a combinator
        // follows it, whether it reads context through `:is()` & co.
        let mut later_sibling = None;
        let mut beyond_x = false;
        let mut x_reads_context = false;
        for (index, component) in parsed.iter_raw_match_order().enumerate() {
            if let Component::Combinator(combinator) = component {
                if subject && *combinator == CssCombinator::LaterSibling {
                    later_sibling = Some(index + 1);
                } else if !subject {
                    beyond_x = true;
                }
                match combinator {
                    CssCombinator::Descendant | CssCombinator::Child => {}
                    CssCombinator::NextSibling | CssCombinator::LaterSibling => {
                        // ADR 0028's limit, kept for parity: a sibling
                        // combinator only next to the subject.
                        if !subject {
                            sibling_seen_away = true;
                        }
                        if *combinator == CssCombinator::NextSibling {
                            reads.adjacent_siblings = true;
                        } else {
                            reads.general_siblings = true;
                        }
                    }
                    _ => return None,
                }
                subject = false;
                continue;
            }
            let part = compound_part(component, false)?;
            reads.dataset |= part.attributes;
            if part.structural {
                if subject {
                    reads.subject_position = true;
                } else {
                    reads.context_position = true;
                }
            }
            reads.context_position |= part.context_structural;
            x_reads_context |= !subject && part.context_structural;
            pseudo |= part.pseudo;
        }
        let later_sibling = later_sibling.filter(|_| !beyond_x && !x_reads_context);
        if sibling_seen_away {
            return None;
        }
        let packed = parsed.specificity();
        Some(Self {
            parsed: parsed.clone(),
            specificity: (packed >> 20, (packed >> 10) & 0x3ff, packed & 0x3ff),
            pseudo,
            reads,
            later_sibling,
            id: NEXT_SELECTOR_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        })
    }

    /// Parses `text` with lightningcss (tests).
    #[cfg(test)]
    pub(super) fn parse(text: &str) -> Option<Self> {
        use lightningcss::rules::CssRule;
        use lightningcss::stylesheet::{ParserOptions, StyleSheet};
        use lightningcss::traits::IntoOwned;
        let css = format!("{text} {{}}");
        let sheet = StyleSheet::parse(&css, ParserOptions::default())
            .ok()?
            .into_owned();
        let CssRule::Style(rule) = sheet.rules.0.first()? else {
            return None;
        };
        match rule.selectors.0.as_slice() {
            [only] => Self::from_parsed(only),
            _ => None,
        }
    }

    pub(super) fn specificity(&self) -> (u32, u32, u32) {
        self.specificity
    }

    pub(super) fn has_pseudo(&self) -> bool {
        self.pseudo
    }

    pub(super) fn reads(&self) -> SelectorReads {
        self.reads
    }

    /// `element` (with its `pseudo` and structural state) and its
    /// `ancestors` (nearest first) against the selector.
    pub(super) fn matches(
        &self,
        element: &HtmlElement,
        pseudo: Pseudo,
        ancestors: &[Ancestor],
        state: &ElementState,
    ) -> bool {
        self.run(element, pseudo, ancestors, state, false)
    }

    /// Whether the selector could match if every interaction test went
    /// either way (ADR 0029's reactivity record). An interaction
    /// pseudo-class answers whatever helps the match: true outside a
    /// `:not()`, false inside one. The allowlist admits no `:not()` inside
    /// another (through `:is()` neither), so that is exact.
    pub(super) fn matches_ignoring_pseudo(
        &self,
        element: &HtmlElement,
        ancestors: &[Ancestor],
        state: &ElementState,
    ) -> bool {
        self.run(element, Pseudo::default(), ancestors, state, true)
    }

    fn run(
        &self,
        element: &HtmlElement,
        pseudo: Pseudo,
        ancestors: &[Ancestor],
        state: &ElementState,
        wild: bool,
    ) -> bool {
        let scope = Scope {
            element,
            pseudo,
            ancestors,
            state,
            wild,
        };
        let node = Node {
            scope: &scope,
            kind: Kind::Subject,
        };
        let mut context =
            MatchingContext::new(MatchingMode::Normal, None, None, QuirksMode::NoQuirks);
        if let (Some(offset), Some(siblings)) = (self.later_sibling, &state.previous_siblings) {
            // The subject's own compound, then "some preceding sibling
            // matches `X`" from the list's cached scan. Offsets: this one
            // counts in parse order (the subject's compound starts right
            // after the `~`, which is match-order index `offset - 1`);
            // `matches_selector`'s counts in match order.
            let subject_start = self.parsed.len() - (offset - 1);
            // The subject's compound is the rightmost, so a match reads
            // `FullyMatched` (it ends the selector), not `Matched`.
            if let CompoundSelectorMatchingResult::NotMatched =
                matches_compound_selector_from(&self.parsed, subject_start, &mut context, &node)
            {
                return false;
            }
            let list = &siblings.list;
            return list.any_before(siblings.count, (self.id, wild), |index| {
                let sibling = node.with(Kind::Listed {
                    list,
                    index,
                    level: 0,
                });
                let mut context =
                    MatchingContext::new(MatchingMode::Normal, None, None, QuirksMode::NoQuirks);
                matches_selector(
                    &self.parsed,
                    offset,
                    None,
                    &sibling,
                    &mut context,
                    &mut |_, _| {},
                )
            });
        }
        matches_selector(&self.parsed, 0, None, &node, &mut context, &mut |_, _| {})
    }
}

/// What one compound part reads.
#[derive(Clone, Copy, Default)]
struct PartReads {
    attributes: bool,
    /// A position (or `:focus-within`) of the element the part tests.
    structural: bool,
    /// An interaction pseudo-class (ADR 0029's pseudo rules).
    pseudo: bool,
    /// A position of an ancestor: a part after a combinator inside a
    /// `:is()`/`:where()`/`:not()` argument.
    context_structural: bool,
}

impl PartReads {
    fn or(self, other: Self) -> Self {
        Self {
            attributes: self.attributes | other.attributes,
            structural: self.structural | other.structural,
            pseudo: self.pseudo | other.pseudo,
            context_structural: self.context_structural | other.context_structural,
        }
    }
}

/// One compound part against the allowlist: `None` when unsupported.
/// The allowlist is also what keeps parts parcel's matcher panics on
/// (`NthOf`, `Has`, `Nesting`, `:nth-col()`) away from it. `negated`:
/// inside a `:not()`, which may not hold another (ADR 0029's reactivity
/// record answers by negation parity, and parcel tracks one level).
fn compound_part(component: &Component<'static>, negated: bool) -> Option<PartReads> {
    let reads = |attributes, structural, pseudo| PartReads {
        attributes,
        structural,
        pseudo,
        context_structural: false,
    };
    Some(match component {
        Component::LocalName(_)
        | Component::ExplicitUniversalType
        | Component::ID(_)
        | Component::Class(_) => PartReads::default(),
        // Any operator and case flag on a `data-*` attribute (the element
        // model carries only those; other attributes skip the rule).
        Component::AttributeInNoNamespaceExists { local_name, .. }
        | Component::AttributeInNoNamespace { local_name, .. } => {
            data_attribute(local_name.as_ref())?;
            reads(true, false, false)
        }
        Component::NonTSPseudoClass(class) => match class {
            PseudoClass::Hover
            | PseudoClass::Active
            | PseudoClass::Focus
            | PseudoClass::FocusVisible => reads(false, false, true),
            // A position-like state (`Position::focus_within`), not
            // wildcarded by the reactivity record.
            PseudoClass::FocusWithin => reads(false, true, false),
            _ => return None,
        },
        Component::Nth(data) => match data.ty {
            NthType::Child
            | NthType::LastChild
            | NthType::OnlyChild
            | NthType::OfType
            | NthType::LastOfType
            | NthType::OnlyOfType => reads(false, true, false),
            // `:nth-col()`/`:nth-last-col()`: parcel's matcher has no arm.
            _ => return None,
        },
        Component::Empty => reads(false, true, false),
        Component::Negation(list) if !negated => nested(list, true)?,
        Component::Is(list) | Component::Where(list) => nested(list, negated)?,
        _ => return None,
    })
}

/// The selectors of a `:not()`/`:is()`/`:where()` argument: descendant and
/// child combinators only (a sibling combinator would need invalidation
/// the cascade doesn't have, ADR 0034).
fn nested(list: &[Parsed], negated: bool) -> Option<PartReads> {
    let mut reads = PartReads::default();
    for selector in list {
        // The argument's own subject compound tests the same element as
        // the compound holding it; parts past a combinator, its ancestors.
        let mut subject = true;
        for part in selector.iter_raw_match_order() {
            match part {
                Component::Combinator(CssCombinator::Descendant | CssCombinator::Child) => {
                    subject = false;
                }
                Component::Combinator(_) => return None,
                part => {
                    let mut part = compound_part(part, negated)?;
                    if !subject {
                        part.context_structural |= part.structural;
                        part.structural = false;
                    }
                    reads = reads.or(part);
                }
            }
        }
    }
    Some(reads)
}

/// The dataset key of a `data-*` attribute name (the element model
/// carries only those).
fn data_attribute(name: &str) -> Option<&str> {
    name.strip_prefix("data-").filter(|key| !key.is_empty())
}

/// What one match runs against.
struct Scope<'a> {
    element: &'a HtmlElement,
    pseudo: Pseudo,
    ancestors: &'a [Ancestor],
    state: &'a ElementState,
    wild: bool,
}

/// An element as parcel's matcher walks it.
#[derive(Clone, Copy)]
pub(super) struct Node<'a> {
    scope: &'a Scope<'a>,
    kind: Kind<'a>,
}

/// Where a [`Node`] sits. `level`: the node's parent is
/// `ancestors[level]` (none past the chain).
#[derive(Clone, Copy, Debug)]
enum Kind<'a> {
    /// The element being styled.
    Subject,
    /// `ancestors[i]`.
    Chain(usize),
    /// An entry of a shared sibling list.
    Listed {
        list: &'a SiblingList,
        index: usize,
        level: usize,
    },
    /// A stand-in sibling where no list is threaded (tests, image runs):
    /// the counts come from the element's [`Position`], so positional
    /// matches agree with it. `same`: of the element's type. `rest` /
    /// `rest_same`: how many more stand-ins follow in this direction.
    Phantom {
        level: usize,
        tag: &'a str,
        same: bool,
        backward: bool,
        rest: usize,
        rest_same: usize,
    },
}

/// The facts a compound reads, for any node kind.
struct View<'a> {
    tag: &'a str,
    id: Option<&'a str>,
    classes: &'a [String],
    pseudo: Pseudo,
    dataset: Option<&'a BTreeMap<String, String>>,
    position: Position,
}

/// A stand-in's tag when it isn't of the element's type: matches no type
/// selector and no real tag.
const OTHER_TYPE: &str = "\0";

impl<'a> Node<'a> {
    fn with(&self, kind: Kind<'a>) -> Self {
        Self {
            scope: self.scope,
            kind,
        }
    }

    fn view(&self) -> View<'a> {
        let scope = self.scope;
        match self.kind {
            Kind::Subject => View {
                tag: &scope.element.tag,
                id: scope.element.id.as_deref(),
                classes: &scope.element.classes,
                pseudo: scope.pseudo,
                dataset: Some(&scope.element.dataset),
                position: scope.state.position(),
            },
            Kind::Chain(index) => ancestor_view(&scope.ancestors[index]),
            Kind::Listed { list, index, .. } => ancestor_view(&list.list[index]),
            Kind::Phantom { tag, same, .. } => View {
                tag: if same { tag } else { OTHER_TYPE },
                id: None,
                classes: &[],
                pseudo: Pseudo::default(),
                dataset: None,
                position: Position::default(),
            },
        }
    }

    /// For the subject and chain nodes: the threaded sibling view (the
    /// list and the node's index in it), its position, its level.
    fn placement(&self) -> Option<(Option<&'a Siblings>, Position, usize)> {
        let scope = self.scope;
        match self.kind {
            Kind::Subject => Some((
                scope.state.previous_siblings.as_ref(),
                scope.state.position(),
                0,
            )),
            Kind::Chain(index) => {
                let ancestor = &scope.ancestors[index];
                Some((ancestor.link.0.as_ref(), ancestor.position, index + 1))
            }
            _ => None,
        }
    }

    fn phantom(&self, level: usize, backward: bool, total: usize, same: usize) -> Option<Self> {
        if total == 0 {
            return None;
        }
        let is_same = same > 0;
        Some(self.with(Kind::Phantom {
            level,
            tag: self.view().tag,
            same: is_same,
            backward,
            rest: total - 1,
            rest_same: same - usize::from(is_same),
        }))
    }

    fn sibling(&self, backward: bool) -> Option<Self> {
        if let Some((siblings, position, level)) = self.placement() {
            return match siblings {
                Some(view) if backward => {
                    let index = view.count.checked_sub(1)?;
                    Some(self.with(Kind::Listed {
                        list: &view.list,
                        index,
                        level,
                    }))
                }
                // The list holds the node itself: the next entry follows it.
                Some(view) if view.list.list.len() > view.count => {
                    let index = view.count + 1;
                    (index < view.list.list.len()).then(|| {
                        self.with(Kind::Listed {
                            list: &view.list,
                            index,
                            level,
                        })
                    })
                }
                _ if backward => self.phantom(
                    level,
                    true,
                    position.tag_index.saturating_sub(1),
                    position.type_index.saturating_sub(1),
                ),
                _ => self.phantom(
                    level,
                    false,
                    position.tag_count.saturating_sub(position.tag_index),
                    position.type_count.saturating_sub(position.type_index),
                ),
            };
        }
        match self.kind {
            Kind::Listed { list, index, level } => {
                let index = if backward {
                    index.checked_sub(1)?
                } else {
                    index + 1
                };
                (index < list.list.len()).then(|| self.with(Kind::Listed { list, index, level }))
            }
            Kind::Phantom {
                level,
                tag,
                backward: direction,
                rest,
                rest_same,
                ..
            } if direction == backward => {
                if rest == 0 {
                    return None;
                }
                let same = rest_same > 0;
                Some(self.with(Kind::Phantom {
                    level,
                    tag,
                    same,
                    backward,
                    rest: rest - 1,
                    rest_same: rest_same - usize::from(same),
                }))
            }
            _ => None,
        }
    }

    fn level(&self) -> usize {
        match self.kind {
            Kind::Subject => 0,
            Kind::Chain(index) => index + 1,
            Kind::Listed { level, .. } | Kind::Phantom { level, .. } => level,
        }
    }
}

fn ancestor_view(ancestor: &Ancestor) -> View<'_> {
    View {
        tag: &ancestor.tag,
        id: ancestor.id.as_deref(),
        classes: &ancestor.classes,
        pseudo: ancestor.pseudo,
        dataset: ancestor.dataset.as_deref(),
        position: ancestor.position,
    }
}

impl fmt::Debug for Node<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Node({:?})", self.kind)
    }
}

impl<'a> Element<'static> for Node<'a> {
    type Impl = Impl;

    fn opaque(&self) -> OpaqueElement {
        match self.kind {
            Kind::Subject => OpaqueElement::new(self.scope.element),
            Kind::Chain(index) => OpaqueElement::new(&self.scope.ancestors[index]),
            Kind::Listed { list, index, .. } => OpaqueElement::new(&list.list[index]),
            Kind::Phantom { .. } => OpaqueElement::new(self.scope),
        }
    }

    fn parent_element(&self) -> Option<Self> {
        let level = self.level();
        (level < self.scope.ancestors.len()).then(|| self.with(Kind::Chain(level)))
    }

    fn parent_node_is_shadow_root(&self) -> bool {
        false
    }

    fn containing_shadow_host(&self) -> Option<Self> {
        None
    }

    fn is_pseudo_element(&self) -> bool {
        false
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        self.sibling(true)
    }

    fn next_sibling_element(&self) -> Option<Self> {
        self.sibling(false)
    }

    fn is_html_element_in_html_document(&self) -> bool {
        true
    }

    fn has_local_name(
        &self,
        local_name: &<Impl as SelectorImpl<'static>>::BorrowedLocalName,
    ) -> bool {
        self.view().tag == local_name.as_ref()
    }

    fn has_namespace(&self, _ns: &<Impl as SelectorImpl<'static>>::BorrowedNamespaceUrl) -> bool {
        true
    }

    fn is_same_type(&self, other: &Self) -> bool {
        self.view().tag == other.view().tag
    }

    fn attr_matches(
        &self,
        _ns: &NamespaceConstraint<&<Impl as SelectorImpl<'static>>::NamespaceUrl>,
        local_name: &<Impl as SelectorImpl<'static>>::LocalName,
        operation: &AttrSelectorOperation<&<Impl as SelectorImpl<'static>>::AttrValue>,
    ) -> bool {
        let Some(key) = data_attribute(local_name.as_ref()) else {
            return false;
        };
        let Some(value) = self.view().dataset.and_then(|dataset| dataset.get(key)) else {
            return false;
        };
        match operation {
            AttrSelectorOperation::Exists => true,
            AttrSelectorOperation::WithValue {
                operator,
                case_sensitivity,
                expected_value,
            } => operator.eval_str(value, expected_value.as_ref(), *case_sensitivity),
        }
    }

    fn match_non_ts_pseudo_class<F>(
        &self,
        class: &<Impl as SelectorImpl<'static>>::NonTSPseudoClass,
        context: &mut MatchingContext<'_, 'static, Impl>,
        _flags_setter: &mut F,
    ) -> bool
    where
        F: FnMut(&Self, ElementSelectorFlags),
    {
        let view = self.view();
        let state = |actual: bool| {
            if self.scope.wild {
                !context.in_negation()
            } else {
                actual
            }
        };
        match class {
            PseudoClass::Hover => state(view.pseudo.hover),
            PseudoClass::Active => state(view.pseudo.active),
            PseudoClass::Focus => state(view.pseudo.focus),
            PseudoClass::FocusVisible => state(view.pseudo.focus_visible),
            PseudoClass::FocusWithin => view.position.focus_within,
            _ => false,
        }
    }

    fn match_pseudo_element(
        &self,
        _pe: &<Impl as SelectorImpl<'static>>::PseudoElement,
        _context: &mut MatchingContext<'_, 'static, Impl>,
    ) -> bool {
        false
    }

    fn is_link(&self) -> bool {
        false
    }

    fn is_html_slot_element(&self) -> bool {
        false
    }

    fn has_id(
        &self,
        id: &<Impl as SelectorImpl<'static>>::Identifier,
        _case: CaseSensitivity,
    ) -> bool {
        self.view().id == Some(id.as_ref())
    }

    fn has_class(
        &self,
        name: &<Impl as SelectorImpl<'static>>::Identifier,
        _case: CaseSensitivity,
    ) -> bool {
        let name: &str = name.as_ref();
        self.view().classes.iter().any(|class| class == name)
    }

    fn imported_part(
        &self,
        _name: &<Impl as SelectorImpl<'static>>::Identifier,
    ) -> Option<<Impl as SelectorImpl<'static>>::Identifier> {
        None
    }

    fn is_part(&self, _name: &<Impl as SelectorImpl<'static>>::Identifier) -> bool {
        false
    }

    fn is_empty(&self) -> bool {
        self.view().position.empty
    }

    fn is_root(&self) -> bool {
        false
    }
}

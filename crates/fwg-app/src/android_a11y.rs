//! Screen reader (TalkBack) support on Android.
//!
//! Slint 1.18's Android backend has no accessibility support: TalkBack only sees two empty views.
//! This mirrors what Slint's winit backend does on the desktop (`i-slint-backend-winit`'s
//! `accesskit.rs`): it walks Slint's item tree, builds an AccessKit tree from the accessible
//! items, and hands it to `accesskit_android`, which answers Android's accessibility requests for
//! Slint's full-screen input view through an injected `AccessibilityDelegate`.
//!
//! Simplifications compared with the desktop version, fine for this app:
//! - Every update is a full tree (AccessKit diffs it and only raises events for what changed).
//! - Changes are found with one property tracker over the whole walk, plus a once-a-second
//!   rebuild for what a tracker can't see (keyboard focus, items created or destroyed).
//! - No popup windows (the app has none) and no per-character text navigation; text fields
//!   expose their value.
//!
//! One addition: Slint gives `Flickable` no accessibility node, so a screen reader can't scroll.
//! Here every Flickable becomes a scroll view with page up/down actions, which is what TalkBack
//! uses to bring the next item on screen when you swipe past the last visible one.
//!
//! Everything here runs on the Slint thread except the two handlers, which `accesskit_android`
//! calls on the Android UI thread; they only post to the Slint event loop.

use std::cell::RefCell;
use std::collections::HashMap;
use std::pin::Pin;
use std::ptr::NonNull;
use std::time::Duration;

use accesskit::{
    Action, ActionHandler, ActionRequest, ActivationHandler, Live, Node, NodeId, Orientation, Role,
    Toggled, TreeId, TreeInfo, TreeUpdate,
};
use accesskit_android::InjectingAdapter;
use i_slint_core::accessibility::{
    AccessibilityAction, AccessibleStringProperty, SupportedAccessibilityAction,
    nearest_accessible_item,
};
use i_slint_core::input::FocusReason;
use i_slint_core::item_tree::{ItemTreeRc, ItemTreeRef, ItemTreeWeak};
use i_slint_core::items::{AccessibleRole, Flickable, ItemRc, WindowItem};
use i_slint_core::lengths::LogicalLength;
use i_slint_core::lengths::ScaleFactor;
use i_slint_core::properties::{PropertyDirtyHandler, PropertyTracker};
use i_slint_core::window::WindowInner;
use slint::ComponentHandle;

use crate::{AppWindow, platform};

thread_local! {
    static STATE: RefCell<Option<A11y>> = const { RefCell::new(None) };
}

/// Called once the window exists. Waits for Slint's input view to be added to the activity (it
/// is added asynchronously on the UI thread), then attaches the adapter to it.
pub fn install(ui: &AppWindow) {
    attach_later(ui.as_weak(), 0);
}

fn attach_later(ui: slint::Weak<AppWindow>, attempts: u32) {
    slint::Timer::single_shot(Duration::from_millis(250), move || {
        if !try_attach(&ui) && attempts < 40 {
            attach_later(ui, attempts + 1);
        }
    });
}

fn try_attach(ui: &slint::Weak<AppWindow>) -> bool {
    let attached = platform::with_activity(|env, activity| {
        let host = env
            .call_method(activity, "accessibilityHost", "()Landroid/view/View;", &[])?
            .l()?;
        if host.is_null() {
            return Ok(None);
        }
        Ok(Some(InjectingAdapter::new(env, &host, Activation, Actions)))
    });
    match attached {
        Ok(Some(adapter)) => {
            let tracker = Box::pin(PropertyTracker::new_with_dirty_handler(Dirty));
            let state = A11y {
                adapter,
                ui: ui.clone(),
                nodes: Nodes::default(),
                tracker,
                active: false,
                enabled: true,
                scheduled: false,
                backstop: slint::Timer::default(),
            };
            STATE.with(|s| *s.borrow_mut() = Some(state));
            true
        }
        Ok(None) => false,
        Err(e) => {
            platform::log(&format!("accessibility: {e}"));
            true // don't retry a JNI failure forty times
        }
    }
}

struct A11y {
    adapter: InjectingAdapter,
    ui: slint::Weak<AppWindow>,
    nodes: Nodes,
    tracker: Pin<Box<PropertyTracker<true, Dirty>>>,
    /// A screen reader asked for the tree; until then nothing is built.
    active: bool,
    /// Whether a screen reader is still running. Android doesn't tell an AccessKit adapter when
    /// TalkBack is turned off, so the backstop timer asks AccessibilityManager each second and
    /// updates pause while it is off.
    enabled: bool,
    scheduled: bool,
    backstop: slint::Timer,
}

/// Runs `f` on the Slint thread's state, if the adapter is attached.
fn with_state(f: impl FnOnce(&mut A11y)) {
    STATE.with(|s| {
        if let Ok(mut s) = s.try_borrow_mut() {
            if let Some(state) = s.as_mut() {
                f(state);
            }
        }
    });
}

/// Rebuilds the tree soon, at most every 100 ms however often properties change (scrolling
/// changes positions every frame).
fn schedule_update() {
    with_state(|state| {
        if !state.active || !state.enabled || state.scheduled {
            return;
        }
        state.scheduled = true;
        slint::Timer::single_shot(Duration::from_millis(100), || {
            with_state(|state| {
                state.scheduled = false;
                state.push_tree();
            })
        });
    });
}

impl A11y {
    fn activate(&mut self) {
        if !self.active {
            self.active = true;
            self.backstop
                .start(slint::TimerMode::Repeated, Duration::from_secs(1), || {
                    let enabled = platform::with_activity(|env, activity| {
                        env.call_method(activity, "screenReaderOn", "()Z", &[])?.z()
                    })
                    .unwrap_or(true);
                    with_state(|state| state.enabled = enabled);
                    schedule_update();
                });
        }
        // Answers the initial request: accesskit_android expects a full tree soon after.
        self.push_tree();
    }

    fn push_tree(&mut self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let window = ui.window();
        let nodes = &mut self.nodes;
        let tracker = self.tracker.as_ref();
        self.adapter
            .update_if_active(|| tracker.evaluate_as_dependency_root(|| nodes.build(window)));
    }

    fn perform(&mut self, request: ActionRequest) {
        let Some(item) = self.nodes.item_for(request.target_node) else {
            return;
        };
        let action = match request.action {
            Action::Click => AccessibilityAction::Default,
            Action::Focus => {
                if let Some(ui) = self.ui.upgrade() {
                    WindowInner::from_pub(ui.window()).set_focus_item(
                        &item,
                        true,
                        FocusReason::PointerClick,
                    );
                }
                return;
            }
            Action::Increment => AccessibilityAction::Increment,
            Action::Decrement => AccessibilityAction::Decrement,
            Action::Expand => AccessibilityAction::Expand,
            Action::ScrollDown | Action::ScrollUp => {
                scroll_page(&item, request.action == Action::ScrollDown);
                return;
            }
            Action::SetValue => match request.data {
                Some(accesskit::ActionData::Value(v)) => {
                    AccessibilityAction::SetValue(v.as_ref().into())
                }
                Some(accesskit::ActionData::NumericValue(v)) => {
                    AccessibilityAction::SetValue(v.to_string().into())
                }
                _ => return,
            },
            Action::ReplaceSelectedText => match request.data {
                Some(accesskit::ActionData::Value(v)) => {
                    AccessibilityAction::ReplaceSelectedText(v.as_ref().into())
                }
                _ => return,
            },
            _ => return,
        };
        item.accessible_action(&action);
    }
}

// ---------------------------------------------------------------------- handlers (UI thread)

struct Activation;

impl ActivationHandler for Activation {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        // Slint's items live on the Slint thread; build there and send it asynchronously.
        let _ = slint::invoke_from_event_loop(|| with_state(A11y::activate));
        None
    }
}

struct Actions;

impl ActionHandler for Actions {
    fn do_action(&mut self, request: ActionRequest) {
        let _ = slint::invoke_from_event_loop(move || {
            with_state(|s| s.perform(request));
            schedule_update();
        });
    }
}

struct Dirty;

impl PropertyDirtyHandler for Dirty {
    fn notify(self: Pin<&Self>) {
        // Called while a property is being changed; react from the event loop.
        slint::Timer::single_shot(Duration::ZERO, schedule_update);
    }
}

// ---------------------------------------------------------------------- the tree

// A node id is the item's index in its item tree (16 bits) and an id for that item tree, the
// same scheme as Slint's desktop backends.
const INDEX_BITS: u32 = 16;
const INDEX_MASK: u64 = (1 << INDEX_BITS) - 1;

#[derive(Default)]
struct Nodes {
    next_component_id: u32,
    components_by_id: HashMap<u32, ItemTreeWeak>,
    component_ids: HashMap<NonNull<u8>, u32>,
}

impl Nodes {
    fn item_for(&self, id: NodeId) -> Option<ItemRc> {
        let component = self
            .components_by_id
            .get(&((id.0 >> INDEX_BITS) as u32))?
            .upgrade()?;
        Some(ItemRc::new(component, (id.0 & INDEX_MASK) as u32))
    }

    fn node_id(&mut self, item: &ItemRc) -> NodeId {
        let component = item.item_tree();
        let ptr = ItemTreeRef::as_ptr(ItemTreeRc::borrow(component));
        // A destroyed component's address can be reused by a new one: only trust a mapping
        // whose component is still this one.
        let known = self.component_ids.get(&ptr).copied().filter(|id| {
            self.components_by_id
                .get(id)
                .and_then(|w| w.upgrade())
                .is_some_and(|c| ItemTreeRc::ptr_eq(&c, component))
        });
        let component_id = known.unwrap_or_else(|| {
            self.next_component_id += 1;
            let id = self.next_component_id;
            self.component_ids.insert(ptr, id);
            self.components_by_id
                .insert(id, ItemTreeRc::downgrade(component));
            id
        });
        NodeId((component_id as u64) << INDEX_BITS | (item.index() as u64 & INDEX_MASK))
    }

    fn build(&mut self, window: &slint::Window) -> TreeUpdate {
        // Forget item trees that no longer exist.
        self.components_by_id.retain(|_, w| w.upgrade().is_some());
        let live: std::collections::HashSet<u32> = self.components_by_id.keys().copied().collect();
        self.component_ids.retain(|_, id| live.contains(id));

        let inner = WindowInner::from_pub(window);
        inner.ensure_tree_instantiated();
        let Some(component) = inner.try_component() else {
            return TreeUpdate {
                nodes: vec![(NodeId(0), Node::new(Role::Window))],
                tree: Some(TreeInfo::new(NodeId(0))),
                tree_id: TreeId::ROOT,
                focus: NodeId(0),
            };
        };
        let scale = ScaleFactor::new(window.scale_factor());
        let mut nodes = Vec::new();
        let root = self.add(ItemRc::new_root(component.clone()), scale, &mut nodes);

        let focus = inner
            .focus_item
            .borrow()
            .upgrade()
            .map(|item| self.node_id(&nearest_accessible_item(item)))
            .filter(|id| nodes.iter().any(|(n, _)| n == id))
            .unwrap_or(root);

        let mut tree = TreeInfo::new(root);
        tree.toolkit_name = Some("Slint".into());
        tree.toolkit_version = Some("1.18.1".into());
        TreeUpdate {
            nodes,
            tree: Some(tree),
            tree_id: TreeId::ROOT,
            focus,
        }
    }

    fn add(&mut self, item: ItemRc, scale: ScaleFactor, nodes: &mut Vec<(NodeId, Node)>) -> NodeId {
        let id = self.node_id(&item);
        let children: Vec<NodeId> = descendants(&item)
            .into_iter()
            .map(|child| self.add(child, scale, nodes))
            .collect();
        let mut node = node_for(&item, scale);
        node.set_children(children);
        nodes.push((id, node));
        id
    }
}

/// The items that get their own node under `root`: accessible items and Flickables, found
/// without descending into them. (Slint's `accessible_descendents`, plus Flickables.)
fn descendants(root: &ItemRc) -> Vec<ItemRc> {
    fn visit(item: ItemRc, out: &mut Vec<ItemRc>) {
        let mut child = item.first_child();
        while let Some(c) = child {
            child = c.next_sibling();
            if c.is_accessible() || c.downcast::<Flickable>().is_some() {
                out.push(c);
            } else {
                visit(c, out);
            }
        }
    }
    let mut out = Vec::new();
    visit(root.clone(), &mut out);
    out
}

/// Scrolls a Flickable by most of its height, like a page key.
fn scroll_page(item: &ItemRc, down: bool) {
    let Some(flick) = item.downcast::<Flickable>() else {
        return;
    };
    let flick = flick.as_pin_ref();
    let height = item.geometry().height();
    let max = (flick.content_height().get() - height).max(0.);
    let y = -flick.content_y().get();
    let step = height * 0.8;
    let y = if down {
        (y + step).min(max)
    } else {
        (y - step).max(0.)
    };
    flick.content_y.set(LogicalLength::new(-y));
}

fn prop(item: &ItemRc, what: AccessibleStringProperty) -> Option<String> {
    item.accessible_string_property(what).map(|s| s.to_string())
}

fn flag(item: &ItemRc, what: AccessibleStringProperty) -> bool {
    prop(item, what).is_some_and(|s| s == "true")
}

/// One item's node, without children. Mirrors `build_node_without_children` in Slint's winit
/// backend.
fn node_for(item: &ItemRc, scale: ScaleFactor) -> Node {
    use AccessibleStringProperty as P;

    let (role, label) = if let Some(window) = item.downcast::<WindowItem>() {
        (Role::Window, Some(window.as_pin_ref().title().to_string()))
    } else if let Some(flick) = item.downcast::<Flickable>() {
        let mut node = Node::new(Role::ScrollView);
        let flick = flick.as_pin_ref();
        let max = ((flick.content_height().get() - item.geometry().height()) * scale.get()).max(0.);
        let y = -flick.content_y().get() * scale.get();
        node.set_bounds(bounds(item, scale));
        node.set_clips_children();
        if max > 0. {
            node.set_orientation(Orientation::Vertical);
            node.set_scroll_y(y as f64);
            node.set_scroll_y_min(0.);
            node.set_scroll_y_max(max as f64);
            if y < max - 0.5 {
                node.add_action(Action::ScrollDown);
            }
            if y > 0.5 {
                node.add_action(Action::ScrollUp);
            }
        }
        return node;
    } else {
        (role_of(item), prop(item, P::Label))
    };

    let mut node = Node::new(role);
    if let Some(label) = label {
        if role == Role::Label {
            node.set_value(label);
        } else {
            node.set_label(label);
        }
    }
    if prop(item, P::Enabled).is_some_and(|s| s != "true") {
        node.set_disabled();
    }
    if !item.is_visible() {
        node.set_hidden();
    }

    if item.borrow().as_ref().clips_children() {
        node.set_clips_children();
    }
    node.set_bounds(bounds(item, scale));

    if flag(item, P::Checkable) {
        node.set_toggled(if flag(item, P::Checked) {
            Toggled::True
        } else {
            Toggled::False
        });
    }
    if let Some(description) = prop(item, P::Description) {
        node.set_description(description);
    }
    if let Some(id) = prop(item, P::Id) {
        node.set_author_id(id);
    }
    if flag(item, P::Expandable) {
        node.set_expanded(flag(item, P::Expanded));
    }
    if let Some(min) = prop(item, P::ValueMinimum).and_then(|s| s.parse().ok()) {
        node.set_min_numeric_value(min);
    }
    if let Some(max) = prop(item, P::ValueMaximum).and_then(|s| s.parse().ok()) {
        node.set_max_numeric_value(max);
    }
    if let Some(step) = prop(item, P::ValueStep).and_then(|s| s.parse().ok()) {
        node.set_numeric_value_step(step);
    }
    if let Some(value) = prop(item, P::Value) {
        match value.parse::<f64>() {
            Ok(n) if role == Role::SpinButton => {
                node.set_numeric_value(n);
                node.set_value(value);
            }
            Ok(n) if !is_text_input(role) => node.set_numeric_value(n),
            _ => node.set_value(value),
        }
    }
    if let Some(placeholder) = prop(item, P::PlaceholderText) {
        node.set_placeholder(placeholder);
    }
    if flag(item, P::ReadOnly) {
        node.set_read_only();
    }
    if let Some(o) =
        prop(item, P::Orientation).and_then(|s| s.parse::<i_slint_core::items::Orientation>().ok())
    {
        node.set_orientation(match o {
            i_slint_core::items::Orientation::Horizontal => Orientation::Horizontal,
            i_slint_core::items::Orientation::Vertical => Orientation::Vertical,
        });
    }
    if let Some(live) = prop(item, P::LiveRegion)
        .and_then(|s| s.parse::<i_slint_core::items::AccessibleLiveness>().ok())
    {
        node.set_live(match live {
            i_slint_core::items::AccessibleLiveness::Polite => Live::Polite,
            i_slint_core::items::AccessibleLiveness::Assertive => Live::Assertive,
            _ => Live::Off,
        });
    }
    if flag(item, P::ItemSelectable) {
        node.set_selected(flag(item, P::ItemSelected));
    }
    if let Some(i) = prop(item, P::ItemIndex).and_then(|s| s.parse().ok()) {
        node.set_position_in_set(i);
    }
    if let Some(n) = prop(item, P::ItemCount).and_then(|s| s.parse().ok()) {
        node.set_size_of_set(n);
    }

    if matches!(
        role,
        Role::Button
            | Role::CheckBox
            | Role::ComboBox
            | Role::ListBoxOption
            | Role::Slider
            | Role::SpinButton
            | Role::Tab
    ) || is_text_input(role)
    {
        node.add_action(Action::Focus);
    }
    let supported = item.supported_accessibility_actions();
    for (slint_action, action) in [
        (SupportedAccessibilityAction::Default, Action::Click),
        (SupportedAccessibilityAction::Decrement, Action::Decrement),
        (SupportedAccessibilityAction::Increment, Action::Increment),
        (SupportedAccessibilityAction::SetValue, Action::SetValue),
        (
            SupportedAccessibilityAction::ReplaceSelectedText,
            Action::ReplaceSelectedText,
        ),
        (SupportedAccessibilityAction::Expand, Action::Expand),
    ] {
        if supported.contains(slint_action) {
            node.add_action(action);
        }
    }
    node
}

/// The item's rectangle in physical pixels from the window's top left (= the host view's).
fn bounds(item: &ItemRc, scale: ScaleFactor) -> accesskit::Rect {
    let geometry = item.geometry();
    let origin = (item.map_to_window(geometry.origin) * scale).cast::<f64>();
    let size = (geometry.size * scale).cast::<f64>();
    accesskit::Rect {
        x0: origin.x,
        y0: origin.y,
        x1: origin.x + size.width,
        y1: origin.y + size.height,
    }
}

fn is_text_input(role: Role) -> bool {
    matches!(
        role,
        Role::TextInput
            | Role::MultilineTextInput
            | Role::PasswordInput
            | Role::SearchInput
            | Role::NumberInput
    )
}

fn role_of(item: &ItemRc) -> Role {
    use i_slint_core::items::InputType;
    match item.accessible_role() {
        AccessibleRole::Button => Role::Button,
        AccessibleRole::Checkbox => Role::CheckBox,
        AccessibleRole::Combobox => Role::ComboBox,
        AccessibleRole::Groupbox => Role::Group,
        AccessibleRole::List => Role::ListBox,
        AccessibleRole::Slider => Role::Slider,
        AccessibleRole::Spinbox => Role::SpinButton,
        AccessibleRole::Tab => Role::Tab,
        AccessibleRole::TabList => Role::TabList,
        AccessibleRole::TabPanel => Role::TabPanel,
        AccessibleRole::Text => Role::Label,
        AccessibleRole::Table => Role::Table,
        AccessibleRole::Tree => Role::Tree,
        AccessibleRole::TextInput => match i_slint_core::accessibility::find_text_input(item) {
            Some(input) if !input.single_line.get_internal() => Role::MultilineTextInput,
            Some(input) => match input.input_type.get_internal() {
                InputType::Decimal | InputType::Number => Role::NumberInput,
                InputType::Password => Role::PasswordInput,
                InputType::Search => Role::SearchInput,
                _ => Role::TextInput,
            },
            None => Role::TextInput,
        },
        AccessibleRole::ProgressIndicator => Role::ProgressIndicator,
        AccessibleRole::Switch => Role::Switch,
        AccessibleRole::ListItem => Role::ListBoxOption,
        AccessibleRole::Image => Role::Image,
        AccessibleRole::RadioButton => Role::RadioButton,
        AccessibleRole::RadioGroup => Role::RadioGroup,
        AccessibleRole::Banner => Role::Banner,
        AccessibleRole::Complementary => Role::Complementary,
        AccessibleRole::ContentInfo => Role::ContentInfo,
        AccessibleRole::Form => Role::Form,
        AccessibleRole::Main => Role::Main,
        AccessibleRole::Navigation => Role::Navigation,
        AccessibleRole::Region => Role::Region,
        AccessibleRole::Search => Role::Search,
        _ => Role::Unknown,
    }
}

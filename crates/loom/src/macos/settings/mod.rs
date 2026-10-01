//! A process-wide AppKit Settings window. The shared schema remains the sole
//! source of labels, types, bounds, choices and comment-preserving disk writes.
mod model;

use crossbeam_channel::Sender;
use loom_app::app::SettingsCategory;
use loom_config::config::LoomConfig;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::*;
use objc2_foundation::{
    NSIndexSet, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
    NSURL,
};
use winit::event_loop::EventLoopProxy;

use super::Command;
use crate::app::ui::settings_panel::schema::{self, FieldKind, FieldMeta, SettingsField};
pub(super) use model::save;

#[derive(Clone, Debug)]
pub(super) enum Event {
    Show,
    Category(SettingsCategory),
    Search(String),
    Change(SettingsField, model::Value),
    Resize,
    OpenFile,
}

fn categories() -> Vec<SettingsCategory> {
    SettingsCategory::ALL
        .iter()
        .copied()
        .filter(|category| *category != SettingsCategory::TabBar)
        .collect()
}

struct TargetState {
    sender: Sender<Command>,
    proxy: EventLoopProxy<()>,
}

define_class!(
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    struct FlippedView;
    unsafe impl NSObjectProtocol for FlippedView {}
    impl FlippedView {
        #[unsafe(method(isFlipped))]
        fn flipped(&self) -> bool { true }
    }
);

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = TargetState]
    struct SettingsTarget;
    unsafe impl NSObjectProtocol for SettingsTarget {}
    unsafe impl NSWindowDelegate for SettingsTarget {
        #[unsafe(method(windowShouldClose:))]
        fn should_close(&self, window: &NSWindow) -> bool {
            // Commit the field editor before AppKit hides this reusable window.
            window.makeFirstResponder(None)
        }
        #[unsafe(method(windowDidResize:))]
        fn resized(&self, _: &NSNotification) { self.send(Event::Resize); }
        #[unsafe(method(windowDidBecomeKey:))]
        fn became_key(&self, _: &NSNotification) { self.send(Event::Resize); }
        #[unsafe(method(windowDidResignKey:))]
        fn resigned_key(&self, _: &NSNotification) { self.send(Event::Resize); }
    }
    unsafe impl NSControlTextEditingDelegate for SettingsTarget {}
    unsafe impl NSTableViewDataSource for SettingsTarget {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn count(&self, _: &NSTableView) -> isize { categories().len() as isize }
    }
    unsafe impl NSTableViewDelegate for SettingsTarget {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn category_view(&self, _: &NSTableView, _: Option<&NSTableColumn>, row: isize) -> Option<Retained<NSView>> {
            usize::try_from(row).ok().and_then(|row| categories().get(row).copied()).map(|category| {
            let view = NSTextField::labelWithString(&NSString::from_str(category.label()), self.mtm());
            view.setFont(Some(&NSFont::systemFontOfSize(13.0)));
            view.setFrame(rect(12.0, 5.0, 160.0, 22.0));
            view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
            view.into_super().into_super()
            })
        }
        #[unsafe(method(tableViewSelectionDidChange:))]
        fn category_changed(&self, notification: &NSNotification) {
            if let Some(object) = notification.object()
                && let Some(table) = object.downcast_ref::<NSTableView>()
                && let Ok(index) = usize::try_from(table.selectedRow())
                && let Some(category) = categories().get(index)
            { self.send(Event::Category(*category)); }
        }
    }
    impl SettingsTarget {
        #[unsafe(method(switchChanged:))]
        fn switch_changed(&self, control: &NSSwitch) {
            if let Some(field) = field(control.tag()) {
                self.send(Event::Change(field, model::Value::Bool(control.state() != 0)));
            }
        }
        #[unsafe(method(numberChanged:))]
        fn number_changed(&self, control: &NSTextField) {
            if let Some(field) = field(control.tag()) {
                self.send(Event::Change(field, model::Value::Number(control.stringValue().to_string())));
            }
        }
        #[unsafe(method(stepperChanged:))]
        fn stepper_changed(&self, control: &NSStepper) {
            if let Some(field) = field(control.tag()) {
                self.send(Event::Change(field, model::Value::Number(control.doubleValue().to_string())));
            }
        }
        #[unsafe(method(choiceChanged:))]
        fn choice_changed(&self, control: &NSPopUpButton) {
            if let Some(field) = field(control.tag()) && let Some(value) = control.titleOfSelectedItem() {
                self.send(Event::Change(field, model::Value::Choice(value.to_string())));
            }
        }
        #[unsafe(method(searchChanged:))]
        fn search_changed(&self, control: &NSSearchField) {
            self.send(Event::Search(control.stringValue().to_string()));
        }
        #[unsafe(method(openConfig:))]
        fn open_config(&self, _: &NSButton) { self.send(Event::OpenFile); }
    }
);

impl SettingsTarget {
    fn send(&self, event: Event) {
        let _ = self.ivars().sender.send(Command::Settings(event));
        let _ = self.ivars().proxy.send_event(());
    }
}

fn field(tag: isize) -> Option<SettingsField> {
    u16::try_from(tag).ok().and_then(SettingsField::from_id)
}
fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(
        NSPoint::new(x, y),
        NSSize::new(width.max(0.0), height.max(0.0)),
    )
}
fn flipped(mtm: MainThreadMarker) -> Retained<FlippedView> {
    unsafe { msg_send![FlippedView::alloc(mtm), initWithFrame: NSRect::ZERO] }
}

enum Control {
    Switch(Retained<NSSwitch>),
    Number {
        text: Retained<NSTextField>,
        stepper: Retained<NSStepper>,
    },
    Choice(Retained<NSPopUpButton>),
}
struct Row {
    meta: &'static FieldMeta,
    label: Retained<NSTextField>,
    description: Retained<NSTextField>,
    control: Control,
    divider: Retained<NSBox>,
}

impl Row {
    fn new(
        meta: &'static FieldMeta,
        parent: &NSView,
        target: &SettingsTarget,
        config: &LoomConfig,
    ) -> Self {
        let mtm = target.mtm();
        let label = NSTextField::labelWithString(&NSString::from_str(meta.label), mtm);
        label.setFont(Some(&NSFont::systemFontOfSize(13.0)));
        let description =
            NSTextField::wrappingLabelWithString(&NSString::from_str(meta.description), mtm);
        description.setFont(Some(&NSFont::systemFontOfSize(11.0)));
        description.setTextColor(Some(&NSColor::secondaryLabelColor()));
        parent.addSubview(&label);
        parent.addSubview(&description);
        let control = match &meta.kind {
            FieldKind::Bool => {
                let control = NSSwitch::new(mtm);
                control.setTag(meta.field.id() as isize);
                control.setToolTip(Some(&NSString::from_str(meta.label)));
                unsafe {
                    control.setTarget(Some(target));
                    control.setAction(Some(sel!(switchChanged:)));
                }
                parent.addSubview(&control);
                Control::Switch(control)
            }
            FieldKind::Float { .. } | FieldKind::Int { .. } => {
                let text = NSTextField::new(mtm);
                text.setTag(meta.field.id() as isize);
                text.setToolTip(Some(&NSString::from_str(meta.label)));
                if let Some(cell) = text.cell() {
                    cell.setSendsActionOnEndEditing(true);
                }
                unsafe {
                    text.setTarget(Some(target));
                    text.setAction(Some(sel!(numberChanged:)));
                }
                let stepper = NSStepper::new(mtm);
                stepper.setTag(meta.field.id() as isize);
                stepper.setValueWraps(false);
                let (min, max, step) = match meta.kind {
                    FieldKind::Float { min, max, step, .. } => {
                        (min as f64, max as f64, step as f64)
                    }
                    FieldKind::Int { min, max, step } => (min as f64, max as f64, step as f64),
                    _ => unreachable!(),
                };
                stepper.setMinValue(min);
                stepper.setMaxValue(max);
                stepper.setIncrement(step);
                unsafe {
                    stepper.setTarget(Some(target));
                    stepper.setAction(Some(sel!(stepperChanged:)));
                }
                parent.addSubview(&text);
                parent.addSubview(&stepper);
                Control::Number { text, stepper }
            }
            FieldKind::Enum { .. } => {
                let control = NSPopUpButton::new(mtm);
                control.setTag(meta.field.id() as isize);
                control.setToolTip(Some(&NSString::from_str(meta.label)));
                for choice in schema::enum_variants(meta.field) {
                    control.addItemWithTitle(&NSString::from_str(&choice));
                }
                unsafe {
                    control.setTarget(Some(target));
                    control.setAction(Some(sel!(choiceChanged:)));
                }
                parent.addSubview(&control);
                Control::Choice(control)
            }
        };
        let divider = NSBox::new(mtm);
        divider.setBoxType(NSBoxType::Separator);
        parent.addSubview(&divider);
        let row = Self {
            meta,
            label,
            description,
            control,
            divider,
        };
        row.sync(config);
        row
    }

    fn sync(&self, config: &LoomConfig) {
        match &self.control {
            Control::Switch(control) => {
                control.setState(isize::from(schema::read_bool(self.meta.field, config)))
            }
            Control::Number { text, stepper } => {
                let value = match self.meta.kind {
                    FieldKind::Float { .. } => schema::read_float(self.meta.field, config) as f64,
                    _ => schema::read_int(self.meta.field, config) as f64,
                };
                text.setStringValue(&NSString::from_str(&schema::display_value(
                    self.meta.field,
                    config,
                )));
                stepper.setDoubleValue(value);
            }
            Control::Choice(control) => {
                let value = NSString::from_str(&schema::read_enum(self.meta.field, config));
                if control.itemWithTitle(&value).is_none() {
                    control.addItemWithTitle(&value);
                }
                control.selectItemWithTitle(&value);
            }
        }
    }

    fn layout(&self, y: f64, width: f64) -> f64 {
        let label_width = (width - 248.0).max(100.0);
        self.label.setFrame(rect(24.0, y + 10.0, label_width, 20.0));
        self.description.setPreferredMaxLayoutWidth(label_width);
        let description_height = self
            .description
            .cell()
            .map(|cell| {
                cell.cellSizeForBounds(rect(0.0, 0.0, label_width, 10000.0))
                    .height
            })
            .unwrap_or(30.0)
            .ceil();
        self.description
            .setFrame(rect(24.0, y + 32.0, label_width, description_height));
        let height = (description_height + 46.0).max(64.0);
        let right = width - 24.0;
        match &self.control {
            Control::Switch(control) => control.setFrame(rect(right - 42.0, y + 13.0, 42.0, 25.0)),
            Control::Number { text, stepper } => {
                text.setFrame(rect(right - 176.0, y + 12.0, 144.0, 24.0));
                stepper.setFrame(rect(right - 24.0, y + 10.0, 20.0, 28.0));
            }
            Control::Choice(control) => {
                control.setFrame(rect(right - 188.0, y + 11.0, 188.0, 28.0))
            }
        }
        self.divider
            .setFrame(rect(24.0, y + height - 1.0, width - 48.0, 1.0));
        height
    }
}

pub(super) struct SettingsWindow {
    window: Retained<NSWindow>,
    root: Retained<FlippedView>,
    sidebar_scroll: Retained<NSScrollView>,
    sidebar: Retained<NSTableView>,
    search: Retained<NSSearchField>,
    scroll: Retained<NSScrollView>,
    document: Retained<FlippedView>,
    heading: Retained<NSTextField>,
    status: Retained<NSTextField>,
    open_file: Retained<NSButton>,
    target: Retained<SettingsTarget>,
    category: SettingsCategory,
    query: String,
    config: LoomConfig,
    rows: Vec<Row>,
}

impl SettingsWindow {
    pub fn new(config: LoomConfig, sender: Sender<Command>, proxy: EventLoopProxy<()>) -> Self {
        let mtm = MainThreadMarker::new().expect("AppKit main thread");
        let target: Retained<SettingsTarget> = unsafe {
            msg_send![
                super(SettingsTarget::alloc(mtm).set_ivars(TargetState { sender, proxy })),
                init
            ]
        };
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect(0.0, 0.0, 820.0, 640.0),
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable
                    | NSWindowStyleMask::Resizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        window.setTitle(&NSString::from_str("Settings"));
        window.setContentMinSize(NSSize::new(760.0, 500.0));
        window.setRestorable(false);
        window.setTabbingMode(NSWindowTabbingMode::Disallowed);
        unsafe {
            window.setReleasedWhenClosed(false);
            window.setDelegate(Some(ProtocolObject::from_ref(&*target)));
        }
        let root = flipped(mtm);
        window.setContentView(Some(&root));
        let sidebar_scroll = NSScrollView::new(mtm);
        sidebar_scroll.setHasVerticalScroller(true);
        sidebar_scroll.setAutohidesScrollers(true);
        let sidebar = NSTableView::new(mtm);
        sidebar.setHeaderView(None);
        sidebar.setRowHeight(34.0);
        sidebar.setStyle(NSTableViewStyle::SourceList);
        let column = NSTableColumn::initWithIdentifier(
            NSTableColumn::alloc(mtm),
            &NSString::from_str("Category"),
        );
        column.setWidth(178.0);
        sidebar.addTableColumn(&column);
        unsafe {
            sidebar.setDataSource(Some(ProtocolObject::from_ref(&*target)));
            sidebar.setDelegate(Some(ProtocolObject::from_ref(&*target)));
        }
        sidebar_scroll.setDocumentView(Some(&sidebar));
        root.addSubview(&sidebar_scroll);
        let search = NSSearchField::new(mtm);
        search.setPlaceholderString(Some(&NSString::from_str("Search settings")));
        search.setSendsSearchStringImmediately(true);
        unsafe {
            search.setTarget(Some(&target));
            search.setAction(Some(sel!(searchChanged:)));
        }
        root.addSubview(&search);
        let scroll = NSScrollView::new(mtm);
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        let document = flipped(mtm);
        scroll.setDocumentView(Some(&document));
        root.addSubview(&scroll);
        let heading = NSTextField::labelWithString(&NSString::from_str("Appearance"), mtm);
        heading.setFont(Some(&NSFont::boldSystemFontOfSize(20.0)));
        document.addSubview(&heading);
        let status = NSTextField::wrappingLabelWithString(
            &NSString::from_str("Changes are saved automatically."),
            mtm,
        );
        status.setFont(Some(&NSFont::systemFontOfSize(11.0)));
        status.setTextColor(Some(&NSColor::secondaryLabelColor()));
        root.addSubview(&status);
        let open_file = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Open Configuration File…"),
                Some(&target),
                Some(sel!(openConfig:)),
                mtm,
            )
        };
        open_file.setBezelStyle(NSBezelStyle::Push);
        root.addSubview(&open_file);
        sidebar.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(0), false);
        let mut this = Self {
            window,
            root,
            sidebar_scroll,
            sidebar,
            search,
            scroll,
            document,
            heading,
            status,
            open_file,
            target,
            category: SettingsCategory::Appearance,
            query: String::new(),
            config,
            rows: Vec::new(),
        };
        this.rebuild();
        this.window.center();
        this
    }

    pub fn is_key(&self) -> bool {
        self.window.isKeyWindow()
    }
    pub fn end_editing(&self) {
        self.window.makeFirstResponder(None);
    }
    pub fn close(&self) {
        self.window.performClose(None);
    }
    pub fn show(&mut self, config: LoomConfig) {
        if !self.is_key() {
            self.refresh(config);
        }
        self.window.makeKeyAndOrderFront(None);
        super::native::activate();
    }
    pub fn refresh(&mut self, config: LoomConfig) {
        self.config = config;
        for row in &self.rows {
            row.sync(&self.config);
        }
    }
    pub fn revert(&self) {
        for row in &self.rows {
            row.sync(&self.config);
        }
    }
    pub fn status(&self, message: &str, error: bool) {
        self.status.setStringValue(&NSString::from_str(message));
        let color = if error {
            NSColor::systemRedColor()
        } else {
            NSColor::secondaryLabelColor()
        };
        self.status.setTextColor(Some(&color));
    }
    pub fn navigate(&mut self, event: Event) {
        match event {
            Event::Category(category) => {
                self.category = category;
                self.query.clear();
                self.search.setStringValue(&NSString::from_str(""));
                self.rebuild();
            }
            Event::Search(query) => {
                self.query = query.to_lowercase();
                self.rebuild();
            }
            Event::Resize => self.layout(),
            Event::OpenFile => {
                let path = loom_config::config::config_path();
                if !path.exists()
                    && let Err(error) =
                        loom_config::writer::EditableConfig::load().and_then(|writer| writer.save())
                {
                    self.status(&error.to_string(), true);
                    return;
                }
                let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
                if !NSWorkspace::sharedWorkspace().openURL(&url) {
                    self.status("Could not open the configuration file.", true);
                }
            }
            _ => {}
        }
    }
    fn rebuild(&mut self) {
        self.rows.clear();
        for view in self.document.subviews().to_vec() {
            view.removeFromSuperview();
        }
        self.document.addSubview(&self.heading);
        let title = if self.query.is_empty() {
            self.category.label()
        } else {
            "Search Results"
        };
        self.heading.setStringValue(&NSString::from_str(title));
        for meta in schema::FIELDS.iter().filter(|meta| {
            meta.category != SettingsCategory::TabBar
                && if self.query.is_empty() {
                    meta.category == self.category
                } else {
                    format!(
                        "{} {} {}",
                        meta.label,
                        meta.description,
                        meta.category.label()
                    )
                    .to_lowercase()
                    .contains(&self.query)
                }
        }) {
            self.rows
                .push(Row::new(meta, &self.document, &self.target, &self.config));
        }
        if self.rows.is_empty() {
            self.heading
                .setStringValue(&NSString::from_str("No matching settings"));
        }
        self.layout();
        self.scroll.contentView().scrollToPoint(NSPoint::ZERO);
        self.scroll
            .reflectScrolledClipView(&self.scroll.contentView());
    }
    pub fn layout(&self) {
        let size = self.root.bounds().size;
        self.search.setFrame(rect(16.0, 18.0, 166.0, 26.0));
        self.sidebar_scroll
            .setFrame(rect(8.0, 60.0, 182.0, size.height - 72.0));
        self.sidebar.setFrameSize(NSSize::new(
            self.sidebar_scroll.contentSize().width,
            categories().len() as f64 * 36.0,
        ));
        self.scroll
            .setFrame(rect(200.0, 0.0, size.width - 200.0, size.height - 82.0));
        let width = self.scroll.contentSize().width;
        self.heading.setFrame(rect(24.0, 20.0, width - 48.0, 30.0));
        let mut y = 64.0;
        for row in &self.rows {
            y += row.layout(y, width);
        }
        self.document.setFrameSize(NSSize::new(
            width,
            (y + 20.0).max(self.scroll.contentSize().height),
        ));
        self.status.setPreferredMaxLayoutWidth(size.width - 248.0);
        self.status
            .setFrame(rect(224.0, size.height - 78.0, size.width - 248.0, 36.0));
        self.open_file
            .setFrame(rect(size.width - 250.0, size.height - 38.0, 226.0, 28.0));
    }
}

impl Drop for SettingsWindow {
    fn drop(&mut self) {
        unsafe {
            self.window.setDelegate(None);
            self.sidebar.setDataSource(None);
            self.sidebar.setDelegate(None);
        }
        self.window.close();
    }
}

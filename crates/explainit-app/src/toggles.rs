//! Menu rows with a native switch. Unlike plain menu items, toggling one keeps the menu open
//! (the same pattern as the Wi-Fi and Bluetooth rows in Control Center).

use std::cell::OnceCell;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send, sel};
use objc2::{MainThreadMarker, Message};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSControlSize, NSControlStateValueOff, NSControlStateValueOn, NSEvent, NSFont,
    NSMenuItem, NSSwitch, NSTextField, NSView,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

const ROW_WIDTH: f64 = 280.0;
const ROW_HEIGHT: f64 = 26.0;
/// Lines the title up with standard menu item titles (which leave room for a check mark).
const LEADING: f64 = 20.0;
const TRAILING: f64 = 14.0;

pub struct ToggleIvars {
    switch: OnceCell<Retained<NSSwitch>>,
    on_change: Box<dyn Fn(bool)>,
}

define_class!(
    /// Title on the left, switch on the right; a click anywhere on the row toggles it.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ExplainItToggleRow"]
    #[ivars = ToggleIvars]
    pub struct ToggleRow;

    impl ToggleRow {
        #[unsafe(method(switchToggled:))]
        fn switch_toggled(&self, _sender: &AnyObject) {
            self.notify();
        }

        // Accept the mouse-down so the matching mouse-up (a click on the title) comes to the row.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {}

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            self.set_on(!self.is_on());
            self.notify();
        }
    }
);

impl ToggleRow {
    /// A menu item holding a new row; `on_change` runs (on the main thread) whenever the user flips it.
    pub fn menu_item(
        mtm: MainThreadMarker,
        title: &str,
        on: bool,
        on_change: impl Fn(bool) + 'static,
    ) -> (Retained<NSMenuItem>, Retained<ToggleRow>) {
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(ROW_WIDTH, ROW_HEIGHT));
        let row = Self::alloc(mtm).set_ivars(ToggleIvars { switch: OnceCell::new(), on_change: Box::new(on_change) });
        let row: Retained<Self> = unsafe { msg_send![super(row), initWithFrame: frame] };
        row.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);

        let switch = NSSwitch::initWithFrame(NSSwitch::alloc(mtm), frame);
        switch.setControlSize(NSControlSize::Small);
        switch.sizeToFit();
        let switch_size = switch.frame().size;
        switch.setFrameOrigin(NSPoint::new(
            ROW_WIDTH - TRAILING - switch_size.width,
            ((ROW_HEIGHT - switch_size.height) / 2.0).round(),
        ));
        switch.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
        switch.setState(if on { NSControlStateValueOn } else { NSControlStateValueOff });
        let target: &AnyObject = &row;
        unsafe {
            switch.setTarget(Some(target));
            switch.setAction(Some(sel!(switchToggled:)));
        }

        let label = NSTextField::labelWithString(&NSString::from_str(title), mtm);
        label.setFont(Some(&NSFont::menuFontOfSize(0.0)));
        label.sizeToFit();
        let label_height = label.frame().size.height;
        label.setFrame(NSRect::new(
            NSPoint::new(LEADING, ((ROW_HEIGHT - label_height) / 2.0).round()),
            NSSize::new(ROW_WIDTH - LEADING - TRAILING - switch_size.width - 8.0, label_height),
        ));
        label.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);

        row.addSubview(&label);
        row.addSubview(&switch);
        let _ = row.ivars().switch.set(switch);
        let item = NSMenuItem::new(mtm);
        item.setView(Some(&row));
        (item, row.retain())
    }

    pub fn set_on(&self, on: bool) {
        if let Some(switch) = self.ivars().switch.get() {
            switch.setState(if on { NSControlStateValueOn } else { NSControlStateValueOff });
        }
    }

    fn is_on(&self) -> bool {
        self.ivars().switch.get().is_some_and(|s| s.state() == NSControlStateValueOn)
    }

    fn notify(&self) {
        (self.ivars().on_change)(self.is_on());
    }
}

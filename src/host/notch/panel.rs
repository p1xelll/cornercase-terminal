use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSApplicationActivationPolicy, NSApplicationDelegate,
    NSAutoresizingMaskOptions, NSBackingStoreType, NSBezierPath, NSColor, NSEvent, NSFont, NSFontWeight,
    NSFontWeightBold, NSFontWeightMedium, NSFontWeightRegular, NSFontWeightSemibold, NSLineBreakMode, NSPanel,
    NSRunningApplication, NSScreen, NSStatusWindowLevel, NSTextAlignment, NSTextField, NSTrackingArea,
    NSTrackingAreaOptions, NSView, NSWindowCollectionBehavior, NSWindowStyleMask, NSWorkspace,
};
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSTimer};
use parking_lot::Mutex;

use super::card::{self, Outcome, Press};
use super::motion::{Frame, Motion, SETTLE, Size};
use super::rinco::{self, Mood};
use super::{Action, Board, Card, Usage};
use crate::process;

const WIDTH: f64 = 380.0;
const SIDE: f64 = 14.0;
const BOTTOM: f64 = 14.0;
const BELOW_NOTCH: f64 = 6.0;
const NO_NOTCH_TOP: f64 = 20.0;
const GAP: f64 = 7.0;
const RADIUS: f64 = 22.0;
const HEADER: f64 = 21.0;
const HEADER_X: f64 = 4.0;
const HEADER_GAP: f64 = 7.0;
const HEADER_MASCOT: f64 = 16.0;
const CARD_RADIUS: f64 = 11.0;
const CARD_V: f64 = 11.0;
const CARD_H: f64 = 12.0;
const MASCOT: f64 = 32.0;
const MASCOT_GAP: f64 = 10.0;
const LINE_GAP: f64 = 3.0;
const TITLE: f64 = 17.0;
const SUBTITLE: f64 = 13.0;
const STATUS: f64 = 14.0;
const PILLS_TOP: f64 = 5.0;
const PILL: f64 = 23.0;
const PILL_PAD: f64 = 14.0;
const OPEN_PAD: f64 = 12.0;
const PILL_GAP: f64 = 6.0;
const CHIP: f64 = 17.0;
const CHIP_PAD: f64 = 7.0;
const CHIP_RADIUS: f64 = 4.0;
const META_GAP: f64 = 6.0;
const TIME: f64 = 26.0;
const ROW: f64 = 24.0;
const ROW_TOP: f64 = 4.0;
const ROW_RADIUS: f64 = 8.0;
const SQUARE: f64 = 7.0;
const ROW_GAP: f64 = 9.0;
const NO_NOTCH_WIDTH: f64 = 120.0;
const NO_NOTCH_HEIGHT: f64 = 1.0;
const MAX_CARDS: usize = 5;
const RESULT_FOR: Duration = Duration::from_millis(2400);
const POLL_EVERY: f64 = 0.1;
const FRAME_EVERY: f64 = 1.0 / 60.0;
const MASCOT_EVERY: f64 = 1.0 / 30.0;
const HEADER_KEY: u64 = u64::MAX;
const CARD_FILL: u32 = 0x1c_1b_20;
const CHIP_FILL: u32 = 0x2e_2d_33;
const CHIP_TEXT: u32 = 0xc4_c0_d6;
const WHITE: u32 = 0xff_ff_ff;
const SUBTLE: u32 = 0x95_91_ab;
const HEADER_MUTED: u32 = 0x8e_8a_a1;
const SEPARATOR: u32 = 0x3c_39_49;
const ROW_HOVER: u32 = 0x14_13_18;

#[derive(Clone, Copy)]
enum Weight {
    Regular,
    Medium,
    Semibold,
    Bold,
}

impl Weight {
    fn value(self) -> NSFontWeight {
        unsafe {
            match self {
                Self::Regular => NSFontWeightRegular,
                Self::Medium => NSFontWeightMedium,
                Self::Semibold => NSFontWeightSemibold,
                Self::Bold => NSFontWeightBold,
            }
        }
    }
}

#[derive(Default)]
struct Inbox {
    board: Option<Board>,
    ended: bool,
}

type Shared = Arc<Mutex<Inbox>>;

#[derive(Clone, Copy)]
struct Place {
    screen: NSRect,
    top: f64,
    notch: Size,
}

#[derive(Clone)]
enum Click {
    Press(Press, Box<Card>),
    Select(u64),
}

#[derive(Clone, Copy)]
enum Clock {
    Since(u64),
    Resets(i64),
}

#[derive(Debug, Clone, PartialEq)]
struct Item {
    card: Card,
    outcome: Option<Outcome>,
    full: bool,
}

impl Item {
    fn mood(&self) -> Mood {
        self.outcome.as_ref().map_or_else(|| card::mood(&self.card), Outcome::mood)
    }
}

#[derive(PartialEq)]
struct Shown {
    items: Vec<Item>,
    usage: Vec<Usage>,
}

struct Ivars {
    inbox: Shared,
    panel: OnceCell<Retained<NSPanel>>,
    terminal: Option<Retained<NSRunningApplication>>,
    board: RefCell<Board>,
    dismissed: RefCell<Vec<Card>>,
    answered: RefCell<Vec<(Card, Outcome, Instant)>>,
    selected: Cell<Option<u64>>,
    shown: RefCell<Option<Shown>>,
    clicks: RefCell<Vec<Click>>,
    clocks: RefCell<Vec<(Retained<NSTextField>, Clock)>>,
    mascots: RefCell<Vec<Retained<Sprite>>>,
    moods: RefCell<HashMap<(u64, Mood), Instant>>,
    content: RefCell<Option<Retained<NSView>>>,
    target: Cell<Size>,
    size: Cell<Option<Size>>,
    place: Cell<Option<Place>>,
    motion: Cell<Option<(Motion, Instant)>>,
    animating: RefCell<Option<Retained<NSTimer>>>,
    blinking: RefCell<Option<Retained<NSTimer>>>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    struct Notch;

    unsafe impl NSObjectProtocol for Notch {}

    unsafe impl NSApplicationDelegate for Notch {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            let panel = make_panel(self.mtm());
            let _ = self.ivars().panel.set(panel);
            unsafe {
                NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                    POLL_EVERY,
                    self,
                    sel!(tick:),
                    None,
                    true,
                );
            }
        }
    }

    impl Notch {
        #[unsafe(method(tick:))]
        fn tick(&self, _timer: &NSTimer) {
            let (board, ended) = {
                let mut inbox = self.ivars().inbox.lock();
                (inbox.board.take(), inbox.ended)
            };
            if ended {
                std::process::exit(0);
            }
            let expired = self.expire();
            if let Some(board) = board {
                self.ivars().dismissed.borrow_mut().retain(|c| board.cards.contains(c));
                *self.ivars().board.borrow_mut() = board;
                self.render();
            } else if expired {
                self.render();
            }
            self.update_clocks();
        }

        #[unsafe(method(frame:))]
        fn frame(&self, _timer: &NSTimer) {
            self.step();
        }

        #[unsafe(method(blink:))]
        fn blink(&self, _timer: &NSTimer) {
            for sprite in self.ivars().mascots.borrow().iter() {
                sprite.advance();
            }
        }
    }
);

#[derive(Default)]
struct ShapeIvars {
    fill: Cell<Option<u32>>,
    hover: Cell<Option<u32>>,
    hovered: Cell<bool>,
    radius: Cell<f64>,
    click: Cell<Option<usize>>,
    notch: OnceCell<Retained<Notch>>,
}

define_class!(
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[ivars = ShapeIvars]
    struct Shape;

    impl Shape {
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {}

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            if let (Some(click), Some(notch)) = (self.ivars().click.get(), self.ivars().notch.get()) {
                notch.clicked(click);
            }
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            self.ivars().hovered.set(true);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.ivars().hovered.set(false);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let ivars = self.ivars();
            let hovered = ivars.hovered.get().then(|| ivars.hover.get()).flatten();
            if let Some(fill) = hovered.or(ivars.fill.get()) {
                colour(fill, 1.0).setFill();
                let radius = ivars.radius.get();
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(self.bounds(), radius, radius).fill();
            }
        }
    }
);

struct SpriteIvars {
    mood: Cell<Mood>,
    since: Cell<Instant>,
    frame: Cell<Option<usize>>,
    pixel: Cell<f64>,
}

define_class!(
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[ivars = SpriteIvars]
    struct Sprite;

    impl Sprite {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let (pixel, height) = (self.ivars().pixel.get(), self.bounds().size.height);
            let Some(rows) = rinco::frame(self.ivars().mood.get(), self.ivars().since.get().elapsed()) else { return };
            for (y, row) in (0_u32..).zip(rows) {
                for (x, dot) in (0_u32..).zip(row.chars()) {
                    let Some(rgb) = rinco::colour(dot) else { continue };
                    colour(rgb, 1.0).setFill();
                    let top = height - f64::from(y + 1) * pixel;
                    NSBezierPath::fillRect(rect(f64::from(x) * pixel, top, pixel, pixel));
                }
            }
        }
    }
);

impl Sprite {
    fn advance(&self) {
        let frame = rinco::index(self.ivars().mood.get(), self.ivars().since.get().elapsed());
        if self.ivars().frame.replace(frame) != frame {
            self.setNeedsDisplay(true);
        }
    }
}

define_class!(
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    struct Background;

    impl Background {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            NSColor::blackColor().setFill();
            NSBezierPath::fillRect(self.bounds());
        }
    }
);

impl Notch {
    fn new(mtm: MainThreadMarker, inbox: Shared) -> Retained<Self> {
        let ivars = Ivars {
            inbox,
            panel: OnceCell::new(),
            terminal: terminal_app(),
            board: RefCell::new(Board::default()),
            dismissed: RefCell::new(Vec::new()),
            answered: RefCell::new(Vec::new()),
            selected: Cell::new(None),
            shown: RefCell::new(None),
            clicks: RefCell::new(Vec::new()),
            clocks: RefCell::new(Vec::new()),
            mascots: RefCell::new(Vec::new()),
            moods: RefCell::new(HashMap::new()),
            content: RefCell::new(None),
            target: Cell::new(Size { width: WIDTH, height: 0.0 }),
            size: Cell::new(None),
            place: Cell::new(None),
            motion: Cell::new(None),
            animating: RefCell::new(None),
            blinking: RefCell::new(None),
        };
        let this = Self::alloc(mtm).set_ivars(ivars);
        unsafe { msg_send![super(this), init] }
    }

    fn clicked(&self, index: usize) {
        let click = self.ivars().clicks.borrow().get(index).cloned();
        match click {
            Some(Click::Press(Press::Open(pane), _)) => self.open(pane),
            Some(Click::Press(Press::Answer(action), card)) => {
                send(&action);
                if let Action::Answer { key, .. } = &action
                    && let Some(outcome) = card::outcome(&card, *key)
                {
                    self.ivars().answered.borrow_mut().push((*card, outcome, Instant::now()));
                } else {
                    self.ivars().dismissed.borrow_mut().push(*card);
                }
            }
            Some(Click::Press(Press::Dismiss, card)) => self.ivars().dismissed.borrow_mut().push(*card),
            Some(Click::Select(pane)) => self.ivars().selected.set(Some(pane)),
            None => {}
        }
        self.render();
    }

    fn expire(&self) -> bool {
        let mut answered = self.ivars().answered.borrow_mut();
        let before = answered.len();
        let (gone, kept): (Vec<_>, Vec<_>) = answered.drain(..).partition(|(_, _, at)| at.elapsed() >= RESULT_FOR);
        *answered = kept;
        self.ivars().dismissed.borrow_mut().extend(gone.into_iter().map(|(card, _, _)| card));
        answered.len() != before
    }

    fn open(&self, pane: u64) {
        if let Some(app) = &self.ivars().terminal {
            app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
        }
        send(&Action::Open(pane));
    }

    fn items(&self) -> Vec<Item> {
        let (board, dismissed, answered) =
            (self.ivars().board.borrow(), self.ivars().dismissed.borrow(), self.ivars().answered.borrow());
        let mut items: Vec<(Card, Option<Outcome>)> = board
            .cards
            .iter()
            .filter(|c| !dismissed.contains(c) && !answered.iter().any(|(a, _, _)| a.pane == c.pane))
            .map(|c| (c.clone(), None))
            .collect();
        for (card, outcome, _) in answered.iter().rev() {
            items.insert(0, (card.clone(), Some(outcome.clone())));
        }
        items.truncate(MAX_CARDS);
        let cards: Vec<Card> = items.iter().map(|(c, _)| c.clone()).collect();
        let done: Vec<u64> = answered.iter().map(|(c, _, _)| c.pane).collect();
        let selected = self.ivars().selected.get().filter(|pane| cards.iter().any(|c| c.pane == *pane));
        let full = card::full(&cards, selected, &done);
        items.into_iter().zip(full).map(|((card, outcome), full)| Item { card, outcome, full }).collect()
    }

    fn render(&self) {
        let Some(panel) = self.ivars().panel.get() else { return };
        let mtm = self.mtm();
        let mut items = self.items();
        if !items.iter().any(|i| i.outcome.is_some() || card::needs_you(&i.card)) {
            items.clear();
        }
        let usage = self.ivars().board.borrow().usage.clone();
        let shown = Shown { items: items.clone(), usage: usage.clone() };
        if self.ivars().shown.borrow().as_ref() == Some(&shown) {
            return;
        }
        *self.ivars().shown.borrow_mut() = Some(shown);
        let place = place(mtm);
        self.ivars().place.set(Some(place));
        let from = self.ivars().size.get();
        if items.is_empty() {
            self.ivars().selected.set(None);
            if let Some(from) = from {
                self.animate(Motion { from, to: place.notch, opening: false, fresh: false });
            }
            return;
        }
        let (content, target) = self.lay_out(mtm, &items, &usage, place.top);
        if let Some(old) = self.ivars().content.replace(Some(content.clone())) {
            old.removeFromSuperview();
        }
        if let Some(background) = panel.contentView() {
            background.addSubview(&content);
        }
        self.ivars().target.set(target);
        panel.orderFrontRegardless();
        self.blink_on();
        self.animate(Motion { from: from.unwrap_or(place.notch), to: target, opening: true, fresh: from.is_none() });
    }

    fn lay_out(&self, mtm: MainThreadMarker, items: &[Item], usage: &[Usage], top: f64) -> (Retained<NSView>, Size) {
        let heights: Vec<f64> = items.iter().map(|i| if i.full { card_height(i) } else { ROW }).collect();
        let below = if top > 0.0 { top + BELOW_NOTCH } else { NO_NOTCH_TOP };
        let gaps = GAP * f64::from(u32::try_from(heights.len()).unwrap_or(0));
        let target = Size { width: WIDTH, height: below + HEADER + gaps + heights.iter().sum::<f64>() + BOTTOM };
        let content: Retained<NSView> =
            unsafe { msg_send![NSView::alloc(mtm), initWithFrame: rect(0.0, 0.0, target.width, target.height)] };
        self.ivars().clicks.borrow_mut().clear();
        self.ivars().clocks.borrow_mut().clear();
        self.ivars().mascots.borrow_mut().clear();
        let mut used = Vec::new();
        let mut y = target.height - below - HEADER;
        let head = rinco::urgent(items.iter().map(Item::mood));
        self.header(mtm, &content, usage, y, &self.sprite(mtm, (HEADER_KEY, head), 1.0, &mut used));
        for (item, height) in items.iter().zip(heights) {
            y -= GAP + height;
            let frame = rect(SIDE, y, WIDTH - 2.0 * SIDE, height);
            let view = if item.full { self.full(mtm, item, frame, &mut used) } else { self.row(mtm, item, frame) };
            content.addSubview(&view);
        }
        self.ivars().moods.borrow_mut().retain(|key, _| used.contains(key));
        self.update_clocks();
        (content, target)
    }

    fn sprite(
        &self,
        mtm: MainThreadMarker,
        key: (u64, Mood),
        pixel: f64,
        used: &mut Vec<(u64, Mood)>,
    ) -> Retained<Sprite> {
        let since = *self.ivars().moods.borrow_mut().entry(key).or_insert_with(Instant::now);
        used.push(key);
        let side = pixel * f64::from(u32::try_from(rinco::SIZE).unwrap_or(0));
        let ivars = SpriteIvars {
            mood: Cell::new(key.1),
            since: Cell::new(since),
            frame: Cell::new(None),
            pixel: Cell::new(pixel),
        };
        let view = Sprite::alloc(mtm).set_ivars(ivars);
        let view: Retained<Sprite> = unsafe { msg_send![super(view), initWithFrame: rect(0.0, 0.0, side, side)] };
        self.ivars().mascots.borrow_mut().push(view.clone());
        view
    }

    fn header(&self, mtm: MainThreadMarker, content: &NSView, usage: &[Usage], y: f64, mascot: &Sprite) {
        let mut x = SIDE + HEADER_X;
        mascot.setFrameOrigin(NSPoint::new(x, y + (HEADER - HEADER_MASCOT) / 2.0));
        content.addSubview(mascot);
        x += HEADER_MASCOT + HEADER_GAP;
        let middle = y + (HEADER - STATUS) / 2.0;
        for (i, window) in usage.iter().enumerate() {
            if i > 0 {
                x = text(mtm, content, "|", (x + 4.0, middle), (11.0, Weight::Semibold, SEPARATOR)) + 4.0 + HEADER_GAP;
            }
            x = text(mtm, content, &window.label, (x, middle), (11.0, Weight::Semibold, WHITE)) + HEADER_GAP;
            let percent = format!("{}%", window.percent);
            x = text(mtm, content, &percent, (x, middle), (11.0, Weight::Semibold, window.tone.rgb())) + HEADER_GAP;
            if let Some(at) = window.resets_at {
                let field = label(mtm, &card::left(at - now()), 11.0, Weight::Medium, HEADER_MUTED);
                let width = field.fittingSize().width + 8.0;
                field.setFrame(rect(x, middle, width, STATUS));
                content.addSubview(&field);
                self.ivars().clocks.borrow_mut().push((field, Clock::Resets(at)));
                x += width;
            }
        }
    }

    fn clickable(&self, shape: &Shape, click: Click, hover: Option<u32>) {
        let mut clicks = self.ivars().clicks.borrow_mut();
        shape.ivars().click.set(Some(clicks.len()));
        clicks.push(click);
        let _ = shape.ivars().notch.set(self.retain());
        if let Some(hover) = hover {
            shape.ivars().hover.set(Some(hover));
            let options = NSTrackingAreaOptions::MouseEnteredAndExited
                | NSTrackingAreaOptions::ActiveAlways
                | NSTrackingAreaOptions::InVisibleRect;
            let area = unsafe {
                NSTrackingArea::initWithRect_options_owner_userInfo(
                    NSTrackingArea::alloc(),
                    shape.bounds(),
                    options,
                    Some(shape),
                    None,
                )
            };
            shape.addTrackingArea(&area);
        }
    }

    fn full(&self, mtm: MainThreadMarker, item: &Item, frame: NSRect, used: &mut Vec<(u64, Mood)>) -> Retained<Shape> {
        let card = &item.card;
        let view = shape(mtm, frame, Some(CARD_FILL), CARD_RADIUS);
        self.clickable(&view, Click::Press(Press::Open(card.pane), Box::new(card.clone())), None);
        let width = frame.size.width;
        let top = frame.size.height - CARD_V;
        let mut x = CARD_H;
        let sprite = self.sprite(mtm, (card.pane, item.mood()), 2.0, used);
        sprite.setFrameOrigin(NSPoint::new(x, top - MASCOT));
        view.addSubview(&sprite);
        x += MASCOT + MASCOT_GAP;
        let right = self.meta(mtm, &view, card, width - CARD_H, top - f64::midpoint(TITLE, CHIP), CHIP_FILL);
        let title = label(mtm, &card.title, 13.0, Weight::Bold, WHITE);
        title.setFrame(rect(x, top - TITLE, right - META_GAP - x, TITLE));
        view.addSubview(&title);
        let mut y = top - TITLE;
        let subtitle = card::subtitle(card);
        if !subtitle.is_empty() {
            y -= LINE_GAP + SUBTITLE;
            let field = label(mtm, &subtitle, 10.0, Weight::Regular, SUBTLE);
            field.setFrame(rect(x, y, width - CARD_H - x, SUBTITLE));
            view.addSubview(&field);
        }
        let (status, tint, mono) = item.outcome.as_ref().map_or_else(
            || {
                let (line, tint) = card::status(card);
                (line, tint, false)
            },
            |outcome| {
                let (line, tint) = outcome.line();
                (line, tint, outcome.monospaced())
            },
        );
        y -= LINE_GAP + STATUS;
        let field =
            if mono { mono_label(mtm, &status, 10.5, tint) } else { label(mtm, &status, 10.5, Weight::Regular, tint) };
        field.setFrame(rect(x, y, width - CARD_H - x, STATUS));
        view.addSubview(&field);
        if item.outcome.is_none() {
            self.pills(mtm, &view, card, x, width - CARD_H, y - PILLS_TOP - PILL);
        }
        view
    }

    fn pills(&self, mtm: MainThreadMarker, parent: &NSView, card: &Card, from: f64, right: f64, y: f64) {
        let mut x = from;
        for button in card::buttons(card) {
            let field = label(mtm, &button.label, 10.0, Weight::Bold, button.text);
            let pad = if matches!(button.press, Press::Answer(_)) { PILL_PAD } else { OPEN_PAD };
            let width = (field.fittingSize().width + 2.0 * pad).min(right - x);
            if width <= 2.0 * pad {
                break;
            }
            let pill = shape(mtm, rect(x, y, width, PILL), Some(button.fill), PILL / 2.0);
            self.clickable(&pill, Click::Press(button.press, Box::new(card.clone())), Some(lighter(button.fill)));
            field.setAlignment(NSTextAlignment::Center);
            field.setFrame(rect(0.0, (PILL - STATUS) / 2.0, width, STATUS));
            pill.addSubview(&field);
            parent.addSubview(&pill);
            x += width + PILL_GAP;
        }
    }

    fn row(&self, mtm: MainThreadMarker, item: &Item, frame: NSRect) -> Retained<Shape> {
        let card = &item.card;
        let view = shape(mtm, frame, None, ROW_RADIUS);
        self.clickable(&view, Click::Select(card.pane), Some(ROW_HOVER));
        let middle = (ROW - ROW_TOP) / 2.0;
        let square = shape(mtm, rect(CARD_H, middle - SQUARE / 2.0, SQUARE, SQUARE), Some(item.mood().antenna()), 0.0);
        view.addSubview(&square);
        let right = self.meta(mtm, &view, card, frame.size.width - CARD_H, middle - CHIP / 2.0, CARD_FILL);
        let x = CARD_H + SQUARE + ROW_GAP;
        let title = label(mtm, &card.title, 11.5, Weight::Regular, WHITE);
        title.setFrame(rect(x, middle - TITLE / 2.0, right - META_GAP - x, TITLE));
        view.addSubview(&title);
        view
    }

    fn meta(&self, mtm: MainThreadMarker, parent: &NSView, card: &Card, right: f64, y: f64, chip_fill: u32) -> f64 {
        let mut x = right;
        if let Some(since) = card.since {
            let field = label(mtm, &card::ago(elapsed(since)), 10.0, Weight::Regular, CHIP_TEXT);
            field.setAlignment(NSTextAlignment::Right);
            field.setFrame(rect(x - TIME, y + (CHIP - STATUS) / 2.0, TIME, STATUS));
            parent.addSubview(&field);
            self.ivars().clocks.borrow_mut().push((field, Clock::Since(since)));
            x -= TIME + META_GAP;
        }
        let field = label(mtm, &card.agent, 9.5, Weight::Regular, CHIP_TEXT);
        let width = field.fittingSize().width + 2.0 * CHIP_PAD;
        x -= width;
        let chip = shape(mtm, rect(x, y, width, CHIP), Some(chip_fill), CHIP_RADIUS);
        field.setAlignment(NSTextAlignment::Center);
        field.setFrame(rect(0.0, (CHIP - SUBTITLE) / 2.0, width, SUBTITLE));
        chip.addSubview(&field);
        parent.addSubview(&chip);
        x
    }

    fn update_clocks(&self) {
        for (field, clock) in self.ivars().clocks.borrow().iter() {
            let shown = match clock {
                Clock::Since(since) => card::ago(elapsed(*since)),
                Clock::Resets(at) => card::left(at - now()),
            };
            if field.stringValue().to_string() != shown {
                field.setStringValue(&NSString::from_str(&shown));
            }
        }
    }

    fn blink_on(&self) {
        let mut blinking = self.ivars().blinking.borrow_mut();
        if blinking.is_none() {
            *blinking = Some(unsafe {
                NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                    MASCOT_EVERY,
                    self,
                    sel!(blink:),
                    None,
                    true,
                )
            });
        }
    }

    fn animate(&self, motion: Motion) {
        let now = Instant::now();
        let reduce = NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion();
        let started = if reduce { now.checked_sub(SETTLE).unwrap_or(now) } else { now };
        self.ivars().motion.set(Some((motion, started)));
        self.step();
        let mut animating = self.ivars().animating.borrow_mut();
        if animating.is_none() && self.ivars().motion.get().is_some() {
            *animating = Some(unsafe {
                NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                    FRAME_EVERY,
                    self,
                    sel!(frame:),
                    None,
                    true,
                )
            });
        }
    }

    fn step(&self) {
        let Some((motion, started)) = self.ivars().motion.get() else { return };
        let frame = motion.at(started.elapsed());
        self.apply(frame);
        if !frame.done {
            return;
        }
        self.ivars().motion.set(None);
        if let Some(timer) = self.ivars().animating.borrow_mut().take() {
            timer.invalidate();
        }
        let empty = self.ivars().shown.borrow().as_ref().is_none_or(|shown| shown.items.is_empty());
        if !motion.opening && empty {
            self.ivars().size.set(None);
            self.ivars().mascots.borrow_mut().clear();
            if let Some(timer) = self.ivars().blinking.borrow_mut().take() {
                timer.invalidate();
            }
            if let Some(content) = self.ivars().content.take() {
                content.removeFromSuperview();
            }
            if let Some(panel) = self.ivars().panel.get() {
                panel.orderOut(None);
            }
        }
    }

    fn apply(&self, frame: Frame) {
        let (Some(panel), Some(place)) = (self.ivars().panel.get(), self.ivars().place.get()) else { return };
        let Frame { size, alpha, .. } = frame;
        let screen = place.screen;
        let x = screen.origin.x + (screen.size.width - size.width) / 2.0;
        let y = screen.origin.y + screen.size.height - size.height;
        panel.setFrame_display(rect(x, y, size.width, size.height + RADIUS), true);
        panel.invalidateShadow();
        if let Some(layer) = panel.contentView().and_then(|view| view.layer()) {
            layer.setCornerRadius(RADIUS.min(f64::midpoint(size.height, RADIUS)));
        }
        self.ivars().size.set(Some(size));
        if let Some(content) = self.ivars().content.borrow().as_ref() {
            let target = self.ivars().target.get();
            content.setFrameOrigin(NSPoint::new((size.width - target.width) / 2.0, size.height - target.height));
            content.setAlphaValue(alpha);
        }
    }
}

fn card_height(item: &Item) -> f64 {
    let subtitle = if card::subtitle(&item.card).is_empty() { 0.0 } else { LINE_GAP + SUBTITLE };
    let pills = if item.outcome.is_none() { PILLS_TOP + PILL } else { 0.0 };
    let column = TITLE + subtitle + LINE_GAP + STATUS + pills;
    2.0 * CARD_V + column.max(MASCOT)
}

fn lighter(rgb: u32) -> u32 {
    let lift = |shift: u32| (((rgb >> shift) & 0xff) + 0x14).min(0xff) << shift;
    lift(16) | lift(8) | lift(0)
}

fn colour(rgb: u32, alpha: f64) -> Retained<NSColor> {
    let part = |shift: u32| f64::from((rgb >> shift) & 0xff) / 255.0;
    NSColor::colorWithSRGBRed_green_blue_alpha(part(16), part(8), part(0), alpha)
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn elapsed(since: u64) -> u64 {
    u64::try_from(now()).unwrap_or(0).saturating_sub(since)
}

fn shape(mtm: MainThreadMarker, frame: NSRect, fill: Option<u32>, radius: f64) -> Retained<Shape> {
    let ivars = ShapeIvars { fill: Cell::new(fill), radius: Cell::new(radius), ..ShapeIvars::default() };
    let view = Shape::alloc(mtm).set_ivars(ivars);
    unsafe { msg_send![super(view), initWithFrame: frame] }
}

fn text(
    mtm: MainThreadMarker,
    parent: &NSView,
    shown: &str,
    (x, y): (f64, f64),
    (size, weight, rgb): (f64, Weight, u32),
) -> f64 {
    let field = label(mtm, shown, size, weight, rgb);
    let width = field.fittingSize().width;
    field.setFrame(rect(x, y, width, STATUS));
    parent.addSubview(&field);
    x + width
}

fn label(mtm: MainThreadMarker, shown: &str, size: f64, weight: Weight, rgb: u32) -> Retained<NSTextField> {
    styled(mtm, shown, &NSFont::systemFontOfSize_weight(size, weight.value()), rgb)
}

fn mono_label(mtm: MainThreadMarker, shown: &str, size: f64, rgb: u32) -> Retained<NSTextField> {
    styled(mtm, shown, &NSFont::monospacedSystemFontOfSize_weight(size, Weight::Regular.value()), rgb)
}

fn styled(mtm: MainThreadMarker, shown: &str, font: &NSFont, rgb: u32) -> Retained<NSTextField> {
    let field = NSTextField::labelWithString(&NSString::from_str(shown), mtm);
    field.setFont(Some(font));
    field.setTextColor(Some(&colour(rgb, 1.0)));
    field.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    field
}

fn make_panel(mtm: MainThreadMarker) -> Retained<NSPanel> {
    let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(mtm),
        rect(0.0, 0.0, WIDTH, SIDE),
        style,
        NSBackingStoreType::Buffered,
        false,
    );
    unsafe { panel.setReleasedWhenClosed(false) };
    panel.setFloatingPanel(true);
    panel.setBecomesKeyOnlyIfNeeded(true);
    panel.setHidesOnDeactivate(false);
    panel.setLevel(NSStatusWindowLevel + 1);
    panel.setOpaque(false);
    panel.setBackgroundColor(Some(&NSColor::clearColor()));
    panel.setHasShadow(true);
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    let background: Retained<Background> =
        unsafe { msg_send![Background::alloc(mtm), initWithFrame: rect(0.0, 0.0, WIDTH, SIDE)] };
    background.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    background.setWantsLayer(true);
    if let Some(layer) = background.layer() {
        layer.setMasksToBounds(true);
        layer.setCornerRadius(RADIUS);
    }
    panel.setContentView(Some(&background));
    panel
}

fn place(mtm: MainThreadMarker) -> Place {
    let small = Size { width: NO_NOTCH_WIDTH, height: NO_NOTCH_HEIGHT };
    let screens = NSScreen::screens(mtm);
    let screen = screens.iter().find(|s| s.safeAreaInsets().top > 0.0).or_else(|| NSScreen::mainScreen(mtm));
    let Some(screen) = screen else {
        return Place { screen: rect(0.0, 0.0, WIDTH, WIDTH), top: 0.0, notch: small };
    };
    let frame = screen.frame();
    let top = screen.safeAreaInsets().top;
    let sides = screen.auxiliaryTopLeftArea().size.width + screen.auxiliaryTopRightArea().size.width;
    let notch = if top > 0.0 && sides > 0.0 {
        Size { width: (frame.size.width - sides).clamp(NO_NOTCH_WIDTH, WIDTH), height: top }
    } else {
        small
    };
    Place { screen: frame, top, notch }
}

fn send(action: &Action) {
    if let Ok(line) = serde_json::to_string(action) {
        let mut out = std::io::stdout();
        let _ = writeln!(out, "{line}").and_then(|()| out.flush());
    }
}

fn terminal_app() -> Option<Retained<NSRunningApplication>> {
    let regular = |pid| {
        NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
            .is_some_and(|app| app.activationPolicy() == NSApplicationActivationPolicy::Regular)
    };
    let parent = rustix::process::getppid().map_or(0, |pid| pid.as_raw_nonzero().get());
    card::terminal(parent, process::parent, regular)
        .and_then(NSRunningApplication::runningApplicationWithProcessIdentifier)
}

pub(crate) fn run() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let inbox: Shared = Arc::default();
    let reading = Arc::clone(&inbox);
    thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if let Ok(board) = serde_json::from_str::<Board>(&line) {
                reading.lock().board = Some(board);
            }
        }
        reading.lock().ended = true;
    });
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let notch = Notch::new(mtm, inbox);
    app.setDelegate(Some(ProtocolObject::from_ref(&*notch)));
    app.run();
}

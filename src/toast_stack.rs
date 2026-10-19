//! One native viewport for all immutable toast snapshots. Empty pixels are
//! excluded from the HWND region; the number of snapshots never limits the UI.
use std::time::{Duration, Instant};

use gpui::component::{
    ActiveTheme, Icon, IconName, Sizable,
    button::{Button, ButtonVariants},
};
use gpui::{prelude::*, *};
use win_notifi_toast::{
    placement::{Corner, Placement, Rect},
    reminders::{ReminderId, ToastSnapshot},
};

use crate::{native, settings_ui::DesktopSettings};

const TRANSITION: Duration = Duration::from_millis(220);
const BORDER_PULSE: Duration = Duration::from_millis(1200);
const BORDER_WIDTH: f32 = 4.;
const GAP: f32 = 12.;
const FONT_SIZE: f32 = 16.;
const LINE_HEIGHT: f32 = 24.;
const BODY_TOP: f32 = 48.;
const BODY_INSET: f32 = 16.;
const CARD_CHROME: f32 = BODY_TOP + 16.;

#[derive(Clone, Copy, PartialEq)]
enum Source {
    Live,
    Demo,
}

#[derive(Default)]
struct Stacks {
    live: Option<WindowHandle<ToastStack>>,
    demo: Option<WindowHandle<ToastStack>>,
    demo_sequence: u64,
}
impl Global for Stacks {}

pub fn initialize(cx: &mut App) {
    cx.set_global(Stacks::default());
}

pub fn sync(snapshots: Vec<ToastSnapshot>, cx: &mut App) -> anyhow::Result<()> {
    let handle = cx.global::<Stacks>().live;
    let handle = update(handle, snapshots, Source::Live, cx)?;
    cx.global_mut::<Stacks>().live = handle;
    // The demonstration uses the same renderer and still receives hover polling.
    if let Some(demo) = cx.global::<Stacks>().demo {
        let _ = demo.update(cx, |view, window, cx| view.poll(window, cx));
    }
    Ok(())
}

pub fn show_demo(cx: &mut App) -> anyhow::Result<()> {
    let old = cx.global::<Stacks>().demo;
    if old.is_some_and(|handle| handle.read(cx).is_ok_and(|view| !view.snapshots.is_empty())) {
        return Ok(());
    }
    let sequence = cx.global::<Stacks>().demo_sequence;
    cx.global_mut::<Stacks>().demo_sequence = sequence.wrapping_add(3);
    let snapshots = [
        ("Пора размяться. Встаньте, расправьте плечи и посмотрите вдаль. Ёжик тоже делает перерыв!", 0x5285f5),
        ("Стакан воды. Небольшая пауза: налейте воды и отдохните от экрана.", 0x29a879),
        ("Проверка русского текста. Съешь ещё этих мягких французских булок, да выпей чаю. 0123456789", 0xb17bea),
    ].into_iter().enumerate().map(|(index, (text, border))| ToastSnapshot {
        reminder_id: sequence + index as u64, text: text.into(), border, sound: false, sequence: sequence + index as u64,
    }).collect();
    let handle = update(old, snapshots, Source::Demo, cx)?;
    cx.global_mut::<Stacks>().demo = handle;
    Ok(())
}

fn update(
    handle: Option<WindowHandle<ToastStack>>,
    snapshots: Vec<ToastSnapshot>,
    source: Source,
    cx: &mut App,
) -> anyhow::Result<Option<WindowHandle<ToastStack>>> {
    let placement = cx.global::<DesktopSettings>().0.placement()?;
    let bottom = matches!(
        cx.global::<DesktopSettings>().0.settings().corner,
        Corner::BottomLeft | Corner::BottomRight
    );
    if let Some(handle) = handle
        && handle
            .update(cx, |view, window, cx| {
                view.reconcile(snapshots.clone(), placement, bottom, window, cx);
                view.poll(window, cx);
            })
            .is_ok()
    {
        return Ok(Some(handle));
    }
    if snapshots.is_empty() {
        return Ok(None);
    }
    let Rect {
        x,
        y,
        width,
        height,
    } = placement.viewport;
    let scale = placement.scale;
    let handle = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                point(px(x as f32 / scale), px(y as f32 / scale)),
                size(px(width as f32 / scale), px(height as f32 / scale)),
            ))),
            titlebar: None,
            kind: WindowKind::PopUp,
            focus: false,
            show: false,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            window_background: WindowBackgroundAppearance::Transparent,
            ..Default::default()
        },
        |window, cx| {
            window.set_window_title(if source == Source::Live {
                "Стопка напоминаний"
            } else {
                "Тестовая стопка напоминаний"
            });
            cx.new(|cx| ToastStack::new(snapshots, placement, bottom, source, cx))
        },
    )?;
    handle.update(cx, |_, window, cx| {
        if let Ok(task) = native::toast_region(window, cx, vec![]) {
            task.detach();
        }
        let task = native::place_toast(window, cx, x, y, width, height)?;
        cx.spawn(async move |_, cx| {
            if let Err(error) = task.await {
                eprintln!("Не удалось разместить стопку: {error:#}");
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            }
        })
        .detach();
        anyhow::Ok(())
    })??;
    Ok(Some(handle))
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Frame {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    opacity: f32,
    content: f32,
}
impl Frame {
    fn between(self, end: Self, progress: f32) -> Self {
        let value = |a, b| a + (b - a) * progress;
        Self {
            x: value(self.x, end.x),
            y: value(self.y, end.y),
            width: value(self.width, end.width),
            height: value(self.height, end.height),
            opacity: value(self.opacity, end.opacity),
            content: value(self.content, end.content),
        }
    }
}
struct Card {
    snapshot: ToastSnapshot,
    height: f32,
    measured_width: f32,
    start: Frame,
    target: Frame,
    since: Instant,
    entering: bool,
    departing: bool,
}
impl Card {
    fn frame(&self, now: Instant) -> Frame {
        let progress =
            (now.duration_since(self.since).as_secs_f32() / TRANSITION.as_secs_f32()).clamp(0., 1.);
        self.start
            .between(self.target, 1. - (1. - progress).powi(3))
    }
    fn target(&mut self, target: Frame, now: Instant) {
        if self.entering {
            self.start = Frame {
                y: target.y - 24.,
                opacity: 0.,
                ..target
            };
            self.target = target;
            self.since = now;
            self.entering = false;
        } else if self.target != target {
            self.start = self.frame(now);
            self.target = target;
            self.since = now;
        }
    }
}

struct Drag {
    id: ReminderId,
    origin: f32,
    delta: f32,
}

struct ToastStack {
    source: Source,
    snapshots: Vec<ToastSnapshot>,
    cards: Vec<Card>,
    placement: Placement,
    bottom: bool,
    expanded: bool,
    keyboard: bool,
    focus: FocusHandle,
    selected: usize,
    scroll: f32,
    total_height: f32,
    hover_until: Instant,
    pulse_since: Instant,
    regions: Vec<Rect>,
    drag: Option<Drag>,
}

impl ToastStack {
    fn new(
        snapshots: Vec<ToastSnapshot>,
        placement: Placement,
        bottom: bool,
        source: Source,
        cx: &mut Context<Self>,
    ) -> Self {
        // Native Alt+F4 and unexpected window removal must close their snapshots,
        // otherwise a vanished sounding toast could keep the common signal alive.
        cx.on_release(|this, cx| {
            if this.source == Source::Live {
                for toast in &this.snapshots {
                    crate::reminder_ui::close(toast.reminder_id, cx);
                }
            }
        })
        .detach();
        let now = Instant::now();
        Self {
            source,
            snapshots: vec![],
            cards: vec![],
            placement,
            bottom,
            expanded: false,
            keyboard: false,
            focus: cx.focus_handle(),
            selected: 0,
            scroll: 0.,
            total_height: 0.,
            hover_until: now,
            pulse_since: now,
            regions: vec![],
            drag: None,
        }
        .with_snapshots(snapshots)
    }
    fn with_snapshots(mut self, snapshots: Vec<ToastSnapshot>) -> Self {
        self.accept(snapshots);
        self
    }
    fn accept(&mut self, snapshots: Vec<ToastSnapshot>) {
        let now = Instant::now();
        for card in &mut self.cards {
            if !snapshots
                .iter()
                .any(|toast| toast.reminder_id == card.snapshot.reminder_id)
                && !card.departing
            {
                card.departing = true;
                let direction = if card.frame(now).x < card.target.x - 1. {
                    -1.
                } else {
                    1.
                };
                card.target(
                    Frame {
                        x: card.target.x + direction * card.target.width,
                        opacity: 0.,
                        ..card.target
                    },
                    now,
                );
            }
        }
        for snapshot in &snapshots {
            if !self.cards.iter().any(|card| {
                card.snapshot.reminder_id == snapshot.reminder_id
                    && card.snapshot.sequence == snapshot.sequence
                    && !card.departing
            }) {
                self.cards.push(Card {
                    snapshot: snapshot.clone(),
                    height: 0.,
                    measured_width: 0.,
                    start: Frame::default(),
                    target: Frame::default(),
                    since: now,
                    entering: true,
                    departing: false,
                });
            }
        }
        self.snapshots = snapshots;
        self.selected = self.selected.min(self.snapshots.len().saturating_sub(1));
    }
    fn reconcile(
        &mut self,
        snapshots: Vec<ToastSnapshot>,
        placement: Placement,
        bottom: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let moved = self.placement.viewport != placement.viewport
            || self.placement.scale != placement.scale;
        if moved {
            self.placement = placement;
            self.regions.clear();
            let r = placement.viewport;
            if let Ok(task) = native::place_toast(window, cx, r.x, r.y, r.width, r.height) {
                task.detach();
            }
        }
        let changed = self.snapshots != snapshots || self.bottom != bottom || moved;
        self.bottom = bottom;
        if self.snapshots != snapshots {
            self.accept(snapshots);
        }
        if changed {
            cx.notify();
        }
    }
    fn poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let hovered = native::cursor_position().is_some_and(|(x, y)| {
            let x = x - self.placement.viewport.x;
            let y = y - self.placement.viewport.y;
            // A small inter-card gap should not collapse the stack underneath
            // the pointer. The HWND itself still excludes those empty pixels.
            let left = self.regions.iter().map(|r| r.x).min().unwrap_or(0);
            let top = self.regions.iter().map(|r| r.y).min().unwrap_or(0);
            let right = self
                .regions
                .iter()
                .map(|r| r.x + r.width)
                .max()
                .unwrap_or(0);
            let bottom = self
                .regions
                .iter()
                .map(|r| r.y + r.height)
                .max()
                .unwrap_or(0);
            x >= left && x < right && y >= top && y < bottom
        });
        let now = Instant::now();
        if hovered {
            self.hover_until = now + Duration::from_millis(180);
        }
        if self.keyboard && !window.is_window_active() {
            self.keyboard = false;
            native::toast_allow_focus(window, false);
        }
        let expanded = hovered || now < self.hover_until || self.keyboard || self.drag.is_some();
        if expanded != self.expanded {
            self.expanded = expanded;
            self.scroll = 0.;
            cx.notify();
        }
    }
    fn dismiss(&mut self, id: ReminderId, cx: &mut Context<Self>) {
        if self.source == Source::Live {
            crate::reminder_ui::close(id, cx);
        }
        let snapshots = self
            .snapshots
            .iter()
            .filter(|toast| toast.reminder_id != id)
            .cloned()
            .collect();
        self.accept(snapshots);
        self.drag = None;
        cx.notify();
    }
    fn finish_drag(&mut self, cx: &mut Context<Self>) {
        if let Some(drag) = self.drag.take() {
            let now = Instant::now();
            if let Some(card) = self
                .cards
                .iter_mut()
                .find(|card| card.snapshot.reminder_id == drag.id && !card.departing)
            {
                card.start = card.frame(now);
                card.start.x += drag.delta;
                card.since = now;
            }
            if drag.delta.abs()
                >= (self.placement.viewport.width as f32 / self.placement.scale * 0.3).min(96.)
            {
                self.dismiss(drag.id, cx);
            } else {
                cx.notify();
            }
        }
    }
    fn wheel(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        if !self.expanded {
            self.expanded = true;
        }
        let delta = f32::from(event.delta.pixel_delta(px(LINE_HEIGHT)).y);
        self.scroll =
            (self.scroll + if self.bottom { delta } else { -delta }).clamp(0., self.max_scroll());
        cx.stop_propagation();
        cx.notify();
    }
    fn max_scroll(&self) -> f32 {
        (self.total_height - self.placement.viewport.height as f32 / self.placement.scale).max(0.)
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let height = self.placement.viewport.height as f32 / self.placement.scale;
        match event.keystroke.key.as_str() {
            "escape" => {
                self.keyboard = false;
                self.expanded = false;
                self.scroll = 0.;
                native::toast_allow_focus(window, false);
            }
            "pagedown" => self.scroll = (self.scroll + height * 0.8).min(self.max_scroll()),
            "pageup" => self.scroll = (self.scroll - height * 0.8).max(0.),
            "home" => {
                self.scroll = 0.;
                self.selected = 0;
            }
            "end" => {
                self.scroll = self.max_scroll();
                self.selected = self.snapshots.len().saturating_sub(1);
            }
            "down" | "tab" => {
                self.selected = (self.selected + 1).min(self.snapshots.len().saturating_sub(1));
                self.reveal_selected();
            }
            "up" => {
                self.selected = self.selected.saturating_sub(1);
                self.reveal_selected();
            }
            "enter" | "backspace" | "delete" => {
                if let Some(toast) = self.snapshots.get(self.selected) {
                    self.dismiss(toast.reminder_id, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
    fn reveal_selected(&mut self) {
        let mut offset = 0.;
        let height = self.placement.viewport.height as f32 / self.placement.scale;
        for (index, toast) in self.snapshots.iter().enumerate() {
            let card_height = self
                .cards
                .iter()
                .find(|c| c.snapshot.reminder_id == toast.reminder_id && !c.departing)
                .map_or(100., |c| c.height);
            if index == self.selected {
                if offset < self.scroll {
                    self.scroll = offset;
                } else if offset + card_height > self.scroll + height {
                    self.scroll = (offset + card_height - height).max(0.);
                }
                break;
            }
            offset += card_height + GAP;
        }
        self.scroll = self.scroll.min(self.max_scroll());
    }
}

impl Render for ToastStack {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        self.cards
            .retain(|card| !card.departing || now.duration_since(card.since) < TRANSITION);
        if self.cards.is_empty() {
            window.remove_window();
        }
        let surface = cx.theme().background;
        let text = cx.theme().foreground;
        let muted = cx.theme().muted_foreground;
        let pulse = now.duration_since(self.pulse_since).as_secs_f32() / BORDER_PULSE.as_secs_f32();
        let border_opacity = 0.25 + 0.75 * (1. + (pulse * std::f32::consts::TAU).cos()) / 2.;
        let width = self.placement.viewport.width as f32 / self.placement.scale;
        let viewport_height = self.placement.viewport.height as f32 / self.placement.scale;
        for card in &mut self.cards {
            if card.measured_width != width {
                let string: SharedString = card.snapshot.text.clone().into();
                let run = TextRun {
                    len: string.len(),
                    font: font("Segoe UI"),
                    color: text,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                card.height = window
                    .text_system()
                    .shape_text(
                        string,
                        px(FONT_SIZE),
                        &[run],
                        Some(px((width - BODY_INSET * 2.).max(1.))),
                        None,
                    )
                    .map(|lines| {
                        lines
                            .iter()
                            .map(|line| f32::from(line.size(px(LINE_HEIGHT)).height))
                            .sum::<f32>()
                            + CARD_CHROME
                    })
                    .unwrap_or(180.)
                    .max(CARD_CHROME + LINE_HEIGHT);
                card.measured_width = width;
            }
        }
        self.total_height = self
            .snapshots
            .iter()
            .filter_map(|toast| {
                self.cards
                    .iter()
                    .find(|c| c.snapshot.reminder_id == toast.reminder_id && !c.departing)
            })
            .map(|c| c.height + GAP)
            .sum::<f32>()
            - GAP;
        self.scroll = self.scroll.clamp(0., self.max_scroll());
        let front_height = self
            .snapshots
            .first()
            .and_then(|toast| {
                self.cards
                    .iter()
                    .find(|c| c.snapshot.reminder_id == toast.reminder_id && !c.departing)
            })
            .map_or(CARD_CHROME + LINE_HEIGHT, |c| c.height);
        let mut offset = 0.;
        for (index, snapshot) in self.snapshots.iter().enumerate() {
            if let Some(card) = self
                .cards
                .iter_mut()
                .find(|card| card.snapshot.reminder_id == snapshot.reminder_id && !card.departing)
            {
                let compact_scale = 1. - index.min(2) as f32 * 0.055;
                let card_width = if self.expanded {
                    width
                } else {
                    width * compact_scale
                };
                let height = if self.expanded {
                    card.height
                } else {
                    front_height.min(viewport_height - 24.).max(40.) * compact_scale
                };
                let y = if self.expanded {
                    if self.bottom {
                        viewport_height - offset - height + self.scroll
                    } else {
                        offset - self.scroll
                    }
                } else if self.bottom {
                    viewport_height
                        - front_height.min(viewport_height - 24.).max(40.)
                        - index.min(2) as f32 * 10.
                } else {
                    front_height.min(viewport_height - 24.).max(40.) - height
                        + index.min(2) as f32 * 10.
                };
                card.target(
                    Frame {
                        x: (width - card_width) / 2.,
                        y,
                        width: card_width,
                        height,
                        opacity: if self.expanded || index < 3 { 1. } else { 0. },
                        content: if self.expanded || index == 0 { 1. } else { 0. },
                    },
                    now,
                );
                offset += card.height + GAP;
            }
        }
        let mut root = div()
            .id("toast-stack")
            .track_focus(&self.focus)
            .size_full()
            .overflow_hidden()
            .font_family("Segoe UI")
            .text_size(px(FONT_SIZE))
            .line_height(px(LINE_HEIGHT))
            .text_color(text)
            .on_scroll_wheel(cx.listener(|this, event, _, cx| this.wheel(event, cx)))
            .on_key_down(cx.listener(Self::key))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if let Some(drag) = &mut this.drag {
                    drag.delta = f32::from(event.position.x) - drag.origin;
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.finish_drag(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.finish_drag(cx)),
            );
        let mut regions = Vec::new();
        // Front-most card must be painted last. Departing cards keep their own
        // snapshot and are never rebound to another reminder during reflow.
        let mut order: Vec<usize> = (0..self.cards.len()).collect();
        order.sort_by_key(|i| {
            if self.cards[*i].departing {
                return usize::MAX;
            }
            self.snapshots
                .iter()
                .position(|toast| toast.reminder_id == self.cards[*i].snapshot.reminder_id)
                .unwrap_or(0)
        });
        for index in order.into_iter().rev() {
            let card = &self.cards[index];
            let mut frame = card.frame(now);
            if let Some(drag) = &self.drag
                && drag.id == card.snapshot.reminder_id
            {
                frame.x += drag.delta;
                frame.opacity *= (1. - drag.delta.abs() / width).max(0.2);
            }
            if frame.opacity < 0.01 {
                continue;
            }
            let left = frame.x.max(0.);
            let top = frame.y.max(0.);
            let right = (frame.x + frame.width).min(width);
            let bottom = (frame.y + frame.height).min(viewport_height);
            if right <= left || bottom <= top {
                continue;
            }
            let scale = self.placement.scale;
            regions.push(Rect {
                x: (left * scale).floor() as i32,
                y: (top * scale).floor() as i32,
                width: ((right * scale).ceil() - (left * scale).floor()) as i32,
                height: ((bottom * scale).ceil() - (top * scale).floor()) as i32,
            });
            let id = card.snapshot.reminder_id;
            let departing = card.departing;
            let selected = self.keyboard
                && self
                    .snapshots
                    .get(self.selected)
                    .is_some_and(|s| s.reminder_id == id);
            let content_opacity = frame.content.clamp(0., 1.);
            let accent: Hsla = rgb(card.snapshot.border).into();
            let close_top = (16. - frame.y).max(16.).min((frame.height - 44.).max(0.));
            let content = div()
                .size_full()
                .opacity(content_opacity)
                .child(
                    div()
                        .absolute()
                        .top(px(16. - BORDER_WIDTH))
                        .left(px(BODY_INSET - BORDER_WIDTH))
                        .text_color(accent)
                        .child(Icon::new(IconName::Bell).with_size(px(18.))),
                )
                .when(card.snapshot.sound, |e| {
                    e.child(
                        div()
                            .absolute()
                            .top(px(16. - BORDER_WIDTH))
                            .left(px(44. - BORDER_WIDTH))
                            .text_size(px(12.))
                            .text_color(muted)
                            .child("Со звуком"),
                    )
                })
                .child(
                    div()
                        .absolute()
                        .top(px(BODY_TOP - BORDER_WIDTH))
                        .left(px(BODY_INSET - BORDER_WIDTH))
                        .w(px((width - BODY_INSET * 2.).max(1.)))
                        .child(card.snapshot.text.clone()),
                )
                .child(
                    div()
                        .absolute()
                        .right(px(14. - BORDER_WIDTH))
                        .top(px(close_top - BORDER_WIDTH))
                        .child(
                        Button::new(("close-toast", id))
                            .icon(IconName::Close)
                            .accessibility_label("Закрыть напоминание")
                            .ghost()
                            .with_size(px(28.))
                            .tab_stop(false)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !departing {
                                    this.dismiss(id, cx);
                                }
                            })),
                    ),
                );
            let mut element = div()
                .id(("toast", card.snapshot.sequence))
                .absolute()
                .left(px(frame.x))
                .top(px(frame.y))
                .w(px(frame.width))
                .h(px(frame.height))
                .opacity(frame.opacity.clamp(0., 1.))
                .overflow_hidden()
                .bg(if selected { cx.theme().muted } else { surface })
                .rounded(px(4.))
                .border(px(BORDER_WIDTH))
                .border_color(accent.opacity(border_opacity))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        if departing {
                            return;
                        }
                        let mouse_x = f32::from(event.position.x);
                        let mouse_y = f32::from(event.position.y);
                        if mouse_x >= frame.x + frame.width - 44.
                            && mouse_x < frame.x + frame.width - 14.
                            && mouse_y >= frame.y + close_top
                            && mouse_y < frame.y + close_top + 28.
                        {
                            return; // Close remains a click target, never a drag handle.
                        }
                        this.drag = Some(Drag {
                            id,
                            origin: mouse_x,
                            delta: 0.,
                        });
                        this.expanded = true;
                        cx.notify();
                    }),
                );
            if frame.content > 0.01 {
                element = element.child(content);
            }
            root = root.child(element);
        }
        if regions != self.regions {
            self.regions = regions.clone();
            if let Ok(task) = native::toast_region(window, cx, regions) {
                cx.spawn(async move |_, _| {
                    if let Err(error) = task.await {
                        eprintln!("Не удалось обновить область стопки: {error:#}");
                    }
                })
                .detach();
            }
        }
        if !self.cards.is_empty() {
            window.request_animation_frame();
        }
        root
    }
}
